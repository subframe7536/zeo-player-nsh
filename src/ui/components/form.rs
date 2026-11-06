//! 表单与弹窗的 shadcn/ui 外观：Input、Select 触发器、Dialog 内容、Tabs
//! 分段。行为来自 nsh-core 的 `input` 与 nsh-base 的 `tabs` / `tab` /
//! `dialog`；尺寸、间距、圆角同 shadcn，颜色取自主题（浅色 / 深色）。

use nsh_base::{TabElement, TabsElement, tab, tabs};
use nsh_core::element::{Input, input};
use nsh_core::prelude::*;

use super::super::store;
use super::super::theme::*;
use super::button::RADIUS_MD;
use super::icons::{Icon, icon};

/// shadcn 的 `rounded-lg`。
pub const RADIUS_LG: f32 = 10.0;

/// shadcn Input：`h-9 rounded-md border border-input px-3 text-sm
/// shadow-xs`，聚焦时 `border-ring ring-3 ring-ring/50`。底色用 `card`
/// 而不是 shadcn 的 `bg-transparent` / `dark:bg-input/30`：面板底色会随
/// 封面染色，透明或半透明的底会把染色带进输入框。`small` 为 `h-8`
/// （放在一行条件里）。
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
        .bg(t.card)
        .focus(move |st| {
            st.border_color(ring)
                .ring(px(3.0))
                .ring_color(ring.opacity(0.5))
        })
}

/// shadcn SelectTrigger（`size="sm"`）：`h-8 rounded-md border border-input
/// px-3 text-sm shadow-xs`，右侧 16 pt chevron 半透明。`width` 不大于 0
/// 时随父容器伸缩（最小 80）；值为空时显示浅色的 `placeholder`。
/// `glyph` 是当前值的前置图标（可无，随 `current` 变化）。
pub fn select_trigger(
    current: impl Fn() -> String + Copy + 'static,
    glyph: impl Fn() -> Option<Icon> + Copy + 'static,
    label: &'static str,
    width: f32,
    placeholder: &'static str,
) -> View {
    trigger_shell(label, width)
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(dynamic(move || {
                    glyph().map(|g| icon(g, 14.0, fg).into_element())
                }))
                .child(
                    text(move || {
                        let value = current();
                        if value.is_empty() {
                            placeholder.to_owned()
                        } else {
                            value
                        }
                    })
                    .flex_1()
                    .min_w(px(0.0))
                    .number_of_lines(1)
                    .text_size(px(14.0))
                    .line_height(px(14.0))
                    .text_color(move || if current().is_empty() { faint() } else { fg() }),
                ),
        )
        .child(trigger_chevron())
}

/// SelectTrigger 的外壳（`h-8 rounded-md border border-input px-3
/// shadow-xs`，悬停 `bg-accent`），内容（值、chevron 等）由调用方给。
/// 底色同 [`input_field`] 用 `card`，不随封面染色。`width` 约定同
/// [`select_trigger`]。popover 的触发器必须是普通 view 时用这个，保持
/// 和下拉触发器同一外观。
pub fn trigger_shell(label: &'static str, width: f32) -> View {
    let t = nsh_base::theme();
    let (input_c, card_c) = (t.input, t.card);
    let trigger = view()
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
        .bg(card_c)
        .hover(move |st| st.bg(hover()))
        .role(AccessibilityRole::Button)
        .label(label);
    if width > 0.0 {
        trigger.w(px(width))
    } else {
        trigger.flex_1().min_w(px(80.0))
    }
}

/// SelectTrigger 右侧的 chevron（16 pt，半透明）。
pub fn trigger_chevron() -> View {
    view()
        .opacity(0.5)
        .child(icon(Icon::ChevronDown, 16.0, muted_fg))
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
        .bg(move || bg())
        .transition_colors()
        .duration(300.0)
        .shadow_lg()
        .on_press(|| {})
}

