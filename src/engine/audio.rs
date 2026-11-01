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

/// 每轮最多解码的包数（防止一轮占太久，命令响应变慢）。
const MAX_PACKETS_PER_TICK: usize = 64;
/// 播放中两轮之间最多等多久（有命令时立即醒）。缓冲有 2 秒，不必频繁醒。
const PLAYING_TICK: Duration = Duration::from_millis(10);
/// 停着时两轮之间最多等多久。
const IDLE_TICK: Duration = Duration::from_millis(100);

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
}

/// 曲目公开状态（跨线程读）。
#[derive(Default)]
pub struct TrackPublic {
    pub path: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Option<String>,
    /// 全屏背景（预先模糊的小图）。
    pub backdrop: Option<String>,
    /// 这首的封面已生成（`artwork` / `backdrop` 为 `None` 时即确实没有封面）。
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
    pub dev_rate: AtomicU32,
    pub ring: Ring,
    pub track: Mutex<TrackPublic>,
    /// 淡入淡出时长（毫秒；0 关闭）。
    pub fade_ms: AtomicU32,
    /// 淡变目标（f32 位，0 或 1；回调朝它推进 `gain`）。
    pub fade_target: AtomicU32,
    /// 当前淡变增益（f32 位，0..1；播放中由回调写，停着时由引擎写）。
    pub gain: AtomicU32,
    /// 循环模式（[`LoopMode`] 的判别值）。
    pub loop_mode: AtomicU8,
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

/// 面向 TSX 的完整状态快照。
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub is_playing: bool,
    pub is_loading: bool,
    pub is_ready: bool,
    pub current_time: f64,
    pub duration: f64,
    pub volume: f64,
    pub path: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Option<String>,
    /// 全屏背景（预先模糊的小图）。
    pub backdrop: Option<String>,
    /// 这首的封面已生成。
    pub covers_ready: bool,
    pub queue: Arc<Vec<String>>,
    pub queue_index: i64,
    pub loop_mode: LoopMode,
    pub error: Option<String>,
}

