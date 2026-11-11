use super::*;

/// 一首曲目应乘的线性增益。
///
/// 回退：专辑模式没有专辑增益 → 曲目增益；曲目模式没有曲目增益 →
/// 专辑增益；都没有 → `fallback_db`（peak 视为无）。`clip_protect` 且
/// 有峰值时，增益不超过 `1 / peak`；结果钳到 `0.0..=4.0`（+12 dB）。
pub(crate) fn replaygain_factor(cfg: &ReplayGainConfig, rg: &zeo_metadata::ReplayGain) -> f32 {
    if cfg.mode == ReplayGainMode::Off {
        return 1.0;
    }
    let (gain_db, peak) = match cfg.mode {
        ReplayGainMode::Album => match rg.album_gain.or(rg.track_gain) {
            Some(db) => (db, rg.album_peak.or(rg.track_peak)),
            None => (cfg.fallback_db, None),
        },
        _ => match rg.track_gain.or(rg.album_gain) {
            Some(db) => (db, rg.track_peak.or(rg.album_peak)),
            None => (cfg.fallback_db, None),
        },
    };
    let db = gain_db + cfg.preamp_db;
    let mut factor = 10f32.powf(db / 20.0);
    if cfg.clip_protect
        && let Some(peak) = peak
    {
        factor = factor.min(1.0 / peak);
    }
    factor.clamp(0.0, 4.0)
}

/// 输出设备的变化（cpal 错误回调转来）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceEvent {
    /// 已自动改道到新的默认设备，流照常（macOS）。
    Rerouted,
    /// 流失效，要在新的默认设备上重建（Windows 换默认设备）。
    Invalidated,
    /// 没有可用的输出设备。
    Lost,
}

pub enum EngineCmd {
    Load {
        path: PathBuf,
        queue: Vec<String>,
    },
    /// 装载但不播放，停在 `time` 秒（启动时恢复上次的曲目）。
    Cue {
        path: PathBuf,
        queue: Vec<String>,
        time: f64,
    },
    /// 跳到当前队列的第 `n` 首。
    Jump(i64),
    Play,
    Pause,
    Toggle,
    Stop,
    Seek(f64),
    SeekBy(f64),
    SetVolume(f64),
    Next,
    Previous,
    /// 设置循环模式（队列按新模式重排，当前曲目不变）。
    SetLoopMode(LoopMode),
    /// 把路径追加到当前队尾（当前曲目与播放状态不变）。
    QueueAdd(Vec<String>),
    /// 输出设备事件（见 [`DeviceEvent`]）。
    Device(DeviceEvent),
    /// ReplayGain 设置变了：存进 `rg_config`，当前曲目立即重算增益。
    SetReplayGain(ReplayGainConfig),
    /// 文件马上要写回标签（原子替换）前释放解码器：丢掉 `Loaded` 让
    /// 文件句柄关闭——Windows 上打开中的文件不能改名。`done` 通知
    /// 发送方句柄真的放下了（写文件必须等它）。位置与播放态由发送方
    /// 记好，`Reload` 传回来。
    Release {
        done: std::sync::mpsc::Sender<()>,
    },
    /// `Release` 之后、文件已替换：按 `MetaCache` 里的新元数据重载
    /// 当前曲目（标题 / 封面刷新），seek 回 `position` 秒，恢复
    /// `playing`。没有当前曲目就不做。
    Reload {
        position: f64,
        playing: bool,
    },
}

/// 曲目公开状态（跨线程读）：冷字段——换曲 / 封面就绪 / 队列 / 出错
/// 才变。`Shared::track` 用 `Mutex<Arc<..>>` 持有它：读侧克隆 `Arc`
/// 只加引用计数，写侧整份换新 `Arc`（指针判变化——不能原地改）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackPublic {
    pub path: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// 大封面 `data:` URI（`Arc` 共享：快照克隆只加引用计数——轮询
    /// 每 100ms 取一次快照，克隆 String 会是每秒几 MB 的堆流量）。
    /// 全屏模糊背景也复用它（滤镜模糊，见 `ui::now_playing`）。
    pub artwork: Option<Arc<str>>,
    /// 这首的封面已生成（`artwork` 为 `None` 时即确实没有封面）。
    pub covers_ready: bool,
    pub duration: f64,
    pub queue: Arc<Vec<String>>,
    pub queue_index: i64,
    pub error: Option<String>,
}

