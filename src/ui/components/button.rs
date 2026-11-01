//! 按钮：nsh-base 的 `button`（按压、禁用、选中、无障碍角色）加上 shadcn/ui
//! Button 的变体与尺寸（高度、内边距、间距、圆角、字号同 shadcn）。颜色取
//! 自主题（`theme.json`），浅色 / 深色自动切换。

use nsh_base::{ButtonElement, button};
use nsh_core::element::ComponentBuilder;
use nsh_core::prelude::*;

use super::super::theme::*;
use super::icons::{Icon, icon_dyn};

/// shadcn 的 `variant`。
#[allow(dead_code, reason = "shadcn 的完整变体集，部分暂未用到")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// 主色底（`bg-primary`）。
    Default,
    /// 次要底色（`bg-secondary`）。
    Secondary,
    /// 描边（`border bg-background shadow-xs`）。
    Outline,
    /// 无底色，悬停才有。
    Ghost,
    /// 危险操作（`bg-destructive text-white`）。
    Destructive,
}

/// shadcn 的 `size`。
#[allow(dead_code, reason = "shadcn 的完整尺寸集，部分暂未用到")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// `h-6 px-2 gap-1 text-xs`。
    Xs,
    /// `h-8 px-3 gap-1.5`。
    Sm,
    /// `h-9 px-4 gap-2`。
    Default,
    /// `h-10 px-6 gap-2`。
    Lg,
    /// `size-6`。
    IconXs,
    /// `size-8`。
    IconSm,
    /// `size-9`。
    Icon,
    /// `size-10`。
    IconLg,
}

impl Size {
    fn height(self) -> f32 {
        match self {
            Size::Xs | Size::IconXs => 24.0,
            Size::Sm | Size::IconSm => 32.0,
            Size::Default | Size::Icon => 36.0,
            Size::Lg | Size::IconLg => 40.0,
        }
    }

    fn is_icon(self) -> bool {
        matches!(
            self,
            Size::IconXs | Size::IconSm | Size::Icon | Size::IconLg
        )
    }

    /// 图标边长（shadcn 的 `[&_svg]:size-4`，`xs` 为 `size-3`）。
    pub fn glyph(self) -> f32 {
        if matches!(self, Size::Xs | Size::IconXs) {
            12.0
        } else {
            16.0
        }
    }

    fn text_size(self) -> f32 {
        if self == Size::Xs { 12.0 } else { 14.0 }
    }
}

/// 变体的文字 / 图标颜色。
pub fn foreground(variant: Variant) -> AdaptiveColor {
    let t = nsh_base::theme();
    match variant {
        Variant::Default => primary_fg(),
        Variant::Secondary => t.secondary_foreground,
        Variant::Outline | Variant::Ghost => t.foreground,
        Variant::Destructive => AdaptiveColor::rgb(0xffffff, 0xffffff),
    }
}

/// 只有外形的按钮（尺寸、底色、状态样式），内容由调用方给。
pub fn shell(variant: Variant, size: Size) -> ButtonElement {
    let t = nsh_base::theme();
    let (ring, secondary, destructive, outline) =
        (t.ring, t.secondary, t.destructive, t.border);
    let h = size.height();
    let b = button()
        .flex_row()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .h(px(h))
        .rounded(px(RADIUS_MD))
        .transition_colors()
        .duration(150.0)
        .focus(move |st| st.ring(px(3.0)).ring_color(ring.opacity(0.5)))
        .disabled_style(|st| st.opacity(0.5));
    let b = if size.is_icon() {
        b.w(px(h))
    } else {
        let (pad, gap) = match size {
            Size::Xs => (8.0, 4.0),
            Size::Sm => (12.0, 6.0),
            Size::Lg => (24.0, 8.0),
            _ => (16.0, 8.0),
        };
        b.px(px(pad)).gap(px(gap))
    };
    match variant {
        Variant::Default => b
            .bg(move || primary())
            .hover(|st| st.bg(move || primary().opacity(0.9)))
            .transition_colors()
            .duration(300.0),
        Variant::Secondary => b
            .bg(secondary)
            .hover(move |st| st.bg(secondary.opacity(0.8))),
        Variant::Outline => b
            .border_1()
            .border_color(outline)
            .bg(move || bg())
            .transition_colors()
            .duration(300.0)
            .shadow_xs()
            .hover(|st| st.bg(hover())),
        Variant::Ghost => b.hover(|st| st.bg(hover())),
        Variant::Destructive => b
            .bg(destructive)
            .hover(move |st| st.bg(destructive.opacity(0.9))),
    }
    .active(|st| st.opacity(0.8))
}

