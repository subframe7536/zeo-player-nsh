use super::*;

pub(crate) fn handle_cmd(
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
