use super::*;

/// 当前输出：消费方 + 设备格式。没有可用设备时是 [`Sink::Null`] 空输出——
/// 按设备节拍消费环形缓冲（位置 / 播放统计 / 无缝换曲照常推进），设备
/// 插回时由 `retry_at` 换成真流并续回原位。
pub(crate) struct Output {
    pub(crate) sink: Option<Sink>,
    pub(crate) rate: u32,
    pub(crate) channels: usize,
    /// 流是否在跑（`play()`/`pause()` 的成功态）。停着时暂停流：省空闲
    /// CPU，也让 Windows 音频会话如实呈非活动态。
    pub(crate) running: bool,
}

impl Output {
    /// 真输出设备在跑（cpal 流），不是空输出兜底。
    pub(crate) fn real(&self) -> bool {
        matches!(self.sink, Some(Sink::Cpal(_)))
    }
}

pub(crate) enum Sink {
    Cpal(cpal::Stream),
    /// 无输出设备时的空输出：线程按设备节拍从环形缓冲取数，输出回调照常
    /// 走（EQ / 增益 / `played` / `listened` 全都一致）。CI 与无声卡
    /// 机器上播放通路仍可用；`running` 跟随播放态，暂停即停取。
    Null(NullSink),
}

pub(crate) struct NullSink {
    pub(crate) running: Arc<AtomicBool>,
    pub(crate) quit: Arc<AtomicBool>,
    pub(crate) handle: Option<std::thread::JoinHandle<()>>,
}

impl NullSink {
    pub(crate) fn start(shared: Arc<Shared>, rate: u32, channels: usize) -> NullSink {
        let running = Arc::new(AtomicBool::new(true));
        let quit = Arc::new(AtomicBool::new(false));
        let handle = std::thread::Builder::new()
            .name("zeo-player-null-out".into())
            .spawn({
                let running = running.clone();
                let quit = quit.clone();
                move || null_output_loop(&shared, &running, &quit, rate, channels)
            })
            .ok();
        NullSink {
            running,
            quit,
            handle,
        }
    }
}

