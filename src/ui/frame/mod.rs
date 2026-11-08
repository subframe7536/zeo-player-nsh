//! 应用框架：路由页（`views`）内容区之外常驻的那一圈——侧栏、标题栏、
//! 播放条、队列弹层与 Windows 窗口按钮。

pub(super) mod bar;
#[cfg(target_os = "windows")]
pub(super) mod caption;
pub(super) mod queue;
pub(super) mod sidebar;
pub(super) mod title_bar;

pub(crate) use sidebar::{adaptive_row, label_fade, sidebar};
pub(crate) use title_bar::title_bar;
