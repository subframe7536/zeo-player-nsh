//! 播放引擎：symphonia 解码 →（rubato 重采样）→ cpal 输出。
//! 引擎线程拥有解码器与命令循环；cpal 回调经共享环形缓冲取数；
//! UI 通过快照 + 轮询任务读取状态（PlayerState 等价字段）。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::Resampler;

use crate::codec::Source;
use crate::media::MediaSession;
use crate::metadata::{data_uri, MetaCache, MetaInfo};

/// 环形缓冲上限（输出帧数，约 250ms @48k）。
const RING_MAX_FRAMES: usize = 12_000;

pub enum EngineCmd {
    Load { path: PathBuf, queue: Vec<String> },
    Play,
    Pause,
    Toggle,
    Stop,
    Seek(f64),
    SeekBy(f64),
    SetVolume(f64),
    Next,
    Previous,
}

/// 曲目公开状态（跨线程读）。
#[derive(Default)]
pub struct TrackPublic {
    pub path: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Option<String>,
    pub duration: f64,
    pub queue: Vec<String>,
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
    pub epoch: AtomicU64,
    pub cb_seen_epoch: AtomicU64, // 回调侧已处理的代际
    pub ring: Mutex<VecDeque<f32>>,
    pub track: Mutex<TrackPublic>,
}

impl Shared {
    pub fn current_time(&self) -> f64 {
        let rate = self.dev_rate.load(Ordering::Relaxed).max(1) as f64;
        self.played.load(Ordering::Relaxed) as f64 / rate
    }
    pub fn volume(&self) -> f64 {
        f32::from_bits(self.volume.load(Ordering::Relaxed)) as f64
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
    pub queue: Vec<String>,
    pub queue_index: i64,
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
            queue: track.queue.clone(),
            queue_index: track.queue_index,
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
            played: AtomicU64::new(0),
            dev_rate: AtomicU32::new(48_000),
            epoch: AtomicU64::new(0),
            cb_seen_epoch: AtomicU64::new(0),
            ring: Mutex::new(VecDeque::new()),
            track: Mutex::new(TrackPublic::default()),
        });
        let media = MediaSession::start(tx.clone());
        std::thread::Builder::new()
            .name("zeo-audio".into())
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
    carry: Vec<f32>,           // 通道转换后的输入暂存（交交错、dev 声道数）
    src_rate: u32,
    src_channels: usize,
}

