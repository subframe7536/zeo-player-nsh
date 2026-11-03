//! 设置 →「外观」分类的各节。

use std::rc::Rc;

use nsh_core::prelude::*;

use super::super::super::components::icons::{Icon, icon, icon_dyn};
use super::super::super::store::{Same, set_theme_color, store};
use super::super::super::theme::*;
use super::setting_section;
use crate::config::ThemeColor;

/// 外观：跟随系统 / 固定浅色 / 固定深色（同一行右侧的下拉）。
pub(super) fn appearance_section() -> View {
    setting_section(
        "外观",
        "跟随系统的浅色 / 深色外观，或为界面固定一种。",
        Some(appearance_select()),
    )
}
/// 「外观」的下拉：触发器显示当前值，菜单列三个带图标的选项。
fn appearance_select() -> AnyElement {
    use crate::config::Appearance;
    use nsh_base::menu;
    let s = store();
    const OPTIONS: &[(Appearance, Icon, &str)] = &[
        (Appearance::System, Icon::Monitor, "系统"),
        (Appearance::Light, Icon::Sun, "浅色"),
        (Appearance::Dark, Icon::Moon, "深色"),
    ];
    let current_mode = move || {
        OPTIONS
            .iter()
            .find(|(m, _, _)| *m == s.library.with(|l| l.config.appearance))
            .copied()
    };
    let current = move || current_mode().map_or("系统", |(_, _, l)| l).to_owned();
    let trigger = super::super::super::components::form::select_trigger(
        current,
        move || current_mode().map(|(_, g, _)| g),
        "外观",
        110.0,
        "",
    );
    let gate = super::super::super::store::menu_control();
    let mut menu = menu()
        .offset(px(4.0))
        .margin(px(8.0))
        .open(gate.open())
        .on_open_change(gate.menu_change())
        .popup_style(|st| {
            st.min_w(px(140.0))
                .p(px(4.0))
                .rounded(px(10.0))
                .overflow_hidden()
                .border_1()
                .border_color(border())
                .bg(nsh_base::theme().popover)
                .shadow_lg()
        })
        .trigger(trigger);
    for &(mode, glyph, label) in OPTIONS {
        menu = menu.child(super::super::item(label, Some(glyph), move || {
            super::super::super::store::set_appearance(mode)
        }));
    }
    menu.into_element()
}
/// 主题色：一排预设色 + 跟随当前曲目封面（强调色，立即生效）。
pub(super) fn accent_section() -> View {
    const OPTIONS: &[(ThemeColor, &str)] = &[
        (ThemeColor::Neutral, "黑白"),
        (ThemeColor::Red, "红"),
        (ThemeColor::Orange, "橙"),
        (ThemeColor::Green, "绿"),
        (ThemeColor::Blue, "蓝"),
        (ThemeColor::Purple, "紫"),
        (ThemeColor::Cover, "封面"),
    ];
    let mut swatches = view().flex_row().flex_wrap().gap(px(8.0));
    for &(mode, label) in OPTIONS {
        swatches = swatches.child(accent_swatch(mode, label));
    }
    setting_section(
        "主题色",
        "按钮与选中态的强调色：默认跟随当前曲目封面（并轻调界面底色），也可钉住黑白或预设色。",
        Some(swatches.into_element()),
    )
}
/// 主题色色块：圆形预设色，选中时显示对勾。「封面」块未生效时为灰底
/// 音符，生效后实时显示当前封面的主色调。
fn accent_swatch(mode: ThemeColor, label: &'static str) -> nsh_base::ButtonElement {
    use super::super::super::store::accent;
    let preset = preset_accent(mode);
    let on = move || store().library.with(|l| l.config.theme_color) == mode;
    nsh_base::button()
        .size(px(24.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .bg(move || match mode {
            ThemeColor::Neutral => nsh_base::theme().primary,
            // 封面色：只在本选项生效时实时显示，否则灰底。
            ThemeColor::Cover => accent().filter(|_| on()).unwrap_or_else(muted),
            _ => preset.unwrap(),
        })
        .transition_colors()
        .duration(300.0)
        .accessibility_label(SharedString::from(format!("主题色：{label}")))
        .selected(on)
        .hover(|st| st.opacity(0.85))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| set_theme_color(mode))
        .child(
            icon_dyn(
                move || {
                    if on() { Icon::Check } else { Icon::Note }
                },
                12.0,
                move || {
                    if !on() {
                        fg()
                    } else if mode == ThemeColor::Neutral {
                        nsh_base::theme().primary_foreground
                    } else {
                        AdaptiveColor::rgb(0xffffff, 0xffffff)
                    }
                },
            )
            .opacity(move || (on() || mode == ThemeColor::Cover) as i32 as f32),
        )
}
/// 字体：系统字体或任一已安装字体（同一行右侧的下拉，带搜索与逐字体预览）。
/// 字体列表懒加载：打开弹层才读取已安装字体；关闭或离开设置页时整块销毁
/// （列表数据、读取任务、各行的字体预览一起释放）。
pub(super) fn font_section() -> View {
    setting_section(
        "字体",
        "整个界面（包括歌词）换用所选字体，立即生效。",
        Some(font_select()),
    )
}
/// 「字体」的下拉：触发器用当前字体写自己的名字；弹层里是搜索框 +
/// 「系统字体」+ 已安装字体列表。`open` 由这里持有（选一个后关上）。
fn font_select() -> AnyElement {
    use nsh_base::{PopoverEvent, popover};
    let s = store();
    let open = signal(false);
    let current = move || s.library.with(|l| l.config.font.clone());
    // 触发器是普通 view（popover 接管按压）：select_trigger 的外壳 +
    // chevron，名字用所选字体渲染，所见即所得。
    let trigger = super::super::super::components::form::trigger_shell("字体", 200.0)
        .child(
            text(move || current().unwrap_or_else(|| "系统字体".to_owned()))
                .flex_1()
                .min_w(px(0.0))
                .number_of_lines(1)
                .text_size(px(14.0))
                .line_height(px(14.0))
                .font_family(move || {
                    SharedString::from(current().unwrap_or_else(|| "system-ui".to_owned()))
                })
                .text_color(fg()),
        )
        .child(super::super::super::components::form::trigger_chevron());

    popover()
        .open(move || Some(open.get()))
        .on_open_change(move |e: &PopoverEvent| {
            if let PopoverEvent::OpenChange(next, _) = e {
                open.set(*next);
            }
        })
        .placement(Placement::Bottom)
        .align(Align::End)
        .offset(px(4.0))
        .margin(px(8.0))
        .popup_style(|st| {
            st.w(px(300.0))
                .p(px(4.0))
                .rounded(px(10.0))
                .overflow_hidden()
                .border_1()
                .border_color(border())
                .bg(nsh_base::theme().popover)
                .shadow_lg()
        })
        .trigger(trigger)
        // 面板随 open 建 / 拆：打开才枚举已安装字体，关闭整块释放。
        .child(when(move || open.get(), move || font_panel(open)))
        .into_element()
}
/// 字体选项的一行：显示名用它自己的字体渲染 + 「歌词 Aa あ」样例，
/// 选中行有主题色底与对勾。
fn font_choice(
    label: impl Fn() -> String + Copy + 'static,
    family: impl Fn() -> String + Copy + 'static,
    selected: impl Fn() -> bool + Copy + 'static,
    press: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    nsh_base::button()
        .h(px(40.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .accessibility_label(move || SharedString::from(label()))
        .selected(selected)
        .selected_style(|st| st.bg(move || primary().opacity(0.12)))
        .transition_colors()
        .duration(300.0)
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| press())
        .child(
            text(move || format!("{} · 歌词 Aa あ", label()))
                .flex_1()
                .min_w(px(0.0))
                .number_of_lines(1)
                .font_family(move || SharedString::from(family()))
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(fg()),
        )
        .child(when(selected, || icon(Icon::Check, 14.0, primary)))
}
/// 「字体」弹层：搜索框、「系统字体」、已安装字体（懒加载：打开才读，
/// 关闭整块销毁）。
fn font_panel(open: Signal<bool>) -> View {
    let s = store();
    let query = signal(String::new());
    let current = move || s.library.with(|l| l.config.font.clone());
    // None：还在读取。任务属于这一块，块销毁时一起取消。
    let installed = signal(None::<Same<Vec<String>>>);
    spawn(async move {
        if let Ok(list) = blocking(super::super::super::fonts::installed_families).await {
            installed.set(Some(Same(std::sync::Arc::new(list))));
        }
    });
    let loading = move || installed.with(Option::is_none);

    // 按搜索词过滤。
    let rows = computed(move || {
        let q = query.get().trim().to_lowercase();
        let out: Vec<String> = installed.with(|list| {
            list.iter()
                .flat_map(|list| list.iter())
                .filter(|f| q.is_empty() || f.to_lowercase().contains(&q))
                .cloned()
                .collect()
        });
        Rc::new(out)
    });
    let count = computed(move || rows.with(|r| r.len()));

    let list = virtual_list(count, move |at| {
        let family =
            computed(move || rows.with(|r| r.get(at.index()).cloned().unwrap_or_default()));
        let family = move || family.get();
        let selected = move || current().as_deref() == Some(family().as_str());
        font_choice(family, family, selected, move || {
            super::super::super::store::set_font(Some(family()));
            open.set(false);
        })
    })
    .estimated_item_size(px(40.0))
    .empty(
        view().h(px(64.0)).items_center().justify_center().child(
            text(move || {
                if loading() {
                    "正在读取字体…"
                } else {
                    "没有匹配的字体"
                }
            })
            .text_size(px(13.0))
            .line_height(px(13.0))
            .text_color(muted_fg()),
        ),
    )
    .flex_1();

    view()
        .gap(px(4.0))
        .child(
            super::super::super::components::form::input_field(true)
                .w_full()
                .value(move || query.get())
                .placeholder("搜索已安装的字体")
                .on_change(move |text: &str| query.set(text.to_owned())),
        )
        .child(font_choice(
            || "系统字体".to_owned(),
            || "system-ui".to_owned(),
            move || current().is_none(),
            move || {
                super::super::super::store::set_font(None);
                open.set(false);
            },
        ))
        .child(view().h(px(1.0)).bg(border()))
        .child(view().h(px(216.0)).child(list))
}
