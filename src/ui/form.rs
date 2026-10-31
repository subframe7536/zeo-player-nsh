//! 表单与弹窗的 shadcn/ui 外观：Input、Select 触发器、Dialog 内容、Tabs
//! 分段。行为来自 nsh-core 的 `input` 与 nsh-base 的 `tabs` / `tab` /
//! `dialog`；尺寸、间距、圆角同 shadcn，颜色取自主题（浅色 / 深色）。

use nsh_base::{tab, tabs, TabElement, TabsElement};
use nsh_core::element::{input, Input};
use nsh_core::prelude::*;

use super::button::RADIUS_MD;
use super::icons::{icon, Icon};
use super::theme::*;

/// shadcn 的 `rounded-lg`。
pub const RADIUS_LG: f32 = 10.0;

/// shadcn Input：`h-9 rounded-md border border-input bg-transparent px-3
/// text-sm shadow-xs`，聚焦时 `border-ring ring-3 ring-ring/50`；深色
/// `bg-input/30`。`small` 为 `h-8`（放在一行条件里）。
pub fn input_field(small: bool) -> Input {
    let t = nsh_base::theme();
    let (input_c, ring) = (t.input, t.ring);
    input()
        .h(px(if small { 32.0 } else { 36.0 }))
        .px(px(12.0))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(input_c)
        .shadow_xs()
        .text_size(px(14.0))
        .text_color(fg())
        .dark(move |st| st.bg(input_c.opacity(0.3)))
        .focus(move |st| st.border_color(ring).ring(px(3.0)).ring_color(ring.opacity(0.5)))
}

/// shadcn SelectTrigger（`size="sm"`）：`h-8 rounded-md border border-input
/// px-3 text-sm shadow-xs`，右侧 16 pt chevron 半透明。
pub fn select_trigger(current: impl Fn() -> String + 'static, label: &'static str, width: f32) -> View {
    let input_c = nsh_base::theme().input;
    view()
        .w(px(width))
        .h(px(32.0))
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .px(px(12.0))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(input_c)
        .shadow_xs()
        .dark(move |st| st.bg(input_c.opacity(0.3)))
        .hover(move |st| st.bg(hover()))
        .role(AccessibilityRole::Button)
        .label(label)
        .child(text(current).flex_1().number_of_lines(1).text_size(px(14.0)).text_color(fg()))
        .child(view().opacity(0.5).child(icon(Icon::ChevronDown, 16.0, muted_fg)))
}

/// shadcn DialogContent：`rounded-lg border bg-background p-6 gap-4
/// shadow-lg`，宽 `width`（shadcn 默认 `sm:max-w-lg` 即 512）。点在内容上
/// 不会穿到背后的遮罩。
pub fn dialog_content(width: f32) -> View {
    view()
        .w(px(width))
        .max_w(move || px((window().width.0 - 32.0).max(240.0)))
        .max_h(move || px((window().height.0 - 80.0).max(240.0)))
        .p(px(24.0))
        .gap(px(16.0))
        .rounded(px(RADIUS_LG))
        .border_1()
        .border_color(border())
        .bg(bg())
        .shadow_lg()
        .on_press(|| {})
}

/// shadcn DialogHeader：标题 `text-lg font-semibold leading-none`，说明
/// `text-sm text-muted-foreground`，间距 8。
pub fn dialog_header(title: &'static str, description: Option<&'static str>) -> View {
    view()
        .gap(px(8.0))
        .child(text(title).text_size(px(18.0)).line_height(px(18.0)).font_weight(FontWeight::SEMIBOLD).text_color(fg()))
        .child(description.map(|d| text(d).text_size(px(14.0)).text_color(muted_fg())))
}

/// shadcn DialogFooter：靠右横排，间距 8。
pub fn dialog_footer() -> View {
    view().flex_row().justify_end().gap(px(8.0))
}

/// 弹窗（nsh-base `dialog`，浮在一切之上；Esc 关闭）：半透明遮罩
/// （shadcn `bg-black/50`），点遮罩关闭，内容居中。
pub fn modal(content: View, on_close: impl Fn() + Copy + 'static) -> AnyElement {
    nsh_base::dialog()
        .animation(ModalAnimation::Fade)
        .on_cancel(move || on_close())
        .child(
            view()
                .absolute()
                .inset(px(0.0))
                .bg(black_a(0.5))
                .items_center()
                .justify_center()
                .on_press(move || on_close())
                .child(content),
        )
        .into_element()
}

/// shadcn TabsList：`h-9 rounded-lg bg-muted p-[3px]`。
pub fn tab_list() -> TabsElement {
    tabs().flex_row().h(px(36.0)).p(px(3.0)).rounded(px(RADIUS_LG)).bg(muted())
}

/// shadcn TabsTrigger：`rounded-md px-2 py-1 text-sm font-medium`，选中时
/// `bg-background shadow-sm`（深色时背景色本身就比 `bg-muted` 暗，不另加描边）。
pub fn tab_trigger(on: impl Fn() -> bool + Copy + 'static, label: &'static str, press: impl Fn() + 'static) -> TabElement {
    tab()
        .selected(on)
        .accessibility_label(label)
        .on_press(move |_: &()| press())
        .h_full()
        .px(px(8.0))
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(AdaptiveColor::TRANSPARENT)
        .transition_colors()
        .duration(150.0)
        .style_with(move |st| {
            if on() {
                st.bg(bg()).shadow_sm()
            } else {
                st
            }
        })
        .child(text(label).text_size(px(14.0)).font_weight(FontWeight::MEDIUM).text_color(move || if on() { fg() } else { muted_fg() }))
}
