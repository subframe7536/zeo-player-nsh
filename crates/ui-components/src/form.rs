//! 表单与弹窗的 shadcn/ui 外观：Input、Select 触发器、Dialog 内容、Tabs
//! 分段。行为来自 nsh-base 的 `Input` 与 `tabs` / `tab` /
//! `dialog`；尺寸、间距、圆角同 shadcn，颜色取自主题（浅色 / 深色）。

use nsh_base::{
    Input, SwitchElement, TabElement, TabsElement, input, switch as base_switch, tab, tabs,
};
use nsh_core::prelude::*;

use super::button::RADIUS_MD;
use super::icons::{Icon, icon};
use zeo_ui_store as store;
use zeo_ui_store::theme::*;

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
        .px(px(8.0))
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

/// shadcn Switch：`w-[42px] h-[24px]` 轨道（`rounded-full bg-muted border border-border`，
/// 开启时 `bg-primary border-primary`），`size-[18px]` 白色滑块（`rounded-full shadow-sm`，
/// 开启时向右平移 18 pt）。
pub fn switch(
    checked: impl Fn() -> bool + 'static,
    label: impl Into<SharedString>,
    on_change: impl Fn(bool) + 'static,
) -> SwitchElement {
    base_switch()
        .checked(checked)
        .accessibility_label(label.into())
        .on_change(move |next: &bool| on_change(*next))
        .track_style(|st| {
            st.w(px(42.0))
                .h(px(24.0))
                .p(px(2.0))
                .rounded_full()
                .bg(muted())
                .border_1()
                .border_color(border())
                .transition_colors()
                .duration(150.0)
                .checked(|st| st.bg(primary()).border_color(primary()))
        })
        .thumb_style(|st| {
            st.size(px(18.0))
                .rounded_full()
                .bg(AdaptiveColor::rgb(0xffffff, 0xffffff))
                .shadow_sm()
                .transition_transform()
                .duration(150.0)
                .checked(|st| st.translate_x(px(18.0)))
        })
}

