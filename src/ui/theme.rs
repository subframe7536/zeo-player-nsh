//! 颜色与尺寸：全部取自主题（`theme.json`），全屏播放页固定深色。

use nsh_base::theme;
use nsh_core::prelude::*;

/// 侧栏宽度。
pub const SIDEBAR: f32 = 232.0;
/// 自绘标题栏高度（交通灯居中其中）。
pub const TITLE_BAR: f32 = 40.0;
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
pub fn primary() -> AdaptiveColor {
    theme().primary
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
/// 浮动播放条的底色：macOS 上半透明（下面有背景模糊）；Windows 上不透明
/// 的 card 色，因为那里不做背景模糊（见 `bar::player_bar`），半透明会让
/// 底下的封面直接透出来。
pub fn bar() -> AdaptiveColor {
    #[cfg(target_os = "windows")]
    return theme().card;
    #[cfg(not(target_os = "windows"))]
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

/// 「喜欢」的红色（心形的描边与填充）。
pub fn like_red() -> AdaptiveColor {
    AdaptiveColor::rgb(0xe5484d, 0xf2555a)
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

/// 右上角被窗口按钮占去的宽度（Windows 约 138 pt；macOS 为 0）。Windows 上
/// 按钮由应用画（`caption`），接管前由 NSH 画、宽度由它报告。
pub fn controls_right() -> f32 {
    let system = nsh_core::element::desktop::title_bar().right.0;
    #[cfg(target_os = "windows")]
    let system = system.max(super::caption::width());
    system
}
