//! 设置 →「播放」分类的各节。

use nsh_core::prelude::*;

use super::super::super::store::store;
use super::setting_section;

/// 播放：淡入淡出时长。
pub(super) fn playback_section() -> View {
    let s = store();
    const STEPS: &[(u32, &str)] = &[
        (0, "关闭"),
        (100, "0.1 秒"),
        (200, "0.2 秒"),
        (400, "0.4 秒"),
        (800, "0.8 秒"),
        (1500, "1.5 秒"),
    ];
    setting_section(
        "淡入淡出",
        "播放、暂停、拖动进度和切歌时，音量在这段时间里渐变，不会突然响起或戛然而止。一首自然播完接下一首时不淡变。",
        None,
    )
    .child(super::super::super::components::radio::group_row().children(STEPS.iter().map(|&(ms, label)| {
        super::super::super::components::radio::item(move || s.library.with(|l| l.config.fade_ms == ms), label, move || super::super::super::store::set_fade_ms(ms))
    })))
}
