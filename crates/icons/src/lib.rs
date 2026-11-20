//! 图标：线条图标集（Icon，Lucide，24 × 24 SVG data: URI）、应用 logo、
//! 播放 / 暂停键的形变图标与它的静态 SVG 版（托盘位图用）。
//!
//! - `glyph`：`Icon` 枚举与各图标的 SVG 内容；
//! - `render`：上色、按（图标, 尺寸, 颜色）缓存、出 `image` 元素；
//! - `logo`：应用图标（取自 `resources/icon.svg`）；
//! - `morph`：播放 / 暂停的共享顶点、`play_pause` 画布动画与 `play_pause_svg`。

mod glyph;
mod logo;
mod morph;
mod render;

pub use glyph::Icon;
pub use logo::logo;
pub use morph::{MORPH_STROKE, PAUSE_SHAPES, PLAY_SHAPES, play_pause, play_pause_svg};
pub use render::{icon, icon_dyn, icon_uri};
