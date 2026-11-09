//! 平台外壳：窗口生命周期 / 关闭行为 / 托盘 / 单实例（`single_instance`），
//! 以及文件管理器 reveal。UI 与引擎经这里的自由函数触达平台能力。

pub mod single_instance;
pub mod tray;
pub mod window;

pub use tray::{TrayAction, TrayState, take_tray_actions};

#[cfg(any(debug_assertions, test))]
pub use tray::simulate_tray;
pub use window::*;

/// 「在文件管理器中显示」的文字。
#[cfg(target_os = "windows")]
pub const REVEAL_LABEL: &str = "在资源管理器中显示";
#[cfg(not(target_os = "windows"))]
pub const REVEAL_LABEL: &str = "在访达中显示";

/// 在系统文件管理器里选中文件。
pub fn reveal(path: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
    // explorer 自己解析 `/select,"路径"`：带空格的路径须原样加引号，不能经
    // Rust 的参数转义。
    #[cfg(target_os = "windows")]
    let _ = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{path}\""))
            .spawn()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = std::process::Command::new("xdg-open")
        .arg(
            std::path::Path::new(path)
                .parent()
                .unwrap_or(std::path::Path::new("/")),
        )
        .spawn();
}