/// shadcn SelectTrigger（`size="sm"`）：`h-8 rounded-md border border-input
/// px-3 text-sm shadow-xs`，右侧 16 pt chevron 半透明。`width` 不大于 0
/// 时随父容器伸缩（最小 80）；值为空时显示浅色的 `placeholder`。
/// `glyph` 是当前值的前置图标（可无，随 `current` 变化）。
/// `width` 不大于 0 的伸缩要加在 select 元素上：`nsh_base::select` 的
/// 根是列容器，触发器在其内被横向拉伸铺满，`flex_1` 会在纵轴上把
/// h-8 压没（flex-basis 0）。
pub fn select_trigger(
    current: impl Fn() -> String + Copy + 'static,
    glyph: impl Fn() -> Option<Icon> + Copy + 'static,
    label: &'static str,
    width: f32,
    placeholder: &'static str,
) -> View {
    select_shell(label, width)
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

/// 触发器外壳的盒体（`h-8 rounded-md border border-input px-3
/// shadow-xs`，悬停 `bg-accent`，角色与无障碍标签）；宽 / 伸缩由
/// 调用方按所在容器决定。
fn shell_box(label: &'static str) -> View {
    let t = nsh_base::theme();
    let (input_c, card_c) = (t.input, t.card);
    view()
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
        .label(label)
}

/// 触发器外壳，给 `nsh_base::select` 内部的触发器用：
/// `width` 不大于 0 时只保最小宽——select 的根是列容器，横向已被
/// 拉伸铺满，行方向的伸缩由调用方加在 select 元素上（列里 `flex_1`
/// 长在纵轴，会把触发器高度压没）。
fn select_shell(label: &'static str, width: f32) -> View {
    let shell = shell_box(label);
    if width > 0.0 {
        shell.w(px(width))
    } else {
        shell.min_w(px(80.0))
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

/// shadcn TabsList：`h-9 rounded-lg bg-muted p-[3px]`。选中的凸起片
/// （底色、边框、`shadow-sm`）画在列表的 indicator 上：它在各 tab 底下、
/// 贴到选中项并原生弹簧跟过去，上下收进列表的 3pt 内边距。浅色用 `card`
/// （白片落在 muted 列表上，且不随封面染色——染过色的 background 与
/// muted 几乎同亮会分不出选中；深色里 card 比列表暗，改用亮一档的
/// `input` 表面）。
pub fn tab_list() -> TabsElement {
    let t = nsh_base::theme();
    let active_bg = AdaptiveColor {
        light: t.card.light,
        dark: t.input.dark,
    };
    tabs()
        .flex_row()
        .h(px(36.0))
        .p(px(3.0))
        .rounded(px(RADIUS_LG))
        .bg(muted())
        .indicator_style(|s| {
            s.top(px(3.0))
                .bottom(px(3.0))
                .rounded(px(RADIUS_MD))
                .border_1()
                .border_color(border())
                .bg(active_bg)
                .shadow_sm()
        })
}

/// shadcn TabsTrigger：`rounded-md px-2 py-1 text-sm font-medium`。选中态
/// 的凸起底 / 边框 / 阴影由 [`tab_list`] 的 indicator 画，tab 自己只管
/// 内容与文字色（选中变深）。`glyph` 为前置图标（可无）；图标与文字
/// 颜色一样。
pub fn tab_trigger(
    on: impl Fn() -> bool + Copy + 'static,
    glyph: Option<Icon>,
    label: &'static str,
    press: impl Fn() + 'static,
) -> TabElement {
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
        .transition_colors()
        .duration(150.0)
        .child(glyph.map(|g| icon(g, 14.0, move || if on() { fg() } else { muted_fg() })))
        .child(
            text(label)
                .text_size(px(14.0))
                .line_height(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if on() { fg() } else { muted_fg() }),
        )
}

/// Select 弹层的 cover 对齐重采脉冲（上游 0.4.0 的限制，见
/// upstream.md）：弹层把选中行盖在触发器上的对齐只在 cover 行
/// 首次布局时采样一次，缓存只认弹层尺寸与 cover 在父级的位置；而虚拟
/// 列表的 scrollToIndex 用估算行高分几帧收敛——采样早于收敛时，滚动
/// 到位后弹层仍停在错位处。`on_open` 后让弹层宽度在 ±1pt 间抖动几帧：
/// 弹层宽绑定触发器实测宽（listbox 的 `.w(width)`），尺寸一变就重采，
/// 最后一次落在收敛后的滚动位置上。1pt 的抖动不可见。
pub fn cover_realign(pulse: Signal<f32>) {
    spawn(async move {
        for i in 0..12u8 {
            next_frame().await;
            pulse.set(f32::from(i % 2));
        }
        pulse.set(0.0);
    });
}

/// 弹性 Select（触发器宽由 select 根的拉伸决定，自身没有 `.w`）的脉
/// 冲通道：平时 `min_w` 松开随伸缩；脉冲时把根的 `min_w` 锁到「未脉冲
/// 时记下的实测宽 +1」，强制整行 +1pt——触发器实测宽跟着变，弹层宽
/// 也就变了。返回加在 select 元素上的 `min_w` 与加在触发器上的布局
/// 监听（元素间互不覆盖，`select` 自己也在触发器上记宽）。
pub fn cover_realign_flex(
    pulse: Signal<f32>,
) -> (
    impl Fn() -> Length + Copy + 'static,
    impl Fn(&Bounds<Points>) + Copy + 'static,
) {
    let held = signal(0.0f32);
    let min_w = move || {
        if pulse.get() > 0.0 {
            Length::from(px(held.get() + 1.0))
        } else {
            Length::from(px(0.0))
        }
    };
    let layout = move |frame: &Bounds<Points>| {
        // 脉冲中不更新：held 要保住未脉冲的宽，脉冲才有基准。
        if pulse.get_untracked() == 0.0 {
            held.set(frame.size.width.0);
        }
    };
    (min_w, layout)
}

/// 下拉项的描述：文字、回调，可选分组小标题与前置图标。
pub struct SelectOption {
    pub heading: Option<&'static str>,
    pub glyph: Option<Icon>,
    pub label: &'static str,
    pub pick: Box<dyn Fn()>,
}

impl SelectOption {
    pub fn new(label: &'static str, pick: impl Fn() + 'static) -> Self {
        Self {
            heading: None,
            glyph: None,
            label,
            pick: Box::new(pick),
        }
    }

    pub fn with_glyph(mut self, glyph: Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }

    pub fn with_heading(mut self, heading: &'static str) -> Self {
        self.heading = Some(heading);
        self
    }
}

impl From<(Option<&'static str>, &'static str, Box<dyn Fn()>)> for SelectOption {
    fn from((heading, label, pick): (Option<&'static str>, &'static str, Box<dyn Fn()>)) -> Self {
        Self {
            heading,
            glyph: None,
            label,
            pick,
        }
    }
}

impl From<(Icon, &'static str, Box<dyn Fn()>)> for SelectOption {
    fn from((glyph, label, pick): (Icon, &'static str, Box<dyn Fn()>)) -> Self {
        Self {
            heading: None,
            glyph: Some(glyph),
            label,
            pick,
        }
    }
}

/// 下拉选择：nsh-base `select`（键盘导航、typeahead、无障碍、虚拟化列表），
/// 触发器与弹层仍是 shadcn 外观；`options` 可带分组小标题与图标。
#[allow(clippy::type_complexity)]
pub fn select(
    current: impl Fn() -> String + Copy + 'static,
    label: &'static str,
    options: Vec<SelectOption>,
    width: f32,
) -> AnyElement {
    select_with_trigger(current, label, options, width, None)
}

/// 支持自定义触发器的下拉选择：触发器自定义（为 `None` 时回落默认 `select_trigger`）；
/// 弹层外观、间距、键盘焦点由全局统一收敛。
#[allow(clippy::type_complexity)]
pub fn select_with_trigger(
    current: impl Fn() -> String + Copy + 'static,
    label: &'static str,
    options: Vec<SelectOption>,
    width: f32,
    custom_trigger: Option<View>,
) -> AnyElement {
    let pulse = signal(0.0f32);
    let mut flex_min_w = None;
    let trigger = if let Some(trig) = custom_trigger {
        if width > 0.0 {
            trig.w(move || px(width + pulse.get()))
        } else {
            let (min_w, layout) = cover_realign_flex(pulse);
            flex_min_w = Some(min_w);
            trig.on_layout(layout)
        }
    } else {
        let mut trig = select_trigger(current, || None, label, width, "");
        if width > 0.0 {
            trig = trig.w(move || px(width + pulse.get()));
        } else {
            let (min_w, layout) = cover_realign_flex(pulse);
            trig = trig.on_layout(layout);
            flex_min_w = Some(min_w);
        }
        trig
    };

    let gate = store::menu_control();
    let has_glyph = options.iter().any(|o| o.glyph.is_some());

    let mut items = Vec::with_capacity(options.len());
    let mut labels = Vec::with_capacity(options.len());
    let mut glyphs = Vec::with_capacity(options.len());
    let mut picks = Vec::with_capacity(options.len());
    for (i, opt) in options.into_iter().enumerate() {
        let mut item = nsh_base::SelectItem::new(i.to_string(), opt.label);
        if let Some(h) = opt.heading {
            item = item.group(h);
        }
        items.push(item);
        labels.push(opt.label);
        glyphs.push(opt.glyph);
        picks.push(opt.pick);
    }
    let value_labels = labels.clone();
    let value = move || {
        let cur = current();
        nsh_base::SelectValue::Controlled(
            value_labels
                .iter()
                .position(|l| *l == cur)
                .map(|i| i.to_string().into()),
        )
    };
    let mut select_el = nsh_base::select()
        .items(items)
        .value(value)
        .accessibility_label(label)
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(4.0))
        .margin(px(8.0))
        .open(gate.open())
        .on_open_change(gate.select_change())
        .on_open(move || cover_realign(pulse))
        .on_change(move |v: &Option<SharedString>| {
            if let Some(v) = v
                && let Ok(i) = v.parse::<usize>()
                && let Some(pick) = picks.get(i)
            {
                pick()
            }
        })
        .popup_style(move |st| {
            let st = if width > 0.0 { st } else { st.min_w(px(120.0)) };
            st.p(px(4.0))
                .rounded(px(8.0))
                .overflow_hidden()
                .border_1()
                .border_color(border())
                .bg(nsh_base::theme().popover)
                .shadow_lg()
        })
        .item_class("focus:bg-hover")
        .item_style(|st| {
            st.px(px(8.0))
                .py(px(5.0))
                .rounded(px(5.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(fg())
        })
        .group_style(|st| {
            st.text_size(px(11.0))
                .line_height(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(faint())
                .px(px(8.0))
                .pt(px(6.0))
                .pb(px(2.0))
        })
        .trigger(trigger);

    if has_glyph {
        let render_labels = labels;
        select_el = select_el.render_item(move |e: &nsh_base::SelectEntry| {
            let glyph = glyphs.get(e.index as usize).copied().flatten();
            let label = render_labels
                .get(e.index as usize)
                .copied()
                .unwrap_or_default();
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(glyph.map(|g| icon(g, 14.0, muted_fg)))
                .child(
                    text(label)
                        .flex_1()
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(fg()),
                )
        });
    }

    if width > 0.0 {
        select_el.into_element()
    } else {
        select_el
            .flex_1()
            .min_w(flex_min_w.expect("弹性分支给了 min_w"))
            .into_element()
    }
}
