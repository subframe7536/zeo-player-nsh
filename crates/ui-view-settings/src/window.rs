//! 设置 →「窗口」分类的各节。

use nsh_core::prelude::*;

use super::{pref_row, setting_section};
use zeo_ui_store::store;

/// 关闭窗口时：每次询问 / 最小化到托盘 / 退出。
pub(crate) fn window_section() -> View {
    use zeo_types::config::CloseAction;
    let s = store();
    let option = move |action: CloseAction, label: &'static str, hint: &'static str| {
        zeo_ui_components::radio::card(
            move || s.library.with(|l| l.config.close_action == action),
            label,
            hint,
            move || zeo_ui_store::set_close_action(action),
        )
    };
    let supported = cfg!(any(target_os = "macos", target_os = "windows"));
    setting_section(
        "关闭窗口时",
        if supported {
            "最小化到托盘后继续播放，界面释放省内存；托盘图标可开窗口、切歌、退出。"
        } else {
            "这个平台还没有托盘，关闭窗口即退出。"
        },
        None,
    )
    .child(supported.then(|| {
        zeo_ui_components::radio::group_cards()
            .child(option(
                CloseAction::Ask,
                "每次询问",
                "可勾「记住选择」不再询问",
            ))
            .child(option(CloseAction::Tray, "最小化到托盘", "继续播放"))
            .child(option(CloseAction::Quit, "退出", "停止播放并退出"))
    }))
    .child(pref_row(
        "记住窗口大小和位置",
        "关闭后窗口大小与位置不再保存。",
        move || s.library.with(|l| l.config.remember_frame),
        zeo_ui_store::set_remember_frame,
    ))
}
