//! ZeoPlayer：本地优先的桌面音乐播放器，纯 Rust NSH 应用。
//! 引擎：lofty 标签、symphonia 解码、cpal 输出、playwire 系统媒体会话、
//! LRC 解析、波形与多根曲库；界面在 [`ui`]，根组件为 [`App`]。

use nsh_core::prelude::*;

pub mod config;
mod db;
#[cfg(test)]
mod e2e;
pub mod engine;
mod metadata;
mod playlists;
mod provider;
mod single_instance;
mod ui;
mod window;

pub use ui::App;

/// The crate's UI set (`nsh.json` `"rust"`).
#[native_module("zeo-player/native")]
fn native(registry: &mut Registry) {
    // 打包 runner 在 `nsh_windows_app::main()` / `nsh_macos_app::main()`
    // 之前调这里（`set_ui_sets` 的参数）：第二份实例在这个点退出，连
    // NSH 的窗口都不会创建，不会闪一下。
    single_instance::enforce();
    registry.register::<App>();
}