/// shadcn DialogHeader：标题 `text-lg font-semibold leading-none`，说明
/// `text-sm text-muted-foreground`，间距 8。
pub fn dialog_header(title: &'static str, description: Option<&'static str>) -> View {
    view()
        .gap(px(8.0))
        .child(
            text(title)
                .text_size(px(18.0))
                .line_height(px(18.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg()),
        )
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
    tabs()
        .flex_row()
        .h(px(36.0))
        .p(px(3.0))
        .rounded(px(RADIUS_LG))
        .bg(muted())
}

/// shadcn TabsTrigger：`rounded-md px-2 py-1 text-sm font-medium`，选中时
/// 加底色、边框与 `shadow-sm`（浅色用 `card`：白片落在 muted 列表上，
/// 且不随封面染色——染过色的 background 与 muted 几乎同亮会分不出选中；
/// 深色里 card 比列表暗，改用亮一档的 `input` 表面）。`glyph` 为前置图标
/// （可无）；图标与文字颜色一样，选中变深。
pub fn tab_trigger(
    on: impl Fn() -> bool + Copy + 'static,
    glyph: Option<Icon>,
    label: &'static str,
    press: impl Fn() + 'static,
) -> TabElement {
    let t = nsh_base::theme();
    let active_bg = AdaptiveColor {
        light: t.card.light,
        dark: t.input.dark,
    };
    tab()
        .selected(on)
        .accessibility_label(label)
        .on_press(move |_: &()| press())
        .h_full()
        .px(px(8.0))
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(AdaptiveColor::TRANSPARENT)
        .transition_colors()
        .duration(150.0)
        .style_with(move |st| {
            if on() {
                st.bg(active_bg).border_color(border()).shadow_sm()
            } else {
                st
            }
        })
        .child(glyph.map(|g| icon(g, 14.0, move || if on() { fg() } else { muted_fg() })))
        .child(
            text(label)
                .text_size(px(14.0))
                .line_height(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if on() { fg() } else { muted_fg() }),
        )
}

/// 下拉弹层的最大高度：比右键菜单矮，弹窗里的下拉在常见窗口高度下
/// 上方或下方放得下（放不下下方时翻到上方）。
pub(crate) const SELECT_POPUP_H: f32 = 300.0;

/// 下拉选择：nsh-base `select`（键盘导航、typeahead、无障碍、虚拟化列表），
/// 触发器与弹层仍是 shadcn 外观；`options` 可带分组小标题。
#[allow(clippy::type_complexity)]
pub fn select(
    current: impl Fn() -> String + Copy + 'static,
    label: &'static str,
    options: Vec<(Option<&'static str>, &'static str, Box<dyn Fn()>)>,
    width: f32,
) -> AnyElement {
    let trigger = select_trigger(current, || None, label, width, "");
    let gate = store::menu_control();
    // 内部值用序号串：选中回调按序号回取；`value` 把当前显示文本映射回序号
    // （显示文本不在选项里——EQ 的「自定义」——时无选中行）。
    let mut items = Vec::with_capacity(options.len());
    let mut labels = Vec::with_capacity(options.len());
    let mut picks = Vec::with_capacity(options.len());
    for (i, (heading, option, pick)) in options.into_iter().enumerate() {
        let mut item = nsh_base::SelectItem::new(i.to_string(), option);
        if let Some(h) = heading {
            item = item.group(h);
        }
        items.push(item);
        labels.push(option);
        picks.push(pick);
    }
    let value = move || {
        let cur = current();
        nsh_base::SelectValue::Controlled(
            labels
                .iter()
                .position(|l| *l == cur)
                .map(|i| i.to_string().into()),
        )
    };
    nsh_base::select()
        .items(items)
        .value(value)
        .accessibility_label(label)
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(4.0))
        .margin(px(8.0))
        .open(gate.open())
        .on_open_change(gate.select_change())
        .on_change(move |v: &Option<SharedString>| {
            if let Some(v) = v
                && let Ok(i) = v.parse::<usize>()
                && let Some(pick) = picks.get(i)
            {
                pick()
            }
        })
        .popup_style(move |st| {
            // 弹层与触发器同宽（shadcn SelectContent 贴触发器宽）；
            // width≤0 的弹性触发器宽度未知，退回 140 下限。
            let st = if width > 0.0 {
                st.w(px(width))
            } else {
                st.min_w(px(140.0))
            };
            st.p(px(4.0))
                .rounded(px(10.0))
                .overflow_hidden()
                .border_1()
                .border_color(border())
                .bg(nsh_base::theme().popover)
                .shadow_lg()
        })
        .list_style(|st| st.max_h(px(SELECT_POPUP_H - 10.0)))
        // `focus:` 是高亮态（指针与键盘导航都算）；`bg-hover` 取主题色。
        .item_class("focus:bg-hover")
        .item_style(|st| {
            st.px(px(10.0))
                .py(px(6.0))
                .rounded(px(6.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(fg())
        })
        .group_style(|st| {
            st.text_size(px(11.0))
                .line_height(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(faint())
                .px(px(10.0))
                .pt(px(6.0))
                .pb(px(2.0))
        })
        .trigger(trigger)
        .into_element()
}
