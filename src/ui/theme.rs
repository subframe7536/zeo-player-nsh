//! 颜色与尺寸：全部取自主题（`theme.json`），全屏播放页固定深色。

use nsh_base::theme;
use nsh_core::prelude::*;

/// 侧栏宽度。
pub const SIDEBAR: f32 = 232.0;
/// 自绘标题栏高度（交通灯居中其中）。
pub const TITLE_BAR: f32 = 52.0;
/// 底部播放条高度（含外边距），列表据此留出底部空间。
pub const BAR_SPACE: f32 = 104.0;

pub fn bg() -> AdaptiveColor {
    theme().background
}
pub fn fg() -> AdaptiveColor {
    theme().foreground
}
pub fn muted() -> AdaptiveColor {
    theme().muted
}
pub fn muted_fg() -> AdaptiveColor {
    theme().muted_foreground
}
pub fn border() -> AdaptiveColor {
    theme().border
}
pub fn card() -> AdaptiveColor {
    theme().card
}
pub fn primary() -> AdaptiveColor {
    theme().primary
}
pub fn primary_fg() -> AdaptiveColor {
    theme().primary_foreground
}
pub fn sidebar() -> AdaptiveColor {
    theme().sidebar
}
pub fn sidebar_fg() -> AdaptiveColor {
    theme().sidebar_foreground
}
pub fn sidebar_accent() -> AdaptiveColor {
    theme().sidebar_accent
}
pub fn sidebar_border() -> AdaptiveColor {
    theme().sidebar_border
}
/// 浮动播放条的半透明底色。
pub fn bar() -> AdaptiveColor {
    theme().color("bar").unwrap_or(theme().card)
}
/// 最弱一级文字（列头、占位）。
pub fn faint() -> AdaptiveColor {
    theme().color("faint").unwrap_or(theme().muted_foreground)
}
/// 行的悬停底色。
pub fn hover() -> AdaptiveColor {
    theme().color("hover").unwrap_or(theme().accent)
}

/// 全屏页上的白色（`alpha` 透明度）。
pub fn white_a(alpha: f32) -> AdaptiveColor {
    AdaptiveColor::rgb(0xffffff, 0xffffff).opacity(alpha)
}

/// 全屏页上的黑色（`alpha` 透明度）。
pub fn black_a(alpha: f32) -> AdaptiveColor {
    AdaptiveColor::rgb(0x000000, 0x000000).opacity(alpha)
}

/// 页面顶部留给标题栏的高度：macOS 交通灯居中在 [`TITLE_BAR`] 里；Windows
/// 的标题栏按钮在右上角，高度由系统报告。读取即订阅（进出全屏时变化）。
pub fn top_inset() -> f32 {
    let bar = nsh_core::element::desktop::title_bar();
    bar.top.0.max(TITLE_BAR)
}

/// 右上角被系统窗口按钮占去的宽度（Windows 约 138 pt；macOS 为 0）。
pub fn controls_right() -> f32 {
    nsh_core::element::desktop::title_bar().right.0
}