struct ResamplerState {
    fft: rubato::Fft<f32>,
    input_scratch: Vec<f32>,
    output_scratch: Vec<f32>,
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
                    |err| eprintln!("zeo cpal: {err}"),
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
        None => set_error(&shared, "没有可用的音频输出设备".into()),
    }
    shared.dev_rate.store(dev_rate, Ordering::Relaxed);

    let mut loaded: Option<Loaded> = None;
    let mut media_tick = std::time::Instant::now();

    loop {
        // 1. 命令
        while let Ok(cmd) = rx.try_recv() {
            handle_cmd(&shared, &media, &mut loaded, cmd, dev_rate, dev_channels);
        }
        // 2. 解码填充
        if let Some(l) = loaded.as_mut() {
            let playing = shared.playing.load(Ordering::Relaxed);
            let ring_len = shared.ring.lock().unwrap().len();
            if playing && ring_len < RING_MAX_FRAMES * dev_channels {
                match fill_ring(l, &shared, dev_channels) {
                    Ok(ended) => {
                        if ended {
                            let (queue, index) = {
                                let t = shared.track.lock().unwrap();
                                (t.queue.clone(), t.queue_index)
                            };
                            let next_index = index + 1;
                            if let Some(next_path) = queue.get(next_index.max(0) as usize).cloned() {
                                load_track(&shared, &media, &mut loaded, PathBuf::from(next_path), queue, next_index, dev_rate, dev_channels);
                                shared.playing.store(true, Ordering::Relaxed);
                            } else {
                                shared.playing.store(false, Ordering::Relaxed);
                                loaded = None;
                                shared.ready.store(false, Ordering::Relaxed);
                                publish_media(&shared, &media, true);
                            }
                        }
                    }
                    Err(err) => {
                        set_error(&shared, &err);
                        shared.playing.store(false, Ordering::Relaxed);
                    }
                }
            }
        }
        // 3. 媒体会话位置心跳（仅在播放中，约 1s）
        if media_tick.elapsed() >= Duration::from_secs(1) {
            media_tick = std::time::Instant::now();
            if shared.playing.load(Ordering::Relaxed) {
                publish_media(&shared, &media, false);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn handle_cmd(
    shared: &Arc<Shared>,
    media: &MediaSession,
    loaded: &mut Option<Loaded>,
    cmd: EngineCmd,
    dev_rate: u32,
    dev_channels: usize,
) {
    match cmd {
        EngineCmd::Load { path, queue } => {
            load_track(shared, media, loaded, path, queue, -1, dev_rate, dev_channels);
            // queue_index 由 load_track 解析实际下标；默认从头播放。
            shared.playing.store(true, Ordering::Relaxed);
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
            }
            reset_ring(shared, 0.0, dev_rate);
            publish_media(shared, media, true);
        }
        EngineCmd::Seek(seconds) => {
            if let Some(l) = loaded.as_mut() {
                match l.source.seek(seconds.max(0.0)) {
                    Ok(actual) => {
                        reset_ring(shared, actual, dev_rate);
                        l.carry.clear();
                        if let Some(r) = l.resampler.as_mut() {
                            // 重建重采样器以清空其内部延迟线。
                            let new = rubato::Fft::<f32>::new(
                                l.src_rate as usize,
                                dev_rate as usize,
                                1024,
                                dev_channels,
                                rubato::FixedSync::Input,
                            );
                            if let Ok(fft) = new {
                                r.fft = fft;
                                r.output_scratch.clear();
                            }
                        }
                        publish_media(shared, media, true);
                    }
                    Err(err) => set_error(shared, &err),
                }
            }
        }
        EngineCmd::SeekBy(delta) => {
            let target = shared.current_time() + delta;
            handle_cmd(shared, media, loaded, EngineCmd::Seek(target), dev_rate, dev_channels);
        }
        EngineCmd::SetVolume(volume) => {
            let clamped = volume.clamp(0.0, 1.0) as f32;
            shared.volume.store(clamped.to_bits(), Ordering::Relaxed);
            publish_media(shared, media, false);
        }
        EngineCmd::Next | EngineCmd::Previous => {
            let (queue, index) = {
                let t = shared.track.lock().unwrap();
                (t.queue.clone(), t.queue_index)
            };
            let target = if matches!(cmd, EngineCmd::Next) { index + 1 } else { index - 1 };
            if let Some(path) = queue.get(target.max(0) as usize).cloned() {
                if target >= 0 {
                    load_track(shared, media, loaded, PathBuf::from(path), queue, target, dev_rate, dev_channels);
                    shared.playing.store(true, Ordering::Relaxed);
                    publish_media(shared, media, true);
                }
            }
        }
    }
}

/// 装载一个曲目（引擎线程内）：读元数据、封面、打开解码器。
fn load_track(
    shared: &Arc<Shared>,
    media: &MediaSession,
    loaded: &mut Option<Loaded>,
    path: PathBuf,
    queue: Vec<String>,
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
                match rubato::Fft::<f32>::new(
                    src_rate as usize,
                    dev_rate as usize,
                    1024,
                    dev_channels,
                    rubato::FixedSync::Input,
                ) {
                    Ok(fft) => Some(ResamplerState { fft, input_scratch: Vec::new(), output_scratch: Vec::new() }),
                    Err(err) => {
                        set_error(shared, &format!("重采样器创建失败: {err}"));
                        shared.loading.store(false, Ordering::Relaxed);
                        return;
                    }
                }
            } else {
                None
            };
            *loaded = Some(Loaded { source, resampler, carry: Vec::new(), src_rate, src_channels });

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
                t.artwork = meta.artwork.as_ref().map(|bytes| data_uri(bytes, meta.artwork_mime.as_deref()));
                t.duration = meta.duration;
                t.queue = queue;
                t.queue_index = actual_index;
                t.error = None;
            }
            shared.ready.store(true, Ordering::Relaxed);
            // 媒体会话封面：写临时文件（file:// 由 macOS 后台线程拉取）。
            let artwork_file = write_cover_file(&path, &meta);
            media.set_artwork_file(artwork_file);
            publish_media(shared, media, true);
        }
        Err(err) => set_error(shared, &err),
    }
    shared.loading.store(false, Ordering::Relaxed);
}

