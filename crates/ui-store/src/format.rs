use super::*;

// --- 格式化 -------------------------------------------------------------------

/// 分:秒。
pub fn format_time(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds.floor() as u64
    } else {
        0
    };
    format!("{}:{:02}", total / 60, total % 60)
}

/// 当前播放进度（拖动时为预览）。
pub fn shown_time() -> f64 {
    let s = store();
    s.seek_preview
        .get()
        .unwrap_or_else(|| s.playback.with(|p| p.time))
}

/// 当前进度比例。
pub fn progress_fraction() -> f64 {
    let s = store();
    let duration = s.playback.with(|p| p.duration);
    if duration > 0.0 {
        (shown_time() / duration).min(1.0)
    } else {
        0.0
    }
}

/// 总时长：1 小时 5 分钟 / 42 分钟。
pub fn format_total(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    if minutes >= 60 {
        format!("{} 小时 {} 分钟", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} 分钟")
    }
}