impl Shared {
    pub fn snapshot(&self) -> Snapshot {
        let track = self.track.lock().unwrap();
        Snapshot {
            is_playing: self.playing.load(Ordering::Relaxed),
            is_loading: self.loading.load(Ordering::Relaxed),
            is_ready: self.ready.load(Ordering::Relaxed),
            current_time: (self.current_time() * 100.0).round() / 100.0, // timeupdate 精度 0.01s
            duration: track.duration,
            volume: self.volume(),
            path: track.path.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            artwork: track.artwork.clone(),
            backdrop: track.backdrop.clone(),
            covers_ready: track.covers_ready,
            queue: track.queue.clone(),
            queue_index: track.queue_index,
            loop_mode: self.loop_mode(),
            error: track.error.clone(),
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
            dev_rate: AtomicU32::new(48_000),
            ring: Ring::new(),
            track: Mutex::new(TrackPublic::default()),
            loop_mode: AtomicU8::new(0),
        });
        let media = MediaSession::start(tx.clone());
        std::thread::Builder::new()
            .name("zeo-player-audio".into())
            .spawn({
                let shared = shared.clone();
                move || run(rx, shared, media)
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
    /// 解码到了文件末尾：等缓冲里的尾巴播完再换下一首。
    drain: Option<Drain>,
    /// 封面还没生成（缓冲先填起来再做，免得开头卡住）。
    cover_pending: bool,
}

/// 文件解码完、等缓冲播空的状态（`len` 最近的缓冲长度，`since` 它最后
/// 一次变短的时刻，用来发现输出停摆）。
struct Drain {
    len: usize,
    since: Instant,
}

impl Loaded {
    /// 作废暂存（seek / 停止后）。
    fn reset_buffers(&mut self) {
        self.carry.clear();
        self.backlog.clear();
        self.drain = None;
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

fn run(rx: std::sync::mpsc::Receiver<EngineCmd>, shared: Arc<Shared>, media: MediaSession) {
    // 输出流：默认设备 + 默认配置（f32）。
    let mut dev_rate: u32 = 48_000;
    let mut dev_channels: usize = 2;
    let mut _stream = None;
    match cpal::default_host().default_output_device() {
        Some(device) => match device.default_output_config() {
            Ok(config) => {
                dev_rate = config.sample_rate();
                dev_channels = config.channels() as usize;
                let stream_config: cpal::StreamConfig = config.into();
                let build = device.build_output_stream(
                    stream_config,
                    {
                        let shared = shared.clone();
                        let dev_channels = dev_channels;
                        move |data: &mut [f32], _| output_callback(&shared, data, dev_channels)
                    },
                    |err| eprintln!("zeo-player cpal: {err}"),
                    None,
                );
                match build {
                    Ok(stream) => {
                        stream.play().ok();
                        _stream = Some(stream);
                    }
                    Err(err) => set_error(&shared, &format!("音频输出打开失败: {err}")),
                }
            }
            Err(err) => set_error(&shared, &format!("音频设备读取失败: {err}")),
        },
        None => set_error(&shared, "没有可用的音频输出设备"),
    }
    shared.dev_rate.store(dev_rate, Ordering::Relaxed);
    shared
        .ring
        .init(dev_rate as usize * RING_CAPACITY_SECS * dev_channels);
    let target = ring_target(dev_rate, dev_channels);
    let has_output = _stream.is_some();
    raise_thread_priority();

    let mut loaded: Option<Loaded> = None;
    // 原始顺序的队列（`Load` 传入）。`track.queue` 是按循环模式重排后
    // 的播放顺序，切模式时从这里重建。
    let mut base_queue: Arc<Vec<String>> = Arc::new(Vec::new());
    let mut media_tick = Instant::now();
    // 上一轮等待时收到的命令（等待用 recv_timeout，有命令立即醒）。
    let mut waiting: Option<EngineCmd> = None;

    loop {
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
                fade_out(&shared, &mut loaded, target, dev_channels);
            }
            handle_cmd(
                &shared,
                &media,
                &mut loaded,
                &mut base_queue,
                cmd,
                dev_rate,
                dev_channels,
            );
            if !shared.playing.load(Ordering::Relaxed) {
                // 停着：下次开始从静音淡入。
                shared.gain.store(0f32.to_bits(), Ordering::Relaxed);
            }
            shared.fade_target.store(1f32.to_bits(), Ordering::Relaxed);
        }
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
                    result = fill_ring(l, &shared, dev_channels);
                    if !matches!(result, Ok(false)) {
                        break;
                    }
                }
                match result {
                    Ok(true) => {
                        l.drain = Some(Drain {
                            len: shared.ring.len(),
                            since: Instant::now(),
                        })
                    }
                    Ok(false) => {}
                    Err(err) => {
                        set_error(&shared, &err);
                        shared.playing.store(false, Ordering::Relaxed);
                    }
                }
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
                    && (len == 0 || !has_output || drain.since.elapsed() > Duration::from_secs(1));
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
                    dev_rate,
                    dev_channels,
                );
                shared.playing.store(true, Ordering::Relaxed);
            } else {
                shared.playing.store(false, Ordering::Relaxed);
                loaded = None;
                shared.ready.store(false, Ordering::Relaxed);
                publish_media(&shared, &media, true);
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
/// （输出回调所在的线程由 cpal / 系统设成实时优先级，这里是喂它的线程。）
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
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
        };
        // SAFETY: GetCurrentThread 返回当前线程的伪句柄，无需关闭。
        unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
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
            let mut t = shared.track.lock().unwrap();
            let queue = ordered_queue(base_queue, mode);
            if let Some(path) = &t.path {
                t.queue_index = queue
                    .iter()
                    .position(|p| p == path)
                    .map(|i| i as i64)
                    .unwrap_or(t.queue_index.max(0));
            }
            t.queue = queue;
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

/// Fisher–Yates 洗牌（xorshift64 做种子，不必引入 rand）。
fn shuffled(base: &[String]) -> Vec<String> {
    let mut out = base.to_vec();
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        | 0x9e37_79b9_7f4a_7c15;
    for i in (1..out.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed % (i as u64 + 1)) as usize;
        out.swap(i, j);
    }
    out
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
            *loaded = Some(Loaded {
                source,
                resampler,
                carry: Vec::new(),
                out: Vec::new(),
                backlog: Vec::new(),
                src_channels,
                drain: None,
                cover_pending: true,
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
            {
                let mut t = shared.track.lock().unwrap();
                t.path = Some(path.to_string_lossy().into_owned());
                t.title = meta.title.clone();
                t.artist = meta.artist.clone();
                t.album = meta.album.clone();
                // 封面在就绪后再生成（缩小的文件，不放原图字节）。
                t.artwork = None;
                t.backdrop = None;
                t.covers_ready = false;
                t.duration = meta.duration;
                t.queue = queue;
                t.queue_index = actual_index;
                t.error = None;
            }
            shared.ready.store(true, Ordering::Relaxed);
            // 封面由引擎循环在缓冲填起来之后生成（`load_covers`）。
            media.set_artwork_file(None);
            publish_media(shared, media, true);
        }
        Err(err) => set_error(shared, &err),
    }
    shared.loading.store(false, Ordering::Relaxed);
}

