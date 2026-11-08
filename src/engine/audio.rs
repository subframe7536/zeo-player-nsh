//! 播放引擎：symphonia 解码 →（rubato 重采样）→ cpal 输出。
//! 引擎线程拥有解码器与命令循环；cpal 回调经共享环形缓冲取数；
//! UI 通过快照 + 轮询任务读取状态（PlayerState 等价字段）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::Resampler;
use serde::{Deserialize, Serialize};

use crate::engine::codec::Source;
use crate::engine::media::MediaSession;
use crate::metadata::MetaCache;

/// 环形缓冲的填充目标（秒）。CPU 忙时引擎线程可能几百毫秒排不上，
/// 缓冲要够撑过去；淡变、音量、暂停都在输出回调里生效，缓冲长短不影响
/// 它们的响应，seek / 换曲会作废整段缓冲。
const RING_TARGET_SECS: usize = 2;
/// 缓冲容量（秒）：目标之外留出一个大解码包的余量。
const RING_CAPACITY_SECS: usize = 3;
/// 默认淡入淡出时长。
pub const DEFAULT_FADE_MS: u32 = 200;
/// 默认播放统计阈值：听过整首的百分比。
pub const DEFAULT_STATS_MIN_PERCENT: u32 = 60;
/// 默认播放统计阈值：实际播出秒数。
pub const DEFAULT_STATS_MIN_SECS: u32 = 30;

/// 每轮最多解码的包数（防止一轮占太久，命令响应变慢）。
const MAX_PACKETS_PER_TICK: usize = 64;
/// 播放中两轮之间最多等多久（有命令时立即醒）。缓冲有 2 秒，不必频繁醒。
const PLAYING_TICK: Duration = Duration::from_millis(10);
/// 停着时两轮之间最多等多久。
const IDLE_TICK: Duration = Duration::from_millis(100);
/// 输出设备缓冲的目标时长（毫秒）：不少设备的默认缓冲只有 ~10ms——系统
/// CPU 忙时回调线程稍一排队就欠载（环形缓冲再大也救不了设备侧的空窗）。
/// 40ms 把调度余量放大几倍，对交互延迟仍无感。
const OUTPUT_BUFFER_MS: u32 = 40;

/// 解码线程 → 输出回调的样本环：单生产者（引擎线程）单消费者（cpal 回调），
/// 无锁。
///
/// 以前是 `Mutex<VecDeque>`：CPU 忙时引擎线程常在持锁期间被抢占，实时回调
/// 只能等它，于是输出断续。现在两边只读写原子量：`write` / `read` 是累计
/// 样本数，`discard` 之前的样本作废（seek / 换曲时由引擎设成当前 `write`，
/// 回调下次取数时跳过）。
pub struct Ring {
    slots: OnceLock<Box<[AtomicU32]>>,
    write: AtomicU64,
    read: AtomicU64,
    discard: AtomicU64,
}

impl Ring {
    pub fn new() -> Ring {
        Ring {
            slots: OnceLock::new(),
            write: AtomicU64::new(0),
            read: AtomicU64::new(0),
            discard: AtomicU64::new(0),
        }
    }

    /// 分配容量（样本数）；只在第一次调用时生效。
    fn init(&self, capacity: usize) {
        let _ = self
            .slots
            .set((0..capacity.max(1)).map(|_| AtomicU32::new(0)).collect());
    }

    /// 分配的容量（样本数；未分配为 0）。
    fn capacity(&self) -> usize {
        self.slots.get().map_or(0, |s| s.len())
    }

    /// 待播的样本数。
    pub fn len(&self) -> usize {
        let w = self.write.load(Ordering::Acquire);
        let r = self
            .read
            .load(Ordering::Acquire)
            .max(self.discard.load(Ordering::Acquire));
        w.saturating_sub(r) as usize
    }

    /// 生产者：写入放得下的整帧，返回写入的样本数。
    fn push(&self, data: &[f32], channels: usize) -> usize {
        let Some(slots) = self.slots.get() else {
            return 0;
        };
        let cap = slots.len() as u64;
        let free = (cap as usize).saturating_sub(self.len());
        let n = data.len().min(free);
        let n = n - n % channels.max(1);
        let w = self.write.load(Ordering::Relaxed);
        for (i, &sample) in data[..n].iter().enumerate() {
            slots[((w + i as u64) % cap) as usize].store(sample.to_bits(), Ordering::Relaxed);
        }
        self.write.store(w + n as u64, Ordering::Release);
        n
    }

    /// 生产者：作废还没播放的样本。
    fn clear(&self) {
        self.discard
            .store(self.write.load(Ordering::Relaxed), Ordering::Release);
    }

    /// 消费者：按顺序取最多 `max` 个样本交给 `sink(序号, 样本)`，返回取出的数量。
    fn pop(&self, max: usize, mut sink: impl FnMut(usize, f32)) -> usize {
        let Some(slots) = self.slots.get() else {
            return 0;
        };
        let cap = slots.len() as u64;
        let w = self.write.load(Ordering::Acquire);
        let r = self
            .read
            .load(Ordering::Relaxed)
            .max(self.discard.load(Ordering::Acquire));
        let n = (w.saturating_sub(r) as usize).min(max);
        for i in 0..n {
            sink(
                i,
                f32::from_bits(slots[((r + i as u64) % cap) as usize].load(Ordering::Relaxed)),
            );
        }
        self.read.store(r + n as u64, Ordering::Release);
        n
    }
}

impl Default for Ring {
    fn default() -> Ring {
        Ring::new()
    }
}

/// 缓冲填充目标（样本数）。
fn ring_target(dev_rate: u32, dev_channels: usize) -> usize {
    dev_rate as usize * RING_TARGET_SECS * dev_channels
}

/// 队列的循环模式（Zplayer 的 LOOP_MODE：list / single / random）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoopMode {
    /// 顺序播放整个队列，到末尾回到第一首。
    #[default]
    List,
    /// 单曲循环：自然播完或按「下一首」都重播同一首。
    Single,
    /// 随机：队列洗牌后的顺序循环（界面显示的队列即洗牌后顺序）。
    Random,
}

impl LoopMode {
    /// 下一个模式（点循环按钮的顺序：顺序 → 单曲 → 随机）。
    pub fn cycle(self) -> LoopMode {
        match self {
            LoopMode::List => LoopMode::Single,
            LoopMode::Single => LoopMode::Random,
            LoopMode::Random => LoopMode::List,
        }
    }

    fn from_bits(value: u8) -> LoopMode {
        match value {
            1 => LoopMode::Single,
            2 => LoopMode::Random,
            _ => LoopMode::List,
        }
    }
}

/// ReplayGain 模式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplayGainMode {
    /// 不做音量标准化。
    #[default]
    Off,
    /// 按曲目增益。
    Track,
    /// 按专辑增益。
    Album,
}

/// ReplayGain 设置（界面 → 引擎，持久化在 `library.json`）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplayGainConfig {
    pub mode: ReplayGainMode,
    /// 预放大（dB，−15…+15）。
    pub preamp_db: f32,
    /// 没有 RG 标签的曲目用的增益（dB，−15…+15）。
    pub fallback_db: f32,
    /// 按标签峰值限制增益，防止削波。
    pub clip_protect: bool,
}

impl Default for ReplayGainConfig {
    fn default() -> ReplayGainConfig {
        ReplayGainConfig {
            mode: ReplayGainMode::Off,
            preamp_db: 0.0,
            fallback_db: 0.0,
            clip_protect: true,
        }
    }
}

