use super::*;

/// 解码一块并写入环形缓冲；返回 Ok(true) 表示当前数据源到尾。缓冲
/// 放不下的部分留在 `backlog`，下次先送；`backlog` 还没送完时不解码
/// 新包。`l.eof` 后数据源换成预取的 `next`（同一缓冲接着推，无缝）。
pub(crate) fn fill_ring(
    l: &mut Loaded,
    shared: &Arc<Shared>,
    dev_channels: usize,
) -> Result<bool, String> {
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
pub(crate) fn convert_channels(input: &[f32], from: usize, to: usize, out: &mut Vec<f32>) {
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
pub(crate) fn fade_out(
    shared: &Arc<Shared>,
    loaded: &mut Option<Loaded>,
    target: usize,
    dev_channels: usize,
) {
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
pub(crate) fn fade_curve(gain: f32) -> f32 {
    gain * gain * (3.0 - 2.0 * gain)
}

/// 每帧增益步长（`ms` 毫秒内从 0 到 1）。
pub(crate) fn fade_step(ms: u32, rate: u32) -> f32 {
    if ms == 0 {
        1.0
    } else {
        1000.0 / (ms as f32 * rate.max(1) as f32)
    }
}

pub(crate) fn output_callback(
    shared: &Shared,
    eq: &mut crate::eq::Equalizer,
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
pub(crate) fn publish_media(shared: &Arc<Shared>, media: &MediaSession, _force: bool) {
    let snap = shared.snapshot();
    media.publish(&snap);
}
