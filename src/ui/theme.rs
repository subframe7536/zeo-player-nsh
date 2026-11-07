//! 颜色与尺寸：全部取自主题（`theme.json`），全屏播放页固定深色。
//! 强调色（`primary`）可被「主题色」设置覆盖：预设色或当前曲目封面
//! 的主色调（`store::accent` 信号，读取即订阅）。

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::config::ThemeColor;

/// 侧栏宽度。
pub const SIDEBAR: f32 = 232.0;
/// 侧栏收成图标栏后的宽度：图标列中心在 30 pt（10 外边距 + 10 行内
/// 边距 + 半个 20 pt 图标格），栏宽定 60 正好让整列居中，图标不位移。
pub const SIDEBAR_RAIL: f32 = 60.0;
/// 窗口窄于它时侧栏收成只有图标的栏（留出曲目表的最小可用宽度）。
const SIDEBAR_COLLAPSE: f32 = 900.0;

/// 窄窗口的图标栏模式（读取即订阅窗口大小）。
pub fn sidebar_collapsed() -> bool {
    window().width.0 < SIDEBAR_COLLAPSE
}

/// 侧栏当前占的宽度（内容区尺寸要用它而不是 [`SIDEBAR`]。侧栏自身的
/// 宽度有收放动画，读的是目标态）。
pub fn sidebar_width() -> f32 {
    if sidebar_collapsed() {
        SIDEBAR_RAIL
    } else {
        SIDEBAR
    }
}
/// 自绘标题栏高度（交通灯居中其中）。
pub const TITLE_BAR: f32 = 40.0;
/// 底部播放条高度（含外边距），列表据此留出底部空间。
pub const BAR_SPACE: f32 = 104.0;

/// 主体界面底色：同侧栏一样的饱和加值，但提亮度成粉彩（更亮的有色底）。
pub fn bg() -> AdaptiveColor {
    tinted(theme().background, 0.05, 0.04)
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
/// 凹陷控件的底（滑块轨道）：明暗两态都比 `muted` 离底色远一档。
pub fn input() -> AdaptiveColor {
    theme().input
}
/// 主色：「主题色」为强调色 / 跟随封面时返回覆盖色，否则主题默认（黑白）。
/// 在绑定 / `move ||` 里调用才会订阅覆盖信号。
pub fn primary() -> AdaptiveColor {
    super::store::accent().unwrap_or_else(|| theme().primary)
}
/// 主色上的文字 / 图标：有覆盖色时统一近白（预设与封面色都压得够深），
/// 否则主题值。
pub fn primary_fg() -> AdaptiveColor {
    if super::store::accent().is_some() {
        AdaptiveColor::rgb(0xffffff, 0xffffff)
    } else {
        theme().primary_foreground
    }
}
/// 预设强调色（`Neutral` / `Cover` 为 `None`）。
pub fn preset_accent(color: ThemeColor) -> Option<AdaptiveColor> {
    Some(match color {
        ThemeColor::Neutral | ThemeColor::Cover => return None,
        ThemeColor::Red => AdaptiveColor::rgb(0xe5263f, 0xff4d63),
        ThemeColor::Orange => AdaptiveColor::rgb(0xea580c, 0xfb923c),
        ThemeColor::Green => AdaptiveColor::rgb(0x16a34a, 0x4ade80),
        ThemeColor::Blue => AdaptiveColor::rgb(0x2563eb, 0x60a5fa),
        ThemeColor::Purple => AdaptiveColor::rgb(0x9333ea, 0xc084fc),
    })
}
/// 封面主色调（色相圈、饱和度）→ 浅 / 深色各一份强调色：饱和度与亮度
/// 压进可读范围，避免荧光色或太浅看不清。
pub fn accent_of(hue: f32, saturation: f32) -> AdaptiveColor {
    let s = saturation.clamp(0.45, 0.85);
    AdaptiveColor::new(hsla(hue, s, 0.42, 1.0), hsla(hue, s, 0.62, 1.0))
}
/// 「跟随封面」时向封面色相轻调的底色：保留各模式的透明，只加封面色
/// 的饱和（`store::tint` 信号，读取即订阅）。`sat_add` 是额外饱和度
///（0..1），`light` 是亮度加减（非 0 时夹在 0.04..0.96，让白底变成
/// 仍有色的粉彩）。
fn tinted(base: AdaptiveColor, sat_add: f32, light: f32) -> AdaptiveColor {
    let Some((h, s)) = super::store::tint() else {
        return base;
    };
    base.map(|c| {
        let l = if light == 0.0 {
            c.l
        } else {
            (c.l + light).clamp(0.04, 0.96)
        };
        hsla(h, (s * 0.35 + sat_add).min(0.5), l, c.a)
    })
}
/// 主侧边栏与设置弹窗左栏的底色：压暗一档，比右侧内容区深（也多一点饱和）。
pub fn sidebar() -> AdaptiveColor {
    tinted(theme().sidebar, 0.05, -0.04)
}
pub fn sidebar_fg() -> AdaptiveColor {
    theme().sidebar_foreground
}
pub fn sidebar_border() -> AdaptiveColor {
    theme().sidebar_border
}
/// 浮动播放条的底色：macOS 上半透明（下面有背景模糊）；Windows 上不透明
/// 的 card 色，因为那里不做背景模糊（见 `frame::bar::player_bar`），半透明会让
/// 底下的封面直接透出来。
pub fn bar() -> AdaptiveColor {
    #[cfg(target_os = "windows")]
    return tinted(theme().card, 0.05, 0.0);
    #[cfg(not(target_os = "windows"))]
    tinted(theme().color("bar").unwrap_or(theme().card), 0.05, 0.0)
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
    let system = system.max(super::frame::caption::width());
    system
}