impl Drop for NullSink {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// 空输出的消费循环：每 ~10ms 取一块（和 `OUTPUT_BUFFER_MS` 量级相当），
/// 节拍对齐 `Instant`——线程被抢占后不追赶，位置就是真实播放时长。
pub(crate) fn null_output_loop(
    shared: &Arc<Shared>,
    running: &Arc<AtomicBool>,
    quit: &Arc<AtomicBool>,
    rate: u32,
    channels: usize,
) {
    let mut eq = crate::eq::Equalizer::new();
    let mut eq_seen = u64::MAX;
    let mut starved = false;
    let chunk_frames = (rate as usize / 100).max(1);
    let mut data = vec![0.0f32; chunk_frames * channels.max(1)];
    let mut tick = Instant::now();
    while !quit.load(Ordering::Relaxed) {
        if !running.load(Ordering::Relaxed) {
            tick = Instant::now();
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        let g = shared.eq_gen.load(Ordering::Acquire);
        if g != eq_seen
            && let Ok(cfg) = shared.eq_config.try_lock()
        {
            eq.configure(&cfg, rate);
            eq_seen = g;
        }
        output_callback(shared, &mut eq, &mut starved, &mut data, channels);
        tick += Duration::from_secs_f64(chunk_frames as f64 / rate.max(1) as f64);
        let now = Instant::now();
        if tick > now {
            std::thread::sleep(tick - now);
        } else {
            tick = now;
        }
    }
}

/// 在系统默认设备上试开输出流：成功返回流与实际格式；任何失败记错误、
/// 返回 `None`（调用方落空输出兜底）。
pub(crate) fn probe_stream(
    shared: &Arc<Shared>,
    tx: &std::sync::mpsc::Sender<EngineCmd>,
) -> Option<(cpal::Stream, u32, usize)> {
    let Some(device) = cpal::default_host().default_output_device() else {
        set_error(shared, "没有可用的音频输出设备");
        return None;
    };
    let config = match device.default_output_config() {
        Ok(config) => config,
        Err(err) => {
            set_error(shared, &format!("音频设备读取失败: {err}"));
            return None;
        }
    };
    let rate = config.sample_rate();
    let channels = config.channels() as usize;
    let mut stream_config = config.config();
    stream_config.buffer_size = desired_buffer_size(config.buffer_size(), rate);
    let build = build_stream(&device, stream_config, shared, tx, rate, channels).or_else(|first| {
        // 设备不接受指定缓冲：退回系统默认（能响，只是调度余量小）。
        eprintln!("zeo-player cpal: {first}（退回默认缓冲）");
        build_stream(&device, config.config(), shared, tx, rate, channels)
    });
    match build {
        Ok(s) => (s.play().is_ok()).then_some((s, rate, channels)),
        Err(err) => {
            set_error(shared, &format!("音频输出打开失败: {err}"));
            None
        }
    }
}

/// 打开输出：默认设备可用就用真流，不可用落 [`Sink::Null`] 空输出兜底
/// （格式沿用 `fallback`；消费不中断，位置照常推进）。
pub(crate) fn open_output(
    shared: &Arc<Shared>,
    tx: &std::sync::mpsc::Sender<EngineCmd>,
    fallback: (u32, usize),
) -> Output {
    match probe_stream(shared, tx) {
        Some((stream, rate, channels)) => Output {
            sink: Some(Sink::Cpal(stream)),
            rate,
            channels,
            running: true,
        },
        None => {
            let (rate, channels) = fallback;
            Output {
                sink: Some(Sink::Null(NullSink::start(shared.clone(), rate, channels))),
                rate,
                channels,
                running: true,
            }
        }
    }
}

/// 想要的设备缓冲（`StreamConfig::buffer_size`）：目标是 [`OUTPUT_BUFFER_MS`]
/// 毫秒的设备缓冲——默认缓冲在不少设备上只有 ~10ms，CPU 忙时回调线程稍一
/// 排队就欠载爆音。声明了支持范围的钳进范围；范围未知（WASAPI 软栈的常态）
/// 也按固定值要——`open_output` 在建流失败时退回 `Default`。
pub(crate) fn desired_buffer_size(
    range: &cpal::SupportedBufferSize,
    rate: u32,
) -> cpal::BufferSize {
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
pub(crate) fn build_stream(
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
            let mut eq = crate::eq::Equalizer::new();
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
pub(crate) fn sync_stream(out: &mut Output, playing: bool) {
    if playing == out.running {
        return;
    }
    match out.sink.as_ref() {
        Some(Sink::Cpal(stream)) => {
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
        Some(Sink::Null(null)) => {
            null.running.store(playing, Ordering::Relaxed);
            out.running = playing;
        }
        None => {}
    }
}

/// 填充目标：设备格式算出的目标，但不超过容量的 2/3（换到更高采样率 /
/// 更多声道的设备后容量不变，只是缓冲的秒数变少）。
pub(crate) fn target_for(out: &Output, capacity: usize) -> usize {
    ring_target(out.rate, out.channels)
        .min(capacity * 2 / 3)
        .max(1)
}

/// 设备事件之后要不要重建输出流、要不要暂停（纯决策，供测试）。
pub(crate) fn device_plan(ev: DeviceEvent, playing: bool, pause_pref: bool) -> (bool, bool) {
    let rebuild = matches!(ev, DeviceEvent::Invalidated | DeviceEvent::Lost);
    // 设备丢了一定暂停；只是换设备时按设置决定。
    let pause = playing && (pause_pref || ev == DeviceEvent::Lost);
    (rebuild, pause)
}

/// 重建成功后恢复：设备格式推进共享状态；有曲目时按新格式重建重采样器、
/// seek 回原位置并重置缓冲基线（`current_time` 要先按旧 `dev_rate` 读）。
pub(crate) fn restore_after_rebuild(
    shared: &Arc<Shared>,
    out: &Output,
    loaded: &mut Option<Loaded>,
) {
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
pub(crate) fn handle_device(
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
    drop(out.sink.take());
    let fallback = (out.rate, out.channels);
    *out = open_output(shared, tx, fallback);
    // 新消费方（真流或空输出）都是 running 态；停着时立刻同步——真流保持
    // 会话非活动，空输出停取省 CPU。
    sync_stream(out, shared.playing.load(Ordering::Relaxed));
    if out.real() {
        restore_after_rebuild(shared, out, loaded);
        None
    } else {
        Some(Instant::now() + Duration::from_secs(1))
    }
}