/// 引擎共享状态（引擎线程写、cpal 回调与读取方读）。
pub struct Shared {
    pub playing: AtomicBool,
    pub loading: AtomicBool,
    pub ready: AtomicBool,
    pub volume: AtomicU32, // f32 位
    pub played: AtomicU64, // 输出帧计数（含 seek 基线）
    /// 本次装载实际播出的输出帧数（`load_track` 清零；seek 不动它）。
    pub listened: AtomicU64,
    pub dev_rate: AtomicU32,
    pub ring: Ring,
    /// 冷字段的共享快照（所有写路径换新 `Arc`，见 [`TrackPublic`]）。
    pub track: Mutex<Arc<TrackPublic>>,
    /// 淡入淡出时长（毫秒；0 关闭）。
    pub fade_ms: AtomicU32,
    /// 淡变目标（f32 位，0 或 1；回调朝它推进 `gain`）。
    pub fade_target: AtomicU32,
    /// 当前淡变增益（f32 位，0..1；播放中由回调写，停着时由引擎写）。
    pub gain: AtomicU32,
    /// 循环模式（[`LoopMode`] 的判别值）。
    pub loop_mode: AtomicU8,
    /// 封面线程代次：换曲 +1，晚到的旧结果自检失效。
    pub cover_gen: AtomicU64,
    /// 输出设备变化时是否暂停（设置项；设备丢失时无论如何都暂停）。
    pub pause_on_device_change: AtomicBool,
    /// 播放统计阈值：听过整首的百分比算一次播放（0 = 不用此条件）。
    pub stats_min_percent: AtomicU32,
    /// 播放统计阈值：实际播出多少秒算一次播放（0 = 不用此条件）。
    pub stats_min_secs: AtomicU32,
    /// 播放记录代次：每记一次播放 +1（界面据此刷新统计）。
    pub plays_gen: AtomicU64,
    /// 当前曲目的 ReplayGain 线性增益（f32 位；装载 / 设置变更时由
    /// 引擎线程写，输出回调读）。
    pub rg_factor: AtomicU32,
    /// ReplayGain 设置（引擎线程与 `SetReplayGain` 的发送方用；
    /// 输出回调不碰——它只读 `rg_factor`，避免在回调里取锁）。
    pub rg_config: Mutex<ReplayGainConfig>,
    /// EQ 配置代次：回调每块比较，变了才 `try_lock` 取新配置重算系数
    /// （拿不到就下一块再试，回调永不阻塞）。
    pub eq_gen: AtomicU64,
    /// 均衡器设置（`set_eq` 的调用方写；回调只在 `try_lock` 成功时读）。
    pub eq_config: Mutex<crate::eq::EqConfig>,
}

impl Shared {
    pub fn current_time(&self) -> f64 {
        let rate = self.dev_rate.load(Ordering::Relaxed).max(1) as f64;
        self.played.load(Ordering::Relaxed) as f64 / rate
    }
    pub fn volume(&self) -> f64 {
        f32::from_bits(self.volume.load(Ordering::Relaxed)) as f64
    }

    pub fn loop_mode(&self) -> LoopMode {
        LoopMode::from_bits(self.loop_mode.load(Ordering::Relaxed))
    }
}

/// 面向 TSX 的完整状态快照：热字段（进度、开关，约 10 Hz 变）拍平，
/// 冷字段（曲目信息）只带一份共享 `Arc`——UI 轮询按 `Arc::ptr_eq`
/// 判曲目是否变了，不用每次克隆十几份字段再逐个比较。
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub is_playing: bool,
    pub is_loading: bool,
    pub is_ready: bool,
    pub current_time: f64,
    pub volume: f64,
    pub loop_mode: LoopMode,
    /// 当前曲目状态（冷字段的共享快照）。
    pub track: Arc<TrackPublic>,
}

impl Shared {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            is_playing: self.playing.load(Ordering::Relaxed),
            is_loading: self.loading.load(Ordering::Relaxed),
            is_ready: self.ready.load(Ordering::Relaxed),
            current_time: (self.current_time() * 100.0).round() / 100.0, // timeupdate 精度 0.01s
            volume: self.volume(),
            loop_mode: self.loop_mode(),
            track: self.track.lock().unwrap().clone(),
        }
    }
}

/// 引擎句柄：发送命令 + 读共享状态。
#[derive(Clone)]
pub struct EngineHandle {
    pub(crate) tx: std::sync::mpsc::Sender<EngineCmd>,
    pub shared: Arc<Shared>,
}

impl EngineHandle {
    pub fn start() -> EngineHandle {
        let (tx, rx) = std::sync::mpsc::channel::<EngineCmd>();
        let shared = Arc::new(Shared {
            playing: AtomicBool::new(false),
            loading: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            volume: AtomicU32::new(0.8f32.to_bits()), // 默认音量 0.8（ap-spec）
            fade_ms: AtomicU32::new(DEFAULT_FADE_MS),
            fade_target: AtomicU32::new(1f32.to_bits()),
            gain: AtomicU32::new(0f32.to_bits()),
            played: AtomicU64::new(0),
            listened: AtomicU64::new(0),
            dev_rate: AtomicU32::new(48_000),
            ring: Ring::new(),
            track: Mutex::new(Arc::new(TrackPublic::default())),
            loop_mode: AtomicU8::new(0),
            cover_gen: AtomicU64::new(0),
            pause_on_device_change: AtomicBool::new(true),
            stats_min_percent: AtomicU32::new(DEFAULT_STATS_MIN_PERCENT),
            stats_min_secs: AtomicU32::new(DEFAULT_STATS_MIN_SECS),
            plays_gen: AtomicU64::new(0),
            rg_factor: AtomicU32::new(1f32.to_bits()),
            rg_config: Mutex::new(ReplayGainConfig::default()),
            eq_gen: AtomicU64::new(0),
            eq_config: Mutex::new(crate::eq::EqConfig::default()),
        });
        let media = MediaSession::start(tx.clone());
        std::thread::Builder::new()
            .name("zeo-player-audio".into())
            .spawn({
                let shared = shared.clone();
                let tx = tx.clone();
                move || run(rx, tx, shared, media)
            })
            .expect("spawn audio thread");
        EngineHandle { tx, shared }
    }

    pub fn send(&self, cmd: EngineCmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared.snapshot()
    }
}
