//! 设置 →「窗口」分类的各节。

use nsh_core::prelude::*;

use super::super::super::store::store;
use super::{pref_row, setting_section};

/// 关闭窗口时：每次询问 / 最小化到托盘 / 退出。
pub(super) fn window_section() -> View {
    use crate::config::CloseAction;
    let s = store();
    let option = move |action: CloseAction, label: &'static str, hint: &'static str| {
        super::super::super::components::radio::card(
            move || s.library.with(|l| l.config.close_action == action),
            label,
            hint,
            move || super::super::super::store::set_close_action(action),
        )
    };
    let supported = cfg!(any(target_os = "macos", target_os = "windows"));
    setting_section(
        "关闭窗口时",
        if supported {
            "最小化到托盘后音乐继续播放，界面释放以节省内存；托盘图标可以打开窗口、播放 / 暂停、切歌和退出。"
        } else {
            "这个平台还没有托盘，关闭窗口即退出。"
        },
        None,
    )
    .child(supported.then(|| {
        super::super::super::components::radio::group_cards()
            .child(option(CloseAction::Ask, "每次询问", "第一次关闭时选择"))
            .child(option(CloseAction::Tray, "最小化到托盘", "继续播放"))
            .child(option(CloseAction::Quit, "退出", "停止播放并退出"))
    }))
    .child(pref_row(
        "记住窗口大小和位置",
        "重新打开时恢复上次的窗口大小与位置；关掉后已存的位置丢弃。",
        move || s.library.with(|l| l.config.remember_frame),
        super::super::super::store::set_remember_frame,
    ))
}

/// 菜单栏（macOS）：托盘图标旁是否显示当前曲名。
#[cfg(target_os = "macos")]
pub(super) fn menubar_section() -> View {
    let s = store();
    setting_section(
        "菜单栏",
        "菜单栏的托盘图标是播放 / 暂停按钮：左键播放 / 暂停，右键打开菜单；图标旁可以显示当前曲目。",
        None,
    )
    .child(pref_row(
        "显示曲名",
        "在菜单栏图标旁显示当前曲名（没有标题时显示文件名，最长 30 个字符）；关掉后只留图标。",
        move || s.library.with(|l| l.config.show_tray_title),
        super::super::super::store::set_show_tray_title,
    ))
}
