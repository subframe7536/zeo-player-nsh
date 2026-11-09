use super::*;

pub(crate) fn run(
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
    // 设备断开后的重开时刻（`Some` 期间每秒试一次）。开局就没设备
    // （空输出兜底）也排上：设备插回后无事件也照接。
    let mut retry_at: Option<Instant> =
        (!out.real()).then(|| Instant::now() + Duration::from_secs(1));

    loop {
        // 0. 设备重试：空输出期间每秒探一次真设备；回来了就换流、按原位置
        //    恢复。探测不动空输出本身——没探到时消费不中断。
        if let Some(at) = retry_at
            && Instant::now() >= at
        {
            match probe_stream(&shared, &tx) {
                Some((stream, rate, channels)) => {
                    drop(out.sink.take());
                    out.sink = Some(Sink::Cpal(stream));
                    out.rate = rate;
                    out.channels = channels;
                    out.running = true;
                    sync_stream(&mut out, shared.playing.load(Ordering::Relaxed));
                    restore_after_rebuild(&shared, &out, &mut loaded);
                    retry_at = None;
                    target = target_for(&out, shared.ring.capacity());
                }
                None => retry_at = Some(Instant::now() + Duration::from_secs(1)),
            }
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
                && (out.sink.is_none()
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
                        || out.sink.is_none()
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
                        let at = zeo_types::config::unix_now();
                        zeo_db::later(move || zeo_db::record_play(&path, at, listened));
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
pub(crate) fn raise_thread_priority() {
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
