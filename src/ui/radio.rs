//! 单选：nsh-base 的 `radio_group` / `radio`（键盘激活、单选角色与选中
//! 状态）加上 shadcn/ui RadioGroup 的外观：16 pt 圆形指示器（`border-input
//! shadow-xs`，选中时主色描边 + 8 pt 主色圆点）、与标签间距 12 pt、标签
//! `text-sm font-medium`。选项卡片版本同 shadcn 的 choice card（`rounded-lg
//! border p-3`，选中时 `border-primary bg-primary/5`）。

use nsh_base::{radio, radio_group, RadioElement, RadioGroupElement};
use nsh_core::element::ComponentBuilder;
use nsh_core::prelude::*;

use super::theme::*;

/// 选项组（横排、可换行）。
pub fn group_row() -> RadioGroupElement {
    radio_group().flex_row().flex_wrap().gap_x(px(24.0)).gap_y(px(12.0))
}

/// 选项组（卡片横排，可换行）。
pub fn group_cards() -> RadioGroupElement {
    radio_group().flex_row().flex_wrap().gap(px(12.0))
}

/// 圆形指示器。
fn indicator(on: impl Fn() -> bool + Copy + 'static) -> View {
    let t = nsh_base::theme();
    let (input, primary) = (t.input, t.primary);
    view()
        .size(px(16.0))
        .flex_shrink_0()
        .rounded_full()
        .border_1()
        .border_color(move || if on() { primary } else { input })
        .shadow_xs()
        .items_center()
        .justify_center()
        .transition_colors()
        .duration(150.0)
        .child(view().size(px(8.0)).rounded_full().bg(primary).opacity(move || if on() { 1.0_f32 } else { 0.0 }))
}

fn base(
    on: impl Fn() -> bool + Copy + 'static,
    label: &'static str,
    pick: impl Fn() + 'static,
) -> RadioElement {
    let ring = nsh_base::theme().ring;
    radio()
        .checked(on)
        .accessibility_label(label)
        .on_change(move |_: &bool| pick())
        .focus(move |st| st.ring(px(3.0)).ring_color(ring.opacity(0.5)))
        .disabled_style(|st| st.opacity(0.5))
}

/// 一个选项：指示器 + 标签。
pub fn item(on: impl Fn() -> bool + Copy + 'static, label: &'static str, pick: impl Fn() + 'static) -> RadioElement {
    base(on, label, pick)
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .rounded(px(4.0))
        .child(indicator(on))
        .child(text(label).text_size(px(14.0)).font_weight(FontWeight::MEDIUM).text_color(fg()))
}

/// 选项卡片：指示器在左上，右侧标题 + 说明。
pub fn card(
    on: impl Fn() -> bool + Copy + 'static,
    label: &'static str,
    hint: &'static str,
    pick: impl Fn() + 'static,
) -> RadioElement {
    let t = nsh_base::theme();
    let (border, primary) = (t.border, t.primary);
    base(on, label, pick)
        .flex_1()
        .min_w(px(160.0))
        .flex_row()
        .items_start()
        .gap(px(12.0))
        .p(px(12.0))
        .rounded(px(RADIUS_LG))
        .border_1()
        .border_color(move || if on() { primary } else { border })
        .bg(move || if on() { primary.opacity(0.05) } else { AdaptiveColor::TRANSPARENT })
        .hover(|st| st.bg(hover()))
        .transition_colors()
        .duration(150.0)
        .child(indicator(on))
        .child(
            view()
                .flex_1()
                .gap(px(6.0))
                .child(text(label).text_size(px(14.0)).line_height(px(16.0)).font_weight(FontWeight::MEDIUM).text_color(fg()))
                .child(text(hint).text_size(px(14.0)).text_color(muted_fg())),
        )
}

/// shadcn 的 `rounded-lg`（`--radius` 0.625rem）。
const RADIUS_LG: f32 = 10.0;
