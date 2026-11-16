//! 应用框架：路由页（`views`）内容区之外常驻的那一圈——侧栏、标题栏、
//! 播放条与队列弹层。

pub mod bar;
pub mod queue;
pub mod sidebar;
pub mod title_bar;

pub use sidebar::sidebar;
pub use title_bar::title_bar;