/// 给当前曲目生成封面：缩到 720 px 的 JPEG 缓存文件（界面与系统媒体会话
/// 共用）和全屏背景小图。
fn load_covers(shared: &Arc<Shared>, media: &MediaSession) {
    let Some(path) = shared.track.lock().unwrap().path.clone() else {
        return;
    };
    let file = std::path::Path::new(&path);
    let cover = crate::metadata::covers::large(file).map(|p| p.to_string_lossy().into_owned());
    let backdrop = crate::metadata::covers::backdrop(file).map(|p| p.to_string_lossy().into_owned());
    {
        let mut t = shared.track.lock().unwrap();
        if t.path.as_deref() != Some(path.as_str()) {
            return;
        }
        t.artwork = cover.clone();
        t.backdrop = backdrop;
        t.covers_ready = true;
    }
    media.set_artwork_file(cover.map(|c| file_url(&c)));
    publish_media(shared, media, true);
}

/// 本地路径 → `file://` URL（Windows 盘符路径同样处理）。
fn file_url(path: &str) -> String {
    let path = path.replace('\\', "/");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
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
    shared.track.lock().unwrap().error = Some(message.to_owned());
}

/// 解码一块并写入环形缓冲；返回 Ok(true) 表示自然结束。缓冲放不下的部分
/// 留在 `backlog`，下次先送；`backlog` 还没送完时不解码新包。
fn fill_ring(l: &mut Loaded, shared: &Arc<Shared>, dev_channels: usize) -> Result<bool, String> {
    if !l.backlog.is_empty() {
        let n = shared.ring.push(&l.backlog, dev_channels);
        l.backlog.drain(..n);
        if !l.backlog.is_empty() {
            return Ok(false);
        }
    }
    let Some(block) = l.source.next_block()? else {
        return Ok(true);
    };
    if block.len() / l.src_channels.max(1) == 0 {
        return Ok(false);
    }
    // 1) 声道数转换 → dev_channels
    convert_channels(block, l.src_channels, dev_channels, &mut l.carry);
    // 2) 重采样或直通
    let samples = if let Some(r) = l.resampler.as_mut() {
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

fn output_callback(shared: &Shared, data: &mut [f32], channels: usize) {
    let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
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
        let take = shared.ring.pop(data.len(), |i, sample| {
            // 每帧（而不是每个样本）推进一次增益，各声道一致。
            if i % ch == 0 && gain != target {
                gain = if gain < target {
                    (gain + step).min(target)
                } else {
                    (gain - step).max(target)
                };
            }
            data[i] = sample * volume * fade_curve(gain);
        });
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
            dev_rate: AtomicU32::new(48_000),
            ring: {
                let ring = Ring::new();
                ring.init(48_000 * 2);
                assert_eq!(ring.push(&vec![1.0_f32; 48_000 * 2], 2), 48_000 * 2);
                ring
            },
            track: Mutex::new(TrackPublic::default()),
            fade_ms: AtomicU32::new(200),
            fade_target: AtomicU32::new(1f32.to_bits()),
            gain: AtomicU32::new(0f32.to_bits()),
            loop_mode: AtomicU8::new(0),
        };
        // 100 ms 后到一半（smoothstep(0.5) = 0.5），200 ms 后满。
        let mut buf = vec![0.0_f32; 4800 * 2];
        output_callback(&shared, &mut buf, 2);
        assert!(
            (buf[buf.len() - 1] - 0.5).abs() < 0.01,
            "{}",
            buf[buf.len() - 1]
        );
        assert!(buf.windows(2).all(|w| w[1] >= w[0] - 1e-6), "淡入单调");
        output_callback(&shared, &mut buf, 2);
        assert!((buf[buf.len() - 1] - 1.0).abs() < 1e-3);
        // 淡出：目标 0，200 ms 后静音。
        shared.fade_target.store(0f32.to_bits(), Ordering::Relaxed);
        let mut long = vec![0.0_f32; 9600 * 2];
        output_callback(&shared, &mut long, 2);
        assert!(
            long[long.len() - 1].abs() < 1e-6
                && f32::from_bits(shared.gain.load(Ordering::Relaxed)) == 0.0
        );
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
}