fn write_cover_file(path: &std::path::Path, meta: &MetaInfo) -> Option<String> {
    let bytes = meta.artwork.as_ref()?;
    let ext = match meta.artwork_mime.as_deref() {
        Some("image/png") => "png",
        _ => "jpg",
    };
    let dir = dirs::cache_dir().unwrap_or_else(|| std::env::temp_dir()).join("Zeo/covers");
    std::fs::create_dir_all(&dir).ok()?;
    let name = format!(
        "{}-{}.{ext}",
        path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        meta.modified_key()
    );
    let file = dir.join(name);
    std::fs::write(&file, bytes.as_ref()).ok()?;
    Some(format!("file://{}", file.to_string_lossy()))
}

impl MetaInfo {
    fn modified_key(&self) -> u64 {
        std::fs::metadata(&self.path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// 清空环形缓冲并把位置基线设为 `seconds`。
fn reset_ring(shared: &Arc<Shared>, seconds: f64, dev_rate: u32) {
    shared.epoch.fetch_add(1, Ordering::Relaxed);
    shared.ring.lock().unwrap().clear();
    shared
        .played
        .store((seconds * dev_rate as f64).round() as u64, Ordering::Relaxed);
}

fn set_error(shared: &Arc<Shared>, message: &str) {
    eprintln!("zeo: {message}");
    shared.track.lock().unwrap().error = Some(message.to_owned());
}

/// 解码一块并写入环形缓冲；返回 Ok(true) 表示自然结束。
fn fill_ring(l: &mut Loaded, shared: &Arc<Shared>, dev_channels: usize) -> Result<bool, String> {
    let Some(block) = l.source.next_block()? else {
        return Ok(true);
    };
    let frames = block.len() / l.src_channels.max(1);
    if frames == 0 {
        return Ok(false);
    }
    // 1) 声道数转换 → dev_channels
    convert_channels(block, l.src_channels, dev_channels, &mut l.carry);
    // 2) 重采样或直通
    if let Some(r) = l.resampler.as_mut() {
        let mut consumed = 0usize;
        while consumed < frames {
            let needed = r.fft.input_frames_next().max(1);
            let take = needed.min(frames - consumed);
            let is_last_partial = take < needed;
            r.input_scratch.clear();
            r.input_scratch.extend_from_slice(
                &l.carry[consumed * dev_channels..(consumed + take) * dev_channels],
            );
            if is_last_partial {
                // 包内尾块不足一块：补零凑整（可忽略的边界损失）。
                r.input_scratch.resize(needed * dev_channels, 0.0);
            }
            let out_max = r.fft.output_frames_max();
            r.output_scratch.resize(out_max * dev_channels, 0.0);
            let input = rubato::audioadapter_buffers::direct::InterleavedSlice::new(
                &r.input_scratch,
                dev_channels,
                needed,
            )
            .map_err(|e| format!("重采样输入错误: {e}"))?;
            let mut output = rubato::audioadapter_buffers::direct::InterleavedSlice::new_mut(
                &mut r.output_scratch,
                dev_channels,
                out_max,
            )
            .map_err(|e| format!("重采样输出错误: {e}"))?;
            let (_, written) = r
                .fft
                .process_into_buffer(&input, &mut output, None)
                .map_err(|e| format!("重采样错误: {e}"))?;
            if written > 0 {
                shared
                    .ring
                    .lock()
                    .unwrap()
                    .extend(r.output_scratch[..written * dev_channels].iter().copied());
            }
            consumed += take;
            if is_last_partial {
                break;
            }
        }
    } else {
        shared.ring.lock().unwrap().extend(l.carry.iter().copied());
    }
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
fn output_callback(shared: &Shared, data: &mut [f32], channels: usize) {
    let epoch = shared.epoch.load(Ordering::Relaxed);
    if epoch != shared.cb_seen_epoch.load(Ordering::Relaxed) {
        shared.cb_seen_epoch.store(epoch, Ordering::Relaxed);
        shared.ring.lock().unwrap().clear();
    }
    let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
    let playing = shared.playing.load(Ordering::Relaxed);
    let mut taken_samples = 0usize;
    if playing && !data.is_empty() {
        let mut ring = shared.ring.lock().unwrap();
        let take = ring.len().min(data.len());
        for slot in data.iter_mut().take(take) {
            *slot = ring.pop_front().unwrap_or(0.0) * volume;
        }
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