/// 文字按钮，可带前置图标（shadcn 的 `<Button><Icon />文字</Button>`）。
pub fn text_button(
    variant: Variant,
    size: Size,
    glyph: Option<Icon>,
    label: impl Into<SharedString>,
    on_press: impl Fn() + 'static,
) -> ButtonElement {
    let label: SharedString = label.into();
    let fg = move || foreground(variant);
    shell(variant, size)
        .accessibility_label(label.clone())
        .on_press(move |_: &()| on_press())
        .child(glyph.map(|g| icon_dyn(move || g, size.glyph(), fg)))
        .child(
            text(label)
                .text_size(px(size.text_size()))
                .line_height(px(size.text_size()))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || fg()),
        )
}

/// 给按钮加提示（nsh-base `tooltip`，桌面上悬停 500 ms 后出现在上方）。
/// 外观同 shadcn TooltipContent：`rounded-md bg-foreground px-3 py-1.5
/// text-xs text-background`。nsh-base 0.3.0 的组件元素没有 `.tooltip`，
/// 所以挂在外面包一层的 view 上。
pub fn tipped(content: impl IntoElement, tip: impl Into<SharedString>) -> View {
    let t = nsh_base::theme();
    let (back, front) = (t.foreground, t.background);
    view().flex_shrink_0().child(content).tooltip(
        nsh_base::tooltip()
            .offset(px(6.0))
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(RADIUS_MD))
            .bg(back)
            .child(
                text(tip.into())
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(front),
            ),
    )
}

/// 全屏播放页（深色背景上的白色图标）的 ghost 按钮：悬停为白色 8%，
/// 与其它按钮一样是 [`RADIUS_MD`] 圆角的方形；`side` 为边长。
pub fn on_dark(side: f32) -> ButtonElement {
    button()
        .size(px(side))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_MD))
        .transition_colors()
        .duration(150.0)
        .hover(|st| st.bg(white_a(0.08)))
        .active(|st| st.opacity(0.6))
        .focus(|st| st.ring(px(3.0)).ring_color(white_a(0.4)))
}

/// shadcn 的 `rounded-md`（`--radius` 0.625rem 减 2 px）。
pub const RADIUS_MD: f32 = 8.0;

/// 图标按钮（ghost，悬停才有底色）。`size` 是图标边长，按钮按它选
/// shadcn 的尺寸：≤ 12 为 `icon-xs`（24），≤ 16 为 `icon-sm`（32），更大为
/// `icon`（36）。
pub fn icon_button(
    name: impl Fn() -> Icon + 'static,
    size: f32,
    label: &'static str,
    tint: impl Fn() -> AdaptiveColor + 'static,
    on_press: impl Fn() + 'static,
) -> ButtonElement {
    let shape = if size <= 12.0 {
        Size::IconXs
    } else if size <= 16.0 {
        Size::IconSm
    } else {
        Size::Icon
    };
    shell(Variant::Ghost, shape)
        .accessibility_label(label)
        .on_press(move |_: &()| on_press())
        .child(icon_dyn(name, size, tint))
}

/// 主操作按钮（shadcn `default`）：「播放」。
pub fn pill_button(
    name: Icon,
    label: &'static str,
    on_press: impl Fn() + 'static,
) -> ButtonElement {
    text_button(Variant::Default, Size::Default, Some(name), label, on_press)
}

/// 次要按钮（shadcn `outline`）。
pub fn outline_button(
    name: Icon,
    label: &'static str,
    on_press: impl Fn() + 'static,
) -> ButtonElement {
    text_button(Variant::Outline, Size::Default, Some(name), label, on_press)
}
