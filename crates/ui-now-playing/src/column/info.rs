use super::*;

/// 左列宽度（= 封面边长）的上限。
pub(crate) const COLUMN: f32 = 326.0;

/// 左列宽度跟着窗口走：有歌词时左右分半，窗口矮封面也跟着缩
///（封面 + 下面的元数据 / 进度 / 控制 / 音量约 340 pt）。
pub(crate) fn column_size() -> f32 {
    let share = if crate::lyrics::lyrics_shown().get() {
        window().width.0 / 2.0
    } else {
        window().width.0
    };
    (share - 72.0)
        .min(window().height.0 - 380.0)
        .clamp(140.0, COLUMN)
}

/// 进度条下方的小字：24-bit / 96kHz / 2304kbps / Stereo。
pub(crate) fn audio_line(m: &MetaInfo) -> String {
    let mut parts = Vec::new();
    if let Some(depth) = m.bit_depth.filter(|d| *d > 0) {
        parts.push(format!("{depth}-bit"));
    }
    if let Some(rate) = m.sample_rate {
        let khz = rate as f64 / 1000.0;
        parts.push(if khz.fract() == 0.0 {
            format!("{khz:.0}kHz")
        } else {
            format!("{khz:.1}kHz")
        });
    }
    if let Some(rate) = m.bit_rate {
        parts.push(format!("{rate}kbps"));
    }
    match m.channels {
        Some(1) => parts.push("Mono".into()),
        Some(2) => parts.push("Stereo".into()),
        Some(n) if n > 2 => parts.push(format!("{n}ch")),
        _ => {}
    }
    parts.join(" / ")
}