/// 一首曲目应乘的线性增益。
///
/// 回退：专辑模式没有专辑增益 → 曲目增益；曲目模式没有曲目增益 →
/// 专辑增益；都没有 → `fallback_db`（peak 视为无）。`clip_protect` 且
/// 有峰值时，增益不超过 `1 / peak`；结果钳到 `0.0..=4.0`（+12 dB）。
pub(crate) fn replaygain_factor(cfg: &ReplayGainConfig, rg: &crate::metadata::ReplayGain) -> f32 {
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
    pub eq_config: Mutex<crate::engine::eq::EqConfig>,
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
    tx: std::sync::mpsc::Sender<EngineCmd>,
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
            eq_config: Mutex::new(crate::engine::eq::EqConfig::default()),
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

// --- 引擎线程 ----------------------------------------------------------------

struct Loaded {
    source: Source,
    resampler: Option<ResamplerState>,
    carry: Vec<f32>, // 通道转换后的输入暂存（交错、dev 声道数）
    out: Vec<f32>,   // 重采样产出暂存
    /// 缓冲放不下的已解码样本（下一轮先送这些）。
    backlog: Vec<f32>,
    src_channels: usize,
    /// 解码到了文件末尾且没预取到下一首：等缓冲里的尾巴播完再换。
    drain: Option<Drain>,
    /// 封面还没生成（缓冲先填起来再做，免得开头卡住）。
    cover_pending: bool,
    /// 本曲目的 ReplayGain 标签（设置变更时重算 `Shared::rg_factor`）。
    rg: crate::metadata::ReplayGain,
    /// 本次装载已记过播放统计（每次装载最多记一次）。
    counted: bool,
    /// 本曲已解完、正在推 `next` 的样本（换曲边界还没播到）。
    eof: bool,
    /// 换曲边界在 `played` 计数里的下标：本曲最后一样本的输出位置。
    /// 播到它时 `next` 转正（无缝换曲——尾巴还没播完下一首的样本
    /// 就已经排进缓冲了）。
    boundary: Option<u64>,
    /// 已预取的下一首：本曲解码到尾时立刻打开，样本直接接着推进
    /// 同一环形缓冲——曲间不再有排空再装载的空档。
    next: Option<NextTrack>,
}

/// 预取的下一首：源、重采样、RG 标签与要发布的曲目快照都先备好，
/// 边界一到只需换指针。
struct NextTrack {
    source: Source,
    resampler: Option<ResamplerState>,
    src_channels: usize,
    rg: crate::metadata::ReplayGain,
    public: TrackPublic,
}

/// 文件解码完、等缓冲播空的状态（`len` 最近的缓冲长度，`since` 它最后
/// 一次变短的时刻，用来发现输出停摆）。
struct Drain {
    len: usize,
    since: Instant,
}

impl Loaded {
    /// 作废暂存（seek / 停止后）：预取的下一首一并丢掉——位置变了
    /// 边界就不算了，下一首要按新位置重新预取。
    fn reset_buffers(&mut self) {
        self.carry.clear();
        self.backlog.clear();
        self.drain = None;
        self.next = None;
        self.eof = false;
        self.boundary = None;
        if let Some(r) = self.resampler.as_mut() {
            // 清空延迟线与暂存，跳转点之前的样本不再输出。
            r.reset();
        }
    }
}

/// 流式重采样：解码包大小不定，攒够重采样器一块的输入才处理，余下留到
/// 下一包。（以前每包的尾巴补零送入，等于每秒插入几十段静音：断续、爆音。）
pub(crate) struct ResamplerState {
    fft: rubato::Fft<f32>,
    channels: usize,
    pending: Vec<f32>,
    output_scratch: Vec<f32>,
}

impl ResamplerState {
    pub(crate) fn new(from: u32, to: u32, channels: usize) -> Result<Self, String> {
        let fft = rubato::Fft::<f32>::new(
            from as usize,
            to as usize,
            1024,
            channels,
            rubato::FixedSync::Input,
        )
        .map_err(|e| e.to_string())?;
        Ok(ResamplerState {
            fft,
            channels,
            pending: Vec::new(),
            output_scratch: Vec::new(),
        })
    }

    /// 清空延迟线与暂存（seek 后）。
    fn reset(&mut self) {
        self.fft.reset();
        self.pending.clear();
    }

    /// 送入交错样本，把产出追加到 `out`。
    pub(crate) fn push(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        let ch = self.channels;
        self.pending.extend_from_slice(input);
        let mut start = 0;
        loop {
            let needed = self.fft.input_frames_next().max(1);
            if self.pending.len() - start < needed * ch {
                break;
            }
            let out_max = self.fft.output_frames_max();
            self.output_scratch.resize(out_max * ch, 0.0);
            let chunk = &self.pending[start..start + needed * ch];
            let input =
                rubato::audioadapter_buffers::direct::InterleavedSlice::new(chunk, ch, needed)
                    .map_err(|e| format!("重采样输入错误: {e}"))?;
            let mut output = rubato::audioadapter_buffers::direct::InterleavedSlice::new_mut(
                &mut self.output_scratch,
                ch,
                out_max,
            )
            .map_err(|e| format!("重采样输出错误: {e}"))?;
            let (_, written) = self
                .fft
                .process_into_buffer(&input, &mut output, None)
                .map_err(|e| format!("重采样错误: {e}"))?;
            out.extend_from_slice(&self.output_scratch[..written * ch]);
            start += needed * ch;
        }
        self.pending.drain(..start);
        Ok(())
    }
}

/// 当前输出：流 + 设备格式。`stream` 为 `None` 时没有可用设备。
struct Output {
    stream: Option<cpal::Stream>,
    rate: u32,
    channels: usize,
    /// 流是否在跑（`play()`/`pause()` 的成功态）。停着时暂停流：省空闲
    /// CPU，也让 Windows 音频会话如实呈非活动态。
    running: bool,
}

/// 在系统默认设备上打开输出流（失败时 `stream: None`，格式沿用 `fallback`）。
fn open_output(
    shared: &Arc<Shared>,
    tx: &std::sync::mpsc::Sender<EngineCmd>,
    fallback: (u32, usize),
) -> Output {
    let mut rate = fallback.0;
    let mut channels = fallback.1;
    let mut stream = None;
    let mut running = false;
    match cpal::default_host().default_output_device() {
        Some(device) => match device.default_output_config() {
            Ok(config) => {
                rate = config.sample_rate();
                channels = config.channels() as usize;
                let mut stream_config = config.config();
                stream_config.buffer_size = desired_buffer_size(config.buffer_size(), rate);
                let build = build_stream(&device, stream_config, shared, tx, rate, channels)
                    .or_else(|first| {
                        // 设备不接受指定缓冲：退回系统默认（能响，只是调度余量小）。
                        eprintln!("zeo-player cpal: {first}（退回默认缓冲）");
                        build_stream(&device, config.config(), shared, tx, rate, channels)
                    });
                match build {
                    Ok(s) => {
                        running = s.play().is_ok();
                        stream = Some(s);
                    }
                    Err(err) => set_error(shared, &format!("音频输出打开失败: {err}")),
                }
            }
            Err(err) => set_error(shared, &format!("音频设备读取失败: {err}")),
        },
        None => set_error(shared, "没有可用的音频输出设备"),
    }
    Output {
        stream,
        rate,
        channels,
        running,
    }
}

/// 想要的设备缓冲（`StreamConfig::buffer_size`）：目标是 [`OUTPUT_BUFFER_MS`]
/// 毫秒的设备缓冲——默认缓冲在不少设备上只有 ~10ms，CPU 忙时回调线程稍一
/// 排队就欠载爆音。声明了支持范围的钳进范围；范围未知（WASAPI 软栈的常态）
/// 也按固定值要——`open_output` 在建流失败时退回 `Default`。
fn desired_buffer_size(range: &cpal::SupportedBufferSize, rate: u32) -> cpal::BufferSize {
    let want = (rate * OUTPUT_BUFFER_MS / 1000).max(1);
    match range {
        // `clamp` 在 min > max 时 panic：坏驱动报了颠倒范围也别死在引擎线程。
        cpal::SupportedBufferSize::Range { min, max } => {
            cpal::BufferSize::Fixed(want.clamp(*min, (*max).max(*min)))
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Fixed(want),
    }
}

/// 建一条输出流（`open_output` 两次尝试共用：先指定缓冲，失败退默认）。
fn build_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    shared: &Arc<Shared>,
    tx: &std::sync::mpsc::Sender<EngineCmd>,
    rate: u32,
    channels: usize,
) -> Result<cpal::Stream, cpal::Error> {
    device.build_output_stream(
        config,
        {
            let shared = shared.clone();
            let mut eq = crate::engine::eq::Equalizer::new();
            // 代次哨兵：强制首块读取配置（真代次不可能到 MAX）。
            let mut eq_seen = u64::MAX;
            // 上一块结尾欠载：下一块的头端要软化（硬零沿爆音）。
            let mut starved = false;
            move |data: &mut [f32], _| {
                let g = shared.eq_gen.load(Ordering::Acquire);
                if g != eq_seen
                    && let Ok(cfg) = shared.eq_config.try_lock()
                {
                    eq.configure(&cfg, rate);
                    eq_seen = g;
                }
                output_callback(&shared, &mut eq, &mut starved, data, channels)
            }
        },
        {
            let tx = tx.clone();
            move |err| {
                eprintln!("zeo-player cpal: {err}");
                let event = match err.kind() {
                    cpal::ErrorKind::DeviceChanged => DeviceEvent::Rerouted,
                    cpal::ErrorKind::StreamInvalidated => DeviceEvent::Invalidated,
                    cpal::ErrorKind::DeviceNotAvailable => DeviceEvent::Lost,
                    _ => return,
                };
                let _ = tx.send(EngineCmd::Device(event));
            }
        },
        None,
    )
}

/// 让输出流的运行状态跟随播放状态：停着时暂停流（省空闲 CPU，也让
/// Windows 音频会话如实呈非活动态——最初为 issue #11 加的，实机证明
/// 与症状无关，但行为本身无害保留）。淡出已在命令处理里等完。
fn sync_stream(out: &mut Output, playing: bool) {
    if playing == out.running {
        return;
    }
    let Some(stream) = out.stream.as_ref() else {
        return;
    };
    let ok = if playing {
        stream.play().map_err(|e| e.to_string())
    } else {
        stream.pause().map_err(|e| e.to_string())
    };
    match ok {
        Ok(()) => out.running = playing,
        Err(err) => eprintln!("zeo-player cpal: {err}"),
    }
}

/// 填充目标：设备格式算出的目标，但不超过容量的 2/3（换到更高采样率 /
/// 更多声道的设备后容量不变，只是缓冲的秒数变少）。
fn target_for(out: &Output, capacity: usize) -> usize {
    ring_target(out.rate, out.channels)
        .min(capacity * 2 / 3)
        .max(1)
}

/// 设备事件之后要不要重建输出流、要不要暂停（纯决策，供测试）。
fn device_plan(ev: DeviceEvent, playing: bool, pause_pref: bool) -> (bool, bool) {
    let rebuild = matches!(ev, DeviceEvent::Invalidated | DeviceEvent::Lost);
    // 设备丢了一定暂停；只是换设备时按设置决定。
    let pause = playing && (pause_pref || ev == DeviceEvent::Lost);
    (rebuild, pause)
}

/// 重建成功后恢复：设备格式推进共享状态；有曲目时按新格式重建重采样器、
/// seek 回原位置并重置缓冲基线（`current_time` 要先按旧 `dev_rate` 读）。
fn restore_after_rebuild(shared: &Arc<Shared>, out: &Output, loaded: &mut Option<Loaded>) {
    let at = shared.current_time();
    shared.dev_rate.store(out.rate, Ordering::Relaxed);
    if let Some(l) = loaded.as_mut() {
        l.resampler = if l.source.sample_rate != out.rate {
            match ResamplerState::new(l.source.sample_rate, out.rate, out.channels) {
                Ok(r) => Some(r),
                Err(err) => {
                    set_error(shared, &format!("重采样器创建失败: {err}"));
                    None
                }
            }
        } else {
            None
        };
        l.reset_buffers();
        let actual = l.source.seek(at).unwrap_or(at);
        reset_ring(shared, actual, out.rate);
    } else {
        reset_ring(shared, at, out.rate);
    }
}

/// 处理设备事件：按决策暂停、需要时重建输出流（先 drop 旧流再开新的）。
/// 返回 `Some` 表示设备仍不可用、记下下一次重开时刻。
fn handle_device(
    ev: DeviceEvent,
    shared: &Arc<Shared>,
    media: &MediaSession,
    tx: &std::sync::mpsc::Sender<EngineCmd>,
    out: &mut Output,
    loaded: &mut Option<Loaded>,
) -> Option<Instant> {
    let (rebuild, pause) = device_plan(
        ev,
        shared.playing.load(Ordering::Relaxed),
        shared.pause_on_device_change.load(Ordering::Relaxed),
    );
    if pause {
        shared.playing.store(false, Ordering::Relaxed);
        shared.gain.store(0f32.to_bits(), Ordering::Relaxed);
        publish_media(shared, media, true);
    }
    if !rebuild {
        return None;
    }
    drop(out.stream.take());
    let fallback = (out.rate, out.channels);
    *out = open_output(shared, tx, fallback);
    if out.stream.is_some() {
        // 重建出的流是 running 的；停着时立刻暂停，保持会话非活动。
        sync_stream(out, shared.playing.load(Ordering::Relaxed));
        restore_after_rebuild(shared, out, loaded);
        None
    } else {
        Some(Instant::now() + Duration::from_secs(1))
    }
}

fn run(
    rx: std::sync::mpsc::Receiver<EngineCmd>,
    tx: std::sync::mpsc::Sender<EngineCmd>,
    shared: Arc<Shared>,
    media: MediaSession,
) {
    let mut out = open_output(&shared, &tx, (48_000, 2));
    shared.dev_rate.store(out.rate, Ordering::Relaxed);
    // 容量只能分配一次：按 max(本次需要, 默认立体声 48k 的 3 秒) 给——之后
    // 换到更高规格的设备容量不变，只是缓冲的秒数变少（target_for 钳住）。
    shared.ring.init(
        (out.rate as usize * RING_CAPACITY_SECS * out.channels)
            .max(48_000 * RING_CAPACITY_SECS * 2),
    );
    let mut target = target_for(&out, shared.ring.capacity());
    raise_thread_priority();

    let mut loaded: Option<Loaded> = None;
    // 原始顺序的队列（`Load` 传入）。`track.queue` 是按循环模式重排后
    // 的播放顺序，切模式时从这里重建。
    let mut base_queue: Arc<Vec<String>> = Arc::new(Vec::new());
    let mut media_tick = Instant::now();
    // 上一轮等待时收到的命令（等待用 recv_timeout，有命令立即醒）。
    let mut waiting: Option<EngineCmd> = None;
    // 设备断开后的重开时刻（`Some` 期间每秒试一次）。
    let mut retry_at: Option<Instant> = None;

    loop {
        // 0. 设备重试：设备回来了就重建输出、按原位置恢复。
        if let Some(at) = retry_at
            && Instant::now() >= at
        {
            let fallback = (out.rate, out.channels);
            out = open_output(&shared, &tx, fallback);
            retry_at = if out.stream.is_some() {
                sync_stream(&mut out, shared.playing.load(Ordering::Relaxed));
                restore_after_rebuild(&shared, &out, &mut loaded);
                None
            } else {
                Some(Instant::now() + Duration::from_secs(1))
            };
            target = target_for(&out, shared.ring.capacity());
        }
        // 1. 命令
        while let Some(cmd) = waiting.take().or_else(|| rx.try_recv().ok()) {
            // 会打断声音的操作：先淡出到静音，操作完再淡入（暂停 / 停止则停在静音）。
            let playing = shared.playing.load(Ordering::Relaxed);
            let interrupts = match &cmd {
                EngineCmd::Load { .. }
                | EngineCmd::Cue { .. }
                | EngineCmd::Seek(_)
                | EngineCmd::SeekBy(_)
                | EngineCmd::Next
                | EngineCmd::Previous
                | EngineCmd::Jump(_)
                | EngineCmd::Stop
                | EngineCmd::Pause => true,
                EngineCmd::Toggle => playing,
                _ => false,
            };
            if interrupts {
                fade_out(&shared, &mut loaded, target, out.channels);
            }
            match cmd {
                EngineCmd::Device(ev) => {
                    retry_at = handle_device(ev, &shared, &media, &tx, &mut out, &mut loaded);
                    target = target_for(&out, shared.ring.capacity());
                }
                cmd => handle_cmd(
                    &shared,
                    &media,
                    &mut loaded,
                    &mut base_queue,
                    cmd,
                    out.rate,
                    out.channels,
                ),
            }
            if !shared.playing.load(Ordering::Relaxed) {
                // 停着：下次开始从静音淡入。
                shared.gain.store(0f32.to_bits(), Ordering::Relaxed);
            }
            shared.fade_target.store(1f32.to_bits(), Ordering::Relaxed);
        }
        // 播放状态本轮可能变了：让流跟上（停着暂停流，开始播放则先启动
        // 流——回调恢复取样后 gain 才从 0 淡入）。
        sync_stream(&mut out, shared.playing.load(Ordering::Relaxed));
        // 2. 解码填充
        let playing = shared.playing.load(Ordering::Relaxed);
        let mut track_done = false;
        if let Some(l) = loaded.as_mut() {
            if playing && l.drain.is_none() {
                let mut result = Ok(false);
                for _ in 0..MAX_PACKETS_PER_TICK {
                    if shared.ring.len() >= target {
                        break;
                    }
                    result = fill_ring(l, &shared, out.channels);
                    match result {
                        Ok(false) => continue,
                        Err(_) => break,
                        Ok(true) => {
                            if l.eof {
                                // 预取源也到尾了（曲目极短）：边界转正
                                // 后下一轮再预取下下一首。
                                l.drain = Some(Drain {
                                    len: shared.ring.len(),
                                    since: Instant::now(),
                                });
                            } else if let Some(next) =
                                prefetch_next(&shared, out.rate, out.channels)
                            {
                                // 本曲到尾：下一首已备好，边界记下后接着
                                // 推进同一缓冲——尾巴播完自然换曲。
                                let left = (shared.ring.len() / out.channels.max(1)) as u64;
                                l.boundary = Some(shared.played.load(Ordering::Relaxed) + left);
                                l.next = Some(next);
                                l.eof = true;
                                continue;
                            } else {
                                // 没有下一首 / 打不开：退回排空再换的老路。
                                l.drain = Some(Drain {
                                    len: shared.ring.len(),
                                    since: Instant::now(),
                                });
                            }
                            break;
                        }
                    }
                }
                if let Err(err) = result {
                    set_error(&shared, &err);
                    shared.playing.store(false, Ordering::Relaxed);
                }
            }
            // 无缝换曲边界：本曲尾巴播到，`next` 转正（输出停摆时立即
            // 转正——反正也播不出位置偏差）。
            if l.boundary.is_some()
                && (out.stream.is_none()
                    || shared.played.load(Ordering::Relaxed) >= l.boundary.unwrap_or(0))
            {
                complete_switch(l, &shared, &media);
            }
            // 封面等缓冲有了一半再生成（第一次要读标签、缩放、模糊，CPU 忙时
            // 可能要几百毫秒）。
            if l.cover_pending && (!playing || l.drain.is_some() || shared.ring.len() >= target / 2)
            {
                l.cover_pending = false;
                load_covers(&shared, &media);
            }
            // 文件解完了：等缓冲里的尾巴播完再换下一首（马上换会把最后一段
            // 截掉）。没有输出设备或输出停摆超过 1 秒时不再等。
            if let Some(drain) = l.drain.as_mut() {
                let len = shared.ring.len();
                if !playing || len < drain.len {
                    drain.len = len;
                    drain.since = Instant::now();
                }
                track_done = playing
                    && (len == 0
                        || out.stream.is_none()
                        || drain.since.elapsed() > Duration::from_secs(1));
            }
            // 播放统计：越过阈值的那一刻记一次（不等播完——中途退出也不丢
            // 已记的这次）。`listened` 只由输出回调推进的实际播出帧数
            // 累加，拖动进度条不算听过。
            if !l.counted {
                let t = shared.track.lock().unwrap().clone();
                let rate = shared.dev_rate.load(Ordering::Relaxed).max(1) as f64;
                let listened = shared.listened.load(Ordering::Relaxed) as f64 / rate;
                if counts_as_play(
                    listened,
                    t.duration,
                    shared.stats_min_percent.load(Ordering::Relaxed),
                    shared.stats_min_secs.load(Ordering::Relaxed),
                ) {
                    l.counted = true;
                    if let Some(path) = t.path.clone() {
                        let at = crate::config::unix_now();
                        crate::db::later(move || crate::db::record_play(&path, at, listened));
                        shared.plays_gen.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        if track_done {
            let (queue, index) = {
                let t = shared.track.lock().unwrap();
                (t.queue.clone(), t.queue_index)
            };
            let next = advance(index, queue.len(), shared.loop_mode(), true);
            if let Some((next_index, next_path)) =
                next.and_then(|i| queue.get(i as usize).cloned().map(|p| (i, p)))
            {
                load_track(
                    &shared,
                    &media,
                    &mut loaded,
                    PathBuf::from(next_path),
                    queue,
                    next_index,
                    out.rate,
                    out.channels,
                );
                shared.playing.store(true, Ordering::Relaxed);
            } else {
                shared.playing.store(false, Ordering::Relaxed);
                loaded = None;
                shared.ready.store(false, Ordering::Relaxed);
                publish_media(&shared, &media, true);
                sync_stream(&mut out, false);
            }
            continue;
        }
        // 3. 媒体会话位置心跳（仅在播放中，约 1s）
        if media_tick.elapsed() >= Duration::from_secs(1) {
            media_tick = Instant::now();
            if shared.playing.load(Ordering::Relaxed) {
                publish_media(&shared, &media, false);
            }
        }
        // 4. 等下一轮：缓冲满了就睡，有命令立即醒。缓冲没满（刚 seek、
        //    包数上限截断）时不睡，接着解码。
        let behind = playing
            && loaded.as_ref().is_some_and(|l| l.drain.is_none())
            && shared.ring.len() < target / 2;
        let wait = if behind {
            Duration::ZERO
        } else if playing {
            PLAYING_TICK
        } else {
            IDLE_TICK
        };
        match rx.recv_timeout(wait) {
            Ok(cmd) => waiting = Some(cmd),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            // 句柄都没了（进程退出中）：不再有命令，照常按节奏解码。
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(wait),
        }
    }
}

/// 提高引擎线程的调度优先级：CPU 忙时解码线程也能按时把缓冲填上。
/// （输出回调所在的线程由 cpal / 系统设成实时优先级——WASAPI 上靠 cpal
/// 的 `realtime` feature 上 MMCSS「Pro Audio」，macOS 由 HAL 的 IO 线程
/// 自带实时调度；这里是喂它的线程。）
fn raise_thread_priority() {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
        }
        const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
        // SAFETY: 只修改调用线程自己的 QoS 类别，参数为系统定义的常量。
        unsafe {
            pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
        }
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
        };
        // SAFETY: GetCurrentThread 返回当前线程的伪句柄，无需关闭。
        unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        }
    }
}

fn handle_cmd(
    shared: &Arc<Shared>,
    media: &MediaSession,
    loaded: &mut Option<Loaded>,
    base_queue: &mut Arc<Vec<String>>,
    cmd: EngineCmd,
    dev_rate: u32,
    dev_channels: usize,
) {
    match cmd {
        EngineCmd::Load { path, queue } => {
            *base_queue = Arc::new(queue);
            let queue = ordered_queue(base_queue, shared.loop_mode());
            let index = queue
                .iter()
                .position(|p| p.as_str() == path.to_string_lossy().as_ref())
                .map(|i| i as i64)
                .unwrap_or(0);
            load_track(
                shared,
                media,
                loaded,
                path,
                queue,
                index,
                dev_rate,
                dev_channels,
            );
            shared.playing.store(true, Ordering::Relaxed);
            publish_media(shared, media, true);
        }
        EngineCmd::Cue { path, queue, time } => {
            *base_queue = Arc::new(queue);
            let queue = ordered_queue(base_queue, shared.loop_mode());
            let index = queue
                .iter()
                .position(|p| p.as_str() == path.to_string_lossy().as_ref())
                .map(|i| i as i64)
                .unwrap_or(0);
            shared.playing.store(false, Ordering::Relaxed);
            load_track(
                shared,
                media,
                loaded,
                path,
                queue,
                index,
                dev_rate,
                dev_channels,
            );
            if time > 0.0
                && let Some(l) = loaded.as_mut()
                && let Ok(actual) = l.source.seek(time)
            {
                reset_ring(shared, actual, dev_rate);
                l.reset_buffers();
            }
            publish_media(shared, media, true);
        }
        EngineCmd::Play => {
            if loaded.is_some() {
                shared.playing.store(true, Ordering::Relaxed);
                publish_media(shared, media, true);
            }
        }
        EngineCmd::Pause => {
            shared.playing.store(false, Ordering::Relaxed);
            publish_media(shared, media, true);
        }
        EngineCmd::Toggle => {
            let now = !shared.playing.load(Ordering::Relaxed);
            if now && loaded.is_some() {
                shared.playing.store(now, Ordering::Relaxed);
            } else {
                shared.playing.store(false, Ordering::Relaxed);
            }
            publish_media(shared, media, true);
        }
        EngineCmd::Stop => {
            shared.playing.store(false, Ordering::Relaxed);
            if let Some(l) = loaded.as_mut() {
                let _ = l.source.seek(0.0);
                l.reset_buffers();
            }
            reset_ring(shared, 0.0, dev_rate);
            publish_media(shared, media, true);
        }
        EngineCmd::Seek(seconds) => {
            if let Some(l) = loaded.as_mut() {
                match l.source.seek(seconds.max(0.0)) {
                    Ok(actual) => {
                        reset_ring(shared, actual, dev_rate);
                        l.reset_buffers();
                        publish_media(shared, media, true);
                    }
                    Err(err) => set_error(shared, &err),
                }
            }
        }
        EngineCmd::SeekBy(delta) => {
            let target = shared.current_time() + delta;
            handle_cmd(
                shared,
                media,
                loaded,
                base_queue,
                EngineCmd::Seek(target),
                dev_rate,
                dev_channels,
            );
        }
        EngineCmd::SetVolume(volume) => {
            let clamped = volume.clamp(0.0, 1.0) as f32;
            shared.volume.store(clamped.to_bits(), Ordering::Relaxed);
            publish_media(shared, media, false);
        }
        EngineCmd::Jump(target) => {
            let queue = shared.track.lock().unwrap().queue.clone();
            if let Some(path) = usize::try_from(target)
                .ok()
                .and_then(|i| queue.get(i))
                .cloned()
            {
                load_track(
                    shared,
                    media,
                    loaded,
                    PathBuf::from(path),
                    queue,
                    target,
                    dev_rate,
                    dev_channels,
                );
                shared.playing.store(true, Ordering::Relaxed);
                publish_media(shared, media, true);
            }
        }
        EngineCmd::Next | EngineCmd::Previous => {
            let (queue, index) = {
                let t = shared.track.lock().unwrap();
                (t.queue.clone(), t.queue_index)
            };
            let forward = matches!(cmd, EngineCmd::Next);
            let target = advance(index, queue.len(), shared.loop_mode(), forward);
            if let Some(target) = target
                && let Some(path) = queue.get(target as usize).cloned()
            {
                load_track(
                    shared,
                    media,
                    loaded,
                    PathBuf::from(path),
                    queue,
                    target,
                    dev_rate,
                    dev_channels,
                );
                shared.playing.store(true, Ordering::Relaxed);
                publish_media(shared, media, true);
            }
        }
        EngineCmd::SetLoopMode(mode) => {
            shared.loop_mode.store(mode as u8, Ordering::Relaxed);
            // 队列按新模式重排：顺序 / 单曲是原序，随机洗牌；当前曲目
            // 跟着挪到它在新顺序里的位置。
            let queue = ordered_queue(base_queue, mode);
            let mut slot = shared.track.lock().unwrap();
            let mut t = (**slot).clone();
            if let Some(path) = &t.path {
                t.queue_index = queue
                    .iter()
                    .position(|p| p == path)
                    .map(|i| i as i64)
                    .unwrap_or(t.queue_index.max(0));
            }
            t.queue = queue;
            *slot = Arc::new(t);
            // 顺序变了：预取的「下一首」可能已不是它，丢掉重预取。
            if let Some(l) = loaded.as_mut() {
                l.next = None;
                l.eof = false;
                l.boundary = None;
            }
        }
        EngineCmd::QueueAdd(paths) => {
            // 追加进原始队列后按当前循环模式重算顺序；当前曲目挪到它在
            // 新顺序里的位置（随机模式下整队顺序会变）。
            let mut base = (**base_queue).clone();
            base.extend(paths);
            *base_queue = Arc::new(base);
            let queue = ordered_queue(base_queue, shared.loop_mode());
            let mut slot = shared.track.lock().unwrap();
            let mut t = (**slot).clone();
            if let Some(path) = &t.path {
                t.queue_index = queue
                    .iter()
                    .position(|p| p == path)
                    .map(|i| i as i64)
                    .unwrap_or(t.queue_index.max(0));
            }
            t.queue = queue;
            *slot = Arc::new(t);
            // 队列变了：预取的「下一首」可能已不是它，丢掉重预取。
            if let Some(l) = loaded.as_mut() {
                l.next = None;
                l.eof = false;
                l.boundary = None;
            }
        }
        // 设备事件在主循环里先拦截（见 run），不会走到这里。
        EngineCmd::Device(_) => {}
        EngineCmd::SetReplayGain(cfg) => {
            *shared.rg_config.lock().unwrap_or_else(|e| e.into_inner()) = cfg;
            // 当前曲目立即生效（下一首装载时也会按新配置算）。
            if let Some(l) = loaded.as_ref() {
                shared
                    .rg_factor
                    .store(replaygain_factor(&cfg, &l.rg).to_bits(), Ordering::Relaxed);
            }
        }
        EngineCmd::Release { done } => {
            // 暂停输出、丢掉解码器（`Source` 的 drop 关文件）——写回
            // 标签的原子替换要文件没被打开（Windows 强制）。`track`
            // 快照不动，界面仍显示当前曲目。
            shared.playing.store(false, Ordering::Relaxed);
            *loaded = None;
            shared.ready.store(false, Ordering::Relaxed);
            shared.ring.clear();
            publish_media(shared, media, true);
            let _ = done.send(());
        }
        EngineCmd::Reload { position, playing } => {
            let Some((path, queue, index)) = ({
                let t = shared.track.lock().unwrap();
                t.path
                    .clone()
                    .map(|p| (PathBuf::from(p), t.queue.clone(), t.queue_index))
            }) else {
                return;
            };
            load_track(
                shared,
                media,
                loaded,
                path,
                queue,
                index,
                dev_rate,
                dev_channels,
            );
            if let Some(l) = loaded.as_mut()
                && position > 0.0
                && let Ok(actual) = l.source.seek(position)
            {
                reset_ring(shared, actual, dev_rate);
                l.reset_buffers();
            }
            // 装载失败（文件写坏了之类）时保持停止，报出错误。
            shared
                .playing
                .store(playing && loaded.is_some(), Ordering::Relaxed);
            publish_media(shared, media, true);
        }
    }
}

/// 按循环模式生成播放顺序：随机洗牌 `base` 的副本（原序保持不动），
/// 其余模式就是 `base` 本身（共享同一个 `Arc`）。
fn ordered_queue(base: &Arc<Vec<String>>, mode: LoopMode) -> Arc<Vec<String>> {
    match mode {
        LoopMode::Random => Arc::new(shuffled(base)),
        _ => base.clone(),
    }
}

/// 随机队列（参考 audio0 的 `createSmartShuffle`）：同一歌手的曲目按
/// 黄金比例近似均匀摊开——连续撞上一个歌手的概率大降；组内纯随机
/// （权重先按 audio0 的默认固定为 3，不按播放次数加权）。歌手从曲库
/// 索引一次取好（不在索引里的归空组，不读文件标签）。
fn shuffled(base: &[String]) -> Vec<String> {
    let tracks = crate::config::library_store()
        .tracks
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let artist_of: std::collections::HashMap<&str, &str> = tracks
        .iter()
        .map(|t| {
            let a = if t.artist.is_empty() {
                t.album_artist.as_str()
            } else {
                t.artist.as_str()
            };
            (t.path.as_str(), a)
        })
        .collect();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let order = spread_order(
        base.len(),
        |i| artist_of.get(base[i].as_str()).copied().unwrap_or(""),
        |_| 3.0,
        seed,
    );
    order.into_iter().map(|i| base[i].clone()).collect()
}

/// 黄金比例（audio0 smart shuffle 同款常数）。
const GOLDEN: f64 = 0.618033988749895;

fn frac(x: f64) -> f64 {
    x - x.floor()
}

/// audio0 的字符串散列：`h = h*31 + c` 按 int32 溢出滚动，归一到 0..1。
fn string_hash(s: &str) -> f64 {
    let mut h: i32 = 0;
    for c in s.chars() {
        h = h.wrapping_mul(31).wrapping_add(c as i32);
    }
    (h as i64).unsigned_abs() as f64 / 2147483648.0
}

/// 把 `0..n` 重排成「同组摊开」的顺序：每组（歌手）按
/// `散列(组名)×黄金比例` 定环上偏移，组内第 j 个落在
/// `偏移 + j/组大小 + 抖动`（模 1），全体按落点排序即得队列——同一
/// 组自然近似均匀摊开（audio0 `createSmartShuffle`）。组内顺序按
/// `rand × (score+1)` 加权（audio0 `weightedShuffle`，+1 让零播放
/// 曲目也有机会）。同 `seed` 同输入必同输出（组键先排序再遍历）。
fn spread_order<'a>(
    n: usize,
    group: impl Fn(usize) -> &'a str,
    score: impl Fn(usize) -> f64,
    seed: u64,
) -> Vec<usize> {
    let mut groups: std::collections::HashMap<&'a str, Vec<usize>> =
        std::collections::HashMap::new();
    for i in 0..n {
        groups.entry(group(i)).or_default().push(i);
    }
    let mut keys: Vec<&&str> = groups.keys().collect();
    keys.sort();
    let mut rng = seed | 0x9e37_79b9_7f4a_7c15;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 11) as f64 / ((1u64 << 53) as f64)
    };
    let global_offset = frac((seed as f64).sin().abs() * 10000.0);
    let mut positions = vec![0.0_f64; n];
    for key in keys {
        let items = &groups[key];
        let len = items.len();
        let density = len as f64 / n.max(1) as f64;
        let base = frac(frac(string_hash(key) + global_offset) * GOLDEN) % (1.0 - density + 0.001);
        // 组内加权序：`rand × (score+1)` 降序，得分高的靠前占位。
        let mut order: Vec<(usize, f64)> = items
            .iter()
            .map(|&i| (i, next() * (1.0 + score(i))))
            .collect();
        order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let len_f = len as f64;
        for (j, &(i, _)) in order.iter().enumerate() {
            let jitter = next() * 0.1 / len_f;
            positions[i] = frac(base + j as f64 / len_f + jitter);
        }
    }
    let mut out: Vec<usize> = (0..n).collect();
    out.sort_by(|a, b| {
        positions[*a]
            .partial_cmp(&positions[*b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// 「听过多少算一次播放」：实际播出 `listened` 秒、整首 `duration` 秒时，
/// 百分比阈值（`min_percent`，0 = 不用）或时长阈值（`min_secs`，0 = 不用）
/// 任一满足即算一次。两个都是 0 时开始播放就算。`duration` 缺失 / 为 0
/// 时百分比条件不生效（秒数条件仍可用）。
fn counts_as_play(listened: f64, duration: f64, min_percent: u32, min_secs: u32) -> bool {
    if min_percent == 0 && min_secs == 0 {
        return true;
    }
    let by_percent =
        min_percent > 0 && duration > 0.0 && listened >= duration * f64::from(min_percent) / 100.0;
    let by_seconds = min_secs > 0 && listened >= f64::from(min_secs);
    by_percent || by_seconds
}

/// 下标 `index` 后去哪一首（`forward` 上一首 / 下一首）：单曲停在原地
/// （重播当前），其余模式前进 / 后退一步并在端点回绕。
fn advance(index: i64, len: usize, mode: LoopMode, forward: bool) -> Option<i64> {
    if len == 0 || index < 0 {
        return None;
    }
    match mode {
        LoopMode::Single => Some(index),
        _ => Some((index + if forward { 1 } else { -1 }).rem_euclid(len as i64)),
    }
}

/// 装载一个曲目（引擎线程内）：读元数据、封面、打开解码器。
// 参数都是引擎线程循环里的局部状态，打包成结构体只会多一层转手。
#[allow(clippy::too_many_arguments)]
fn load_track(
    shared: &Arc<Shared>,
    media: &MediaSession,
    loaded: &mut Option<Loaded>,
    path: PathBuf,
    queue: Arc<Vec<String>>,
    queue_index_hint: i64,
    dev_rate: u32,
    dev_channels: usize,
) {
    shared.loading.store(true, Ordering::Relaxed);
    reset_ring(shared, 0.0, dev_rate);
    // 播放统计按「本次装载」归零（单曲循环重载即新的计次机会）。
    shared.listened.store(0, Ordering::Relaxed);

    let meta = match MetaCache::global().get(&path) {
        Ok(meta) => meta,
        Err(err) => {
            set_error(shared, &err);
            shared.loading.store(false, Ordering::Relaxed);
            return;
        }
    };

    match Source::open(&path) {
        Ok(source) => {
            let src_rate = source.sample_rate;
            let src_channels = source.channels;
            let resampler = if src_rate != dev_rate {
                match ResamplerState::new(src_rate, dev_rate, dev_channels) {
                    Ok(r) => Some(r),
                    Err(err) => {
                        set_error(shared, &format!("重采样器创建失败: {err}"));
                        shared.loading.store(false, Ordering::Relaxed);
                        return;
                    }
                }
            } else {
                None
            };
            // RG 标签随扫描入缓存 / 库；迁移前的老行四项全空时现场补读。
            let rg = if meta.replaygain.is_empty() {
                crate::metadata::read_replaygain(&path)
            } else {
                meta.replaygain
            };
            let cfg = *shared.rg_config.lock().unwrap_or_else(|e| e.into_inner());
            shared
                .rg_factor
                .store(replaygain_factor(&cfg, &rg).to_bits(), Ordering::Relaxed);
            *loaded = Some(Loaded {
                source,
                resampler,
                carry: Vec::new(),
                out: Vec::new(),
                backlog: Vec::new(),
                src_channels,
                drain: None,
                cover_pending: true,
                rg,
                counted: false,
                eof: false,
                boundary: None,
                next: None,
            });

            let actual_index = if queue_index_hint >= 0 {
                queue_index_hint
            } else {
                queue
                    .iter()
                    .position(|p| p == &path.to_string_lossy())
                    .map(|i| i as i64)
                    .unwrap_or(0)
            };
            // 封面已在内存（队尾预载 / 播过的大图 URI）就直接进快照：
            // 同一帧里曲目信息与封面一起换，界面不会闪占位符；后台
            // `load_covers` 照旧跑（色调补算、媒体会话、冷缓存生成）。
            *shared.track.lock().unwrap() =
                Arc::new(track_public(&path, &meta, queue, actual_index));
            shared.ready.store(true, Ordering::Relaxed);
            // 封面由引擎循环在缓冲填起来之后生成（`load_covers`）；媒体
            // 会话的封面在 `publish` 里现查缓存，不用单独喂。
            publish_media(shared, media, true);
        }
        Err(err) => set_error(shared, &err),
    }
    shared.loading.store(false, Ordering::Relaxed);
}

/// 曲目的公开快照（装载与无缝换曲共用）：封面现查大图缓存——在内存
/// （队尾预载 / 预热 / 播过）就直接带上，没有由 cover 线程后补；
/// 大图是按需读元数据生成的 data URI，不落盘。
fn track_public(
    path: &std::path::Path,
    meta: &crate::metadata::MetaInfo,
    queue: Arc<Vec<String>>,
    queue_index: i64,
) -> TrackPublic {
    let artwork = crate::metadata::covers::cached_artwork(path);
    let title = if !meta.title.is_empty() {
        meta.title.clone()
    } else {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown".to_owned())
    };
    let artist = if !meta.artist.is_empty() {
        meta.artist.clone()
    } else {
        "Unknown Artist".to_owned()
    };
    let album = if !meta.album.is_empty() {
        meta.album.clone()
    } else {
        "Unknown Album".to_owned()
    };
    TrackPublic {
        path: Some(path.to_string_lossy().into_owned()),
        title,
        artist,
        album,
        covers_ready: artwork.is_some(),
        artwork,
        duration: meta.duration,
        queue,
        queue_index,
        error: None,
    }
}

/// 预取下一首（引擎线程，本曲尾巴还在播时调）：按循环模式定目标，
/// 读元数据、打开解码器、备好重采样器与曲目快照；封面另起线程预热，
/// 边界转正时 `cached_artwork` 多半已经命中。`None` = 没法定或打
/// 不开（走 `drain` 老路，`load_track` 会原样报错）。
fn prefetch_next(shared: &Arc<Shared>, dev_rate: u32, dev_channels: usize) -> Option<NextTrack> {
    let (queue, index) = {
        let t = shared.track.lock().unwrap();
        (t.queue.clone(), t.queue_index)
    };
    let next_index = advance(index, queue.len(), shared.loop_mode(), true)?;
    let path = PathBuf::from(queue.get(next_index as usize)?.clone());
    let meta = MetaCache::global().get(&path).ok()?;
    let source = Source::open(&path).ok()?;
    let resampler = if source.sample_rate != dev_rate {
        match ResamplerState::new(source.sample_rate, dev_rate, dev_channels) {
            Ok(r) => Some(r),
            Err(_) => return None,
        }
    } else {
        None
    };
    let rg = if meta.replaygain.is_empty() {
        crate::metadata::read_replaygain(&path)
    } else {
        meta.replaygain
    };
    warm_covers(&path);
    let src_channels = source.channels;
    Some(NextTrack {
        source,
        resampler,
        src_channels,
        rg,
        public: track_public(&path, &meta, queue, next_index),
    })
}

/// 给下一首预热封面：大图 URI 与列表封面文件都生成好（纯生成、不碰
/// 共享状态）——边界转正时快照就能带上封面，少一帧占位符。
fn warm_covers(path: &std::path::Path) {
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let _ = crate::metadata::covers::artwork_uri(&path);
        let _ = crate::metadata::covers::list_cover_pub(&path);
    });
}

/// 无缝换曲的边界到了：预取的 `next` 转正。`played` 是「本曲播出位置」
/// 的累计帧计数——原子减去边界，回调在边界后推过的帧数正好续成新曲
/// 位置；`listened` 同理归零（多播出的尾巴帧直接继承成新曲收听量）。
fn complete_switch(l: &mut Loaded, shared: &Arc<Shared>, media: &MediaSession) {
    let Some(next) = l.next.take() else {
        l.eof = false;
        l.boundary = None;
        return;
    };
    // 输出停摆时立即转正的边界可能还没播到：按当前位置钳一下再减。
    let boundary = l
        .boundary
        .take()
        .unwrap_or(0)
        .min(shared.played.load(Ordering::Relaxed));
    let prev = shared.played.fetch_sub(boundary, Ordering::Relaxed);
    shared
        .listened
        .store(prev.saturating_sub(boundary), Ordering::Relaxed);
    l.source = next.source;
    l.resampler = next.resampler;
    l.src_channels = next.src_channels;
    l.rg = next.rg;
    let cfg = *shared.rg_config.lock().unwrap_or_else(|e| e.into_inner());
    shared
        .rg_factor
        .store(replaygain_factor(&cfg, &l.rg).to_bits(), Ordering::Relaxed);
    l.eof = false;
    l.drain = None;
    l.counted = false;
    l.cover_pending = true;
    let mut public = next.public;
    // 预热多半已把大图做进缓存：转正时再查一次，少一帧占位符。
    if public.artwork.is_none()
        && let Some(path) = public.path.as_deref().map(PathBuf::from)
    {
        public.artwork = crate::metadata::covers::cached_artwork(&path);
        public.covers_ready = public.artwork.is_some();
    }
    *shared.track.lock().unwrap() = Arc::new(public);
    publish_media(shared, media, true);
}

/// 给当前曲目生成封面：720 px 的 JPEG `data:` URI（界面显示用，实时读
/// 元数据不落盘；全屏模糊背景复用它）；列表封面文件也顺手生成一份，
/// 系统媒体会话在 `publish` 里现查缓存拿它。读标签 + 解码缩放在冷缓存
/// 时要几百毫秒：挪出引擎线程（不然饿死音频回调、也排不进正在预热的
/// 缩略图池），线程随换曲自生自灭，`cover_gen` 让晚到的旧结果失效。
fn load_covers(shared: &Arc<Shared>, media: &MediaSession) {
    let Some(path) = shared.track.lock().unwrap().path.clone() else {
        return;
    };
    let gen_id = shared.cover_gen.fetch_add(1, Ordering::Relaxed) + 1;
    let shared = shared.clone();
    let media = media.clone();
    std::thread::spawn(move || {
        if shared.cover_gen.load(Ordering::Relaxed) != gen_id {
            return;
        }
        let file = std::path::Path::new(&path);
        let cover = crate::metadata::covers::artwork_uri(file);
        // 系统媒体会话的封面在 publish 里现查列表缓存：这里顺手把文件
        // 生成出来（冷缓存 / 还没显示过的曲目才缺）。
        let _ = crate::metadata::covers::list_cover_pub(file);
        {
            let mut slot = shared.track.lock().unwrap();
            if shared.cover_gen.load(Ordering::Relaxed) != gen_id
                || slot.path.as_deref() != Some(path.as_str())
            {
                return;
            }
            let mut t = (**slot).clone();
            t.artwork = cover;
            t.covers_ready = true;
            *slot = Arc::new(t);
        }
        publish_media(&shared, &media, true);
    });
}

/// 作废缓冲里还没播的样本，并把位置基线设为 `seconds`。
fn reset_ring(shared: &Arc<Shared>, seconds: f64, dev_rate: u32) {
    shared.ring.clear();
    shared.played.store(
        (seconds * dev_rate as f64).round() as u64,
        Ordering::Relaxed,
    );
}

fn set_error(shared: &Arc<Shared>, message: &str) {
    eprintln!("zeo-player: {message}");
    let mut slot = shared.track.lock().unwrap();
    // 同一条错误不换快照（不然解码失败的循环每秒都刷一遍界面）。
    if slot.error.as_deref() == Some(message) {
        return;
    }
    let mut t = (**slot).clone();
    t.error = Some(message.to_owned());
    *slot = Arc::new(t);
}

/// 解码一块并写入环形缓冲；返回 Ok(true) 表示当前数据源到尾。缓冲
/// 放不下的部分留在 `backlog`，下次先送；`backlog` 还没送完时不解码
/// 新包。`l.eof` 后数据源换成预取的 `next`（同一缓冲接着推，无缝）。
fn fill_ring(l: &mut Loaded, shared: &Arc<Shared>, dev_channels: usize) -> Result<bool, String> {
    if !l.backlog.is_empty() {
        let n = shared.ring.push(&l.backlog, dev_channels);
        l.backlog.drain(..n);
        if !l.backlog.is_empty() {
            return Ok(false);
        }
    }
    let (src, src_channels, resampler) = if l.eof {
        let next = l.next.as_mut().expect("eof 时 next 必在");
        (&mut next.source, next.src_channels, &mut next.resampler)
    } else {
        (&mut l.source, l.src_channels, &mut l.resampler)
    };
    let Some(block) = src.next_block()? else {
        return Ok(true);
    };
    if block.len() / src_channels.max(1) == 0 {
        return Ok(false);
    }
    // 1) 声道数转换 → dev_channels
    convert_channels(block, src_channels, dev_channels, &mut l.carry);
    // 2) 重采样或直通
    let samples = if let Some(r) = resampler.as_mut() {
        l.out.clear();
        r.push(&l.carry, &mut l.out)?;
        &l.out
    } else {
        &l.carry
    };
    let n = shared.ring.push(samples, dev_channels);
    l.backlog.extend_from_slice(&samples[n..]);
    Ok(false)
}

/// 交错样本声道转换（mono↔stereo、多声道降混）。
fn convert_channels(input: &[f32], from: usize, to: usize, out: &mut Vec<f32>) {
    out.clear();
    let from = from.max(1);
    let to = to.max(1);
    if from == to {
        out.extend_from_slice(input);
        return;
    }
    let frames = input.len() / from;
    out.reserve(frames * to);
    for frame in 0..frames {
        let start = frame * from;
        match (from, to) {
            (1, 2) => {
                let s = input[start];
                out.push(s);
                out.push(s);
            }
            (2, 1) => out.push((input[start] + input[start + 1]) * 0.5),
            _ => {
                // 多声道平均到单声道再复制到所有目标声道。
                let sum: f32 = input[start..start + from].iter().sum();
                let avg = sum / from as f32;
                for _ in 0..to {
                    out.push(avg);
                }
            }
        }
    }
}

/// cpal 回调：取环形缓冲、施加音量、推进播放位置。
/// 淡出到静音再返回（没在播放或关闭淡变时立即返回）。等待期间继续填充
/// 缓冲，长淡出也不会断音。
fn fade_out(shared: &Arc<Shared>, loaded: &mut Option<Loaded>, target: usize, dev_channels: usize) {
    let ms = shared.fade_ms.load(Ordering::Relaxed);
    if ms == 0 || !shared.playing.load(Ordering::Relaxed) {
        return;
    }
    shared.fade_target.store(0f32.to_bits(), Ordering::Relaxed);
    let deadline = std::time::Instant::now() + Duration::from_millis(ms as u64 + 150);
    while std::time::Instant::now() < deadline {
        if f32::from_bits(shared.gain.load(Ordering::Relaxed)) <= 0.0 {
            return;
        }
        if let Some(l) = loaded.as_mut() {
            while shared.ring.len() < target {
                if !matches!(fill_ring(l, shared, dev_channels), Ok(false)) {
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// 淡变曲线：线性增益的 smoothstep，听感比线性平滑。
fn fade_curve(gain: f32) -> f32 {
    gain * gain * (3.0 - 2.0 * gain)
}

/// 每帧增益步长（`ms` 毫秒内从 0 到 1）。
fn fade_step(ms: u32, rate: u32) -> f32 {
    if ms == 0 {
        1.0
    } else {
        1000.0 / (ms as f32 * rate.max(1) as f32)
    }
}

fn output_callback(
    shared: &Shared,
    eq: &mut crate::engine::eq::Equalizer,
    starved: &mut bool,
    data: &mut [f32],
    channels: usize,
) {
    let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
    let rg = f32::from_bits(shared.rg_factor.load(Ordering::Relaxed));
    let playing = shared.playing.load(Ordering::Relaxed);
    let mut taken_samples = 0usize;
    if playing && !data.is_empty() {
        let target = f32::from_bits(shared.fade_target.load(Ordering::Relaxed));
        let step = fade_step(
            shared.fade_ms.load(Ordering::Relaxed),
            shared.dev_rate.load(Ordering::Relaxed),
        );
        let mut gain = f32::from_bits(shared.gain.load(Ordering::Relaxed));
        let ch = channels.max(1);
        // 取样 → EQ → rg × 音量 × 淡变 → 钳到 ±1。
        let take = shared.ring.pop(data.len(), |i, sample| data[i] = sample);
        eq.process(&mut data[..take], ch);
        for slot in data[..take].iter_mut().enumerate() {
            let (i, x) = slot;
            // 每帧（而不是每个样本）推进一次增益，各声道一致。
            if i % ch == 0 && gain != target {
                gain = if gain < target {
                    (gain + step).min(target)
                } else {
                    (gain - step).max(target)
                };
            }
            *x = (*x * rg * volume * fade_curve(gain)).clamp(-1.0, 1.0);
        }
        // 欠载沿软化（~2ms）：缓冲没填满时给尾巴拉到 0、续播头端从 0
        // 拉回——硬零沿会咔哒。只在欠载边界动数据，正常播放无感。
        if take > 0 {
            let edge =
                ((shared.dev_rate.load(Ordering::Relaxed) / 500).max(1) as usize).min(take / ch);
            if *starved {
                for f in 0..edge {
                    let g = fade_curve(f as f32 / edge as f32);
                    for c in 0..ch {
                        data[f * ch + c] *= g;
                    }
                }
            }
            if take < data.len() {
                let last = take / ch;
                for f in 0..edge {
                    let g = fade_curve(1.0 - f as f32 / edge as f32);
                    for c in 0..ch {
                        data[(last - 1 - f) * ch + c] *= g;
                    }
                }
            }
        }
        *starved = take < data.len();
        // 缓冲空了（还没解出来）也照样推进淡出，不让引擎干等。
        if take == 0 && target < gain {
            gain = (gain - step * (data.len() / ch) as f32).max(target);
        }
        shared.gain.store(gain.to_bits(), Ordering::Relaxed);
        taken_samples = take;
    }
    if taken_samples < data.len() {
        for slot in data[taken_samples..].iter_mut() {
            *slot = 0.0;
        }
    }
    if taken_samples > 0 {
        let frames = (taken_samples / channels.max(1)) as u64;
        shared.played.fetch_add(frames, Ordering::Relaxed);
        shared.listened.fetch_add(frames, Ordering::Relaxed);
    }
}

/// 发布媒体会话状态。
fn publish_media(shared: &Arc<Shared>, media: &MediaSession, _force: bool) {
    let snap = shared.snapshot();
    media.publish(&snap);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_ramps_over_the_configured_time() {
        // 200 ms @ 48 kHz：9600 帧从 0 到 1。
        let step = fade_step(200, 48_000);
        assert!((1.0 / step - 9600.0).abs() < 0.5);
        assert_eq!(fade_step(0, 48_000), 1.0, "关闭时一步到位");
        assert_eq!(fade_curve(0.0), 0.0);
        assert_eq!(fade_curve(1.0), 1.0);
        assert!((fade_curve(0.5) - 0.5).abs() < 1e-6);
        let shared = Shared {
            playing: AtomicBool::new(true),
            loading: AtomicBool::new(false),
            ready: AtomicBool::new(true),
            volume: AtomicU32::new(1f32.to_bits()),
            played: AtomicU64::new(0),
            listened: AtomicU64::new(0),
            dev_rate: AtomicU32::new(48_000),
            ring: {
                let ring = Ring::new();
                ring.init(48_000 * 2);
                assert_eq!(ring.push(&vec![1.0_f32; 48_000 * 2], 2), 48_000 * 2);
                ring
            },
            track: Mutex::new(Arc::new(TrackPublic::default())),
            fade_ms: AtomicU32::new(200),
            fade_target: AtomicU32::new(1f32.to_bits()),
            gain: AtomicU32::new(0f32.to_bits()),
            loop_mode: AtomicU8::new(0),
            cover_gen: AtomicU64::new(0),
            pause_on_device_change: AtomicBool::new(true),
            stats_min_percent: AtomicU32::new(DEFAULT_STATS_MIN_PERCENT),
            stats_min_secs: AtomicU32::new(DEFAULT_STATS_MIN_SECS),
            plays_gen: AtomicU64::new(0),
            rg_factor: AtomicU32::new(1f32.to_bits()),
            rg_config: Mutex::new(ReplayGainConfig::default()),
            eq_gen: AtomicU64::new(0),
            eq_config: Mutex::new(crate::engine::eq::EqConfig::default()),
        };
        // 100 ms 后到一半（smoothstep(0.5) = 0.5），200 ms 后满。
        let mut eq = crate::engine::eq::Equalizer::new();
        let mut starved = false;
        let mut buf = vec![0.0_f32; 4800 * 2];
        output_callback(&shared, &mut eq, &mut starved, &mut buf, 2);
        assert!(
            (buf[buf.len() - 1] - 0.5).abs() < 0.01,
            "{}",
            buf[buf.len() - 1]
        );
        assert!(buf.windows(2).all(|w| w[1] >= w[0] - 1e-6), "淡入单调");
        output_callback(&shared, &mut eq, &mut starved, &mut buf, 2);
        assert!((buf[buf.len() - 1] - 1.0).abs() < 1e-3);
        // 淡出：目标 0，200 ms 后静音。
        shared.fade_target.store(0f32.to_bits(), Ordering::Relaxed);
        let mut long = vec![0.0_f32; 9600 * 2];
        output_callback(&shared, &mut eq, &mut starved, &mut long, 2);
        assert!(
            long[long.len() - 1].abs() < 1e-6
                && f32::from_bits(shared.gain.load(Ordering::Relaxed)) == 0.0
        );
    }

    /// 播放统计阈值：任一条件满足即计；0 关闭该条件；两个都 0 立即计。
    #[test]
    fn play_stats_threshold_rules() {
        // 默认 60% / 30 秒，60 秒的歌。
        assert!(!counts_as_play(20.0, 60.0, 60, 30)); // 33% 且 <30s
        assert!(counts_as_play(30.0, 60.0, 60, 30)); // 秒数到了
        assert!(counts_as_play(36.0, 60.0, 60, 30)); // 60% 到了
        assert!(counts_as_play(60.0, 100.0, 60, 30)); // 恰好 60%
        // 0 = 不用该条件。
        assert!(counts_as_play(1.0, 600.0, 0, 1)); // 只按秒
        assert!(!counts_as_play(0.5, 600.0, 0, 1));
        assert!(counts_as_play(300.0, 600.0, 50, 0)); // 只按百分比
        assert!(!counts_as_play(299.0, 600.0, 50, 0));
        // 两个都 0：开始播放即计。
        assert!(counts_as_play(0.0, 600.0, 0, 0));
        // 时长未知 / 为 0：百分比条件不生效，秒数仍可用。
        assert!(!counts_as_play(20.0, 0.0, 60, 30));
        assert!(counts_as_play(30.0, 0.0, 60, 30));
    }

    /// 环形缓冲：按序取出、绕回、只写整帧、`clear` 作废未播样本。
    #[test]
    fn ring_keeps_order_frames_and_discards_on_clear() {
        let ring = Ring::new();
        assert_eq!(ring.push(&[1.0, 2.0], 2), 0, "未分配时什么也不写");
        ring.init(8);
        assert_eq!(ring.push(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 2), 6);
        assert_eq!(ring.push(&[7.0, 8.0, 9.0], 2), 2, "放不下的半帧留给调用方");
        let mut got = Vec::new();
        assert_eq!(ring.pop(4, |_, s| got.push(s)), 4);
        assert_eq!(got, [1.0, 2.0, 3.0, 4.0]);
        // 绕回到开头的槽。
        assert_eq!(ring.push(&[9.0, 10.0, 11.0, 12.0], 2), 4);
        got.clear();
        ring.pop(usize::MAX, |_, s| got.push(s));
        assert_eq!(got, [5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0]);
        // clear 之后只剩新写入的样本。
        ring.push(&[1.0, 1.0], 2);
        ring.clear();
        assert_eq!(ring.len(), 0);
        ring.push(&[2.0, 2.0], 2);
        got.clear();
        ring.pop(usize::MAX, |_, s| got.push(s));
        assert_eq!(got, [2.0, 2.0]);
    }

    /// 两个线程同时写读：消费者拿到的是连续递增的序列，不丢不重。
    #[test]
    fn ring_is_consistent_across_threads() {
        let ring = Arc::new(Ring::new());
        ring.init(1024);
        const TOTAL: usize = 200_000;
        let producer = {
            let ring = ring.clone();
            std::thread::spawn(move || {
                let mut next = 0usize;
                while next < TOTAL {
                    let chunk: Vec<f32> =
                        (next..(next + 100).min(TOTAL)).map(|v| v as f32).collect();
                    next += ring.push(&chunk, 1);
                    std::thread::yield_now();
                }
            })
        };
        let mut expected = 0usize;
        while expected < TOTAL {
            ring.pop(256, |_, s| {
                assert_eq!(s as usize, expected);
                expected += 1;
            });
            std::thread::yield_now();
        }
        producer.join().unwrap();
    }

    /// 44.1 kHz → 48 kHz（Windows 共享模式的常见组合），按 MP3 / FLAC 那样
    /// 大小不一的包送入：输出连续、没有插入的静音或跳变，长度符合比例。
    #[test]
    fn resampling_odd_packets_is_continuous() {
        let (from, to, ch) = (44_100_u32, 48_000_u32, 2_usize);
        let freq = 1000.0_f32;
        let total = from as usize * 2; // 2 s
        let input: Vec<f32> = (0..total)
            .flat_map(|i| {
                let v = (i as f32 * freq * std::f32::consts::TAU / from as f32).sin() * 0.5;
                [v, v]
            })
            .collect();
        let mut r = ResamplerState::new(from, to, ch).unwrap();
        let mut out = Vec::new();
        let sizes = [1152_usize, 4096, 576, 1000, 4608, 17];
        let (mut at, mut k) = (0, 0);
        while at < total {
            let n = sizes[k % sizes.len()].min(total - at);
            r.push(&input[at * ch..(at + n) * ch], &mut out).unwrap();
            at += n;
            k += 1;
        }
        let frames = out.len() / ch;
        let expected = total as f64 * to as f64 / from as f64;
        assert!(
            (frames as f64) > expected - 3000.0 && (frames as f64) <= expected + 10.0,
            "{frames} vs {expected}"
        );
        // 跳过开头的滤波器延迟，此后相邻样本的差不超过正弦的最大斜率（加余量）。
        let max_step = 0.5 * freq * std::f32::consts::TAU / to as f32 * 1.3;
        let left: Vec<f32> = out.iter().step_by(ch).copied().collect();
        let worst = left[2048..]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst < max_step, "跳变 {worst} ≥ {max_step}");
    }

    /// 设备事件决策：Rerouted 不重建；Lost 播放中一定暂停；Invalidated
    /// 播放中按设置决定是否暂停、但总是重建。
    #[test]
    fn device_plan_decides_rebuild_and_pause() {
        for ev in [
            DeviceEvent::Rerouted,
            DeviceEvent::Invalidated,
            DeviceEvent::Lost,
        ] {
            for playing in [false, true] {
                for pref in [false, true] {
                    let (rebuild, pause) = device_plan(ev, playing, pref);
                    assert_eq!(
                        rebuild,
                        ev != DeviceEvent::Rerouted,
                        "{ev:?} playing={playing} pref={pref}"
                    );
                    let want_pause = playing && (pref || ev == DeviceEvent::Lost);
                    assert_eq!(pause, want_pause, "{ev:?} playing={playing} pref={pref}");
                }
            }
        }
        // 关键断言单独点出来（上表已覆盖，这里是语义文档）。
        assert_eq!(
            device_plan(DeviceEvent::Invalidated, true, false),
            (true, false)
        );
        assert_eq!(device_plan(DeviceEvent::Lost, true, false), (true, true));
    }

    /// 换到高规格设备后填充目标不超过容量的 2/3（容量只有一份）。
    #[test]
    fn device_target_never_exceeds_capacity() {
        let out = Output {
            stream: None,
            rate: 192_000,
            channels: 8,
            running: false,
        };
        let capacity = 48_000 * 3 * 2;
        let target = target_for(&out, capacity);
        assert!(target > 0);
        assert!(
            target <= capacity * 2 / 3,
            "{target} > {}",
            capacity * 2 / 3
        );
    }

    /// ReplayGain 增益：模式回退、预放大叠加、峰值限幅与上限钳制。
    #[test]
    fn replaygain_factor_follows_mode_and_fallbacks() {
        use crate::engine::audio::ReplayGainMode::*;
        use crate::metadata::ReplayGain;
        let cfg = |mode, preamp, fallback, clip| ReplayGainConfig {
            mode,
            preamp_db: preamp,
            fallback_db: fallback,
            clip_protect: clip,
        };
        let rg = |tg, tp, ag, ap| ReplayGain {
            track_gain: tg,
            track_peak: tp,
            album_gain: ag,
            album_peak: ap,
        };
        let near = |got: f32, want: f32, what: &str| {
            assert!((got - want).abs() < 1e-3, "{what}: {got} != {want}");
        };
        // 关闭 → 1.0；按曲目 −6.02 dB → ≈0.5。
        assert_eq!(
            replaygain_factor(&cfg(Off, 0.0, 0.0, true), &rg(Some(-6.0), None, None, None)),
            1.0
        );
        near(
            replaygain_factor(
                &cfg(Track, 0.0, 0.0, true),
                &rg(Some(-6.02), None, None, None),
            ),
            0.5,
            "track -6.02dB",
        );
        // 专辑模式没有专辑增益 → 回退曲目增益。
        near(
            replaygain_factor(
                &cfg(Album, 0.0, 0.0, true),
                &rg(Some(-6.02), None, None, None),
            ),
            0.5,
            "album 回退 track",
        );
        // 专辑模式有专辑增益 → 用专辑的。
        near(
            replaygain_factor(
                &cfg(Album, 0.0, 0.0, true),
                &rg(Some(-6.0), None, Some(-12.0), None),
            ),
            10f32.powf(-12.0 / 20.0),
            "album gain",
        );
        // 都没有 → 无标签增益。
        near(
            replaygain_factor(&cfg(Track, 0.0, -3.0, true), &rg(None, None, None, None)),
            10f32.powf(-3.0 / 20.0),
            "fallback",
        );
        // 预放大 +6 dB 与曲目 −6 dB 抵消 → ≈1。
        near(
            replaygain_factor(
                &cfg(Track, 6.0, 0.0, true),
                &rg(Some(-6.0), None, None, None),
            ),
            1.0,
            "preamp 叠加",
        );
        // 防削波：+6 dB 增益但峰值 0.8 → 钳到 1/0.8。
        near(
            replaygain_factor(
                &cfg(Track, 0.0, 0.0, true),
                &rg(Some(6.0), Some(0.8), None, None),
            ),
            1.0 / 0.8,
            "clip protect",
        );
        // 关掉防削波 → 全额 +6 dB。
        near(
            replaygain_factor(
                &cfg(Track, 0.0, 0.0, false),
                &rg(Some(6.0), Some(0.8), None, None),
            ),
            10f32.powf(6.0 / 20.0),
            "no clip protect",
        );
        // 上限钳到 4.0（+12 dB）。
        assert_eq!(
            replaygain_factor(
                &cfg(Track, 0.0, 0.0, false),
                &rg(Some(30.0), None, None, None)
            ),
            4.0
        );
    }

    /// 设备缓冲请求：目标 ~40ms；声明了范围的钳进范围，未知的直接要固定值。
    #[test]
    fn output_buffer_size_targets_forty_ms() {
        use cpal::{BufferSize, SupportedBufferSize};
        // 48 kHz → 1920 帧（40ms），落在范围内不动。
        assert_eq!(
            desired_buffer_size(&SupportedBufferSize::Range { min: 15, max: 4096 }, 48_000),
            BufferSize::Fixed(1920)
        );
        // 上限更小 / 下限更高时分别钳到端点。
        assert_eq!(
            desired_buffer_size(
                &SupportedBufferSize::Range {
                    min: 128,
                    max: 1024
                },
                48_000
            ),
            BufferSize::Fixed(1024)
        );
        assert_eq!(
            desired_buffer_size(
                &SupportedBufferSize::Range {
                    min: 4096,
                    max: 8192
                },
                44_100
            ),
            BufferSize::Fixed(4096)
        );
        // 范围未知（WASAPI 软栈常态）也要固定值——失败由调用方退默认。
        assert_eq!(
            desired_buffer_size(&SupportedBufferSize::Unknown, 48_000),
            BufferSize::Fixed(1920)
        );
    }

    /// 峰值限幅不会让输出超过 1.0。
    #[test]
    fn replaygain_factor_never_clips() {
        let cfg = ReplayGainConfig {
            mode: ReplayGainMode::Track,
            preamp_db: 0.0,
            fallback_db: 0.0,
            clip_protect: true,
        };
        let rg = crate::metadata::ReplayGain {
            track_gain: Some(6.0),
            track_peak: Some(0.8),
            ..Default::default()
        };
        let factor = replaygain_factor(&cfg, &rg);
        assert!(
            (factor - 1.0 / 0.8).abs() < 1e-3,
            "{factor} != {}",
            1.0 / 0.8
        );
        assert!(factor <= 4.0);
    }

    /// 智能洗牌：同 seed 同输入必同输出；结果是全排列；同一歌手的曲目
    /// 不再连成大段（对照均匀摊开的环上落点）。
    #[test]
    fn spread_order_is_deterministic_and_spreads_artists() {
        let artists: Vec<&str> = (0..30).map(|i| ["A", "B", "C"][i % 3]).collect();
        let order = spread_order(30, |i| artists[i], |_| 0.0, 42);
        assert_eq!(
            order,
            spread_order(30, |i| artists[i], |_| 0.0, 42),
            "同 seed 必须确定"
        );
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, (0..30).collect::<Vec<_>>(), "必须是全排列");
        // 相邻同歌手的最长连段：纯随机 30 首 3 歌手常见连段 ≥4；摊开应 ≤2。
        let mut run = 1;
        let mut worst = 1;
        for w in order.windows(2) {
            run = if artists[w[0]] == artists[w[1]] {
                run + 1
            } else {
                1
            };
            worst = worst.max(run);
        }
        assert!(worst <= 2, "同歌手最长连段 {worst} > 2");
        // 换 seed 应得不同顺序（极小概率相同就再换一个）。
        let other = spread_order(30, |i| artists[i], |_| 0.0, 7);
        assert_ne!(order, other, "不同 seed 顺序应不同");
    }

    /// 组内按 `rand × (score+1)` 加权：同一歌手的高分曲目更常占组内
    /// 前位（多 seed 汇总，避免单次偶然）。
    #[test]
    fn spread_order_weights_high_score_first_within_group() {
        // 一个歌手 4 首：index 0 得分 100，其余 0。
        let artists = vec!["S"; 4];
        let mut first = 0usize;
        for seed in 0..64u64 {
            let order = spread_order(
                4,
                |i| artists[i],
                |i| if i == 0 { 100.0 } else { 0.0 },
                seed,
            );
            if order[0] == 0 {
                first += 1;
            }
        }
        assert!(first >= 40, "高分曲在队首 {first}/64 次，加权应明显偏前");
    }
}
