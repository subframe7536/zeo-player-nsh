//! 设置 →「外观」分类的各节。

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
/// 「外观」的下拉：触发器显示当前值，nsh-base `select` 列三个带图标的选项
/// （行内容自绘；高亮 / 键盘导航 / 无障碍由上游接管）。
fn appearance_select() -> AnyElement {
    use crate::config::Appearance;
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
    nsh_base::select()
        .items(
            OPTIONS
                .iter()
                .enumerate()
                .map(|(i, (_, _, label))| nsh_base::SelectItem::new(i.to_string(), *label))
                .collect::<Vec<_>>(),
        )
        .value(move || {
            let mode = s.library.with(|l| l.config.appearance);
            nsh_base::SelectValue::Controlled(
                OPTIONS
                    .iter()
                    .position(|(m, _, _)| *m == mode)
                    .map(|i| i.to_string().into()),
            )
        })
        .accessibility_label("外观")
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(4.0))
        .margin(px(8.0))
        .open(gate.open())
        .on_open_change(gate.select_change())
        .on_change(move |v: &Option<SharedString>| {
            if let Some(v) = v
                && let Ok(i) = v.parse::<usize>()
                && let Some(&(mode, _, _)) = OPTIONS.get(i)
            {
                super::super::super::store::set_appearance(mode)
            }
        })
        .render_item(|e: &nsh_base::SelectEntry| {
            let (glyph, label) = OPTIONS
                .get(e.index as usize)
                .map(|(_, g, l)| (*g, *l))
                .unwrap_or((Icon::Monitor, ""));
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(icon(glyph, 15.0, muted_fg))
                .child(
                    text(label)
                        .flex_1()
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(fg()),
                )
        })
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
        .item_class("focus:bg-hover")
        .item_style(|st| st.px(px(10.0)).py(px(6.0)).rounded(px(6.0)))
        .trigger(trigger)
        .into_element()
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
/// 「字体」的下拉：nsh-base `select` —— 触发器用当前字体写自己的名字；
/// 内置搜索框（`searchable`），「系统字体」作为第一项，已安装字体懒加载
/// （`on_open` 才枚举，离开设置页随本节销毁）。行内容自绘：显示名用它
/// 自己的字体渲染 + 「歌词 Aa あ」样例，选中行有主题色底与对勾。
fn font_select() -> AnyElement {
    const SYSTEM: &str = "system-ui";
    let s = store();
    let current = move || s.library.with(|l| l.config.font.clone());
    // None：还没读取过（打开弹层才枚举已安装字体）。
    let installed = signal(None::<Same<Vec<String>>>);
    // 触发器是普通 view（select 接管按压）：select_trigger 的外壳 +
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
    let gate = super::super::super::store::menu_control();
    nsh_base::select()
        .items(move || {
            let mut items = vec![nsh_base::SelectItem::new(SYSTEM, "系统字体")];
            if let Some(list) = installed.get() {
                items.extend(
                    list.iter()
                        .map(|f| nsh_base::SelectItem::new(f.clone(), f.clone())),
                );
            }
            items
        })
        .value(move || {
            nsh_base::SelectValue::Controlled(Some(
                current().unwrap_or_else(|| SYSTEM.into()).into(),
            ))
        })
        .accessibility_label("字体")
        .searchable(true)
        .search_placeholder("搜索已安装的字体")
        .search_accessibility_label("搜索字体")
        .placement(Placement::Bottom)
        .align(Align::End)
        .offset(px(4.0))
        .margin(px(8.0))
        .open(gate.open())
        .on_open_change(gate.select_change())
        // 打开才枚举已安装字体；离开设置页本节销毁，数据与任务一起释放。
        .on_open(move || {
            if installed.get_untracked().is_some() {
                return;
            }
            spawn(async move {
                if let Ok(list) = blocking(super::super::super::fonts::installed_families).await {
                    installed.set(Some(Same(std::sync::Arc::new(list))));
                }
            });
        })
        .on_change(move |v: &Option<SharedString>| match v.as_deref() {
            Some(SYSTEM) | None => super::super::super::store::set_font(None),
            Some(f) => super::super::super::store::set_font(Some(f.to_owned())),
        })
        .render_item(move |e: &nsh_base::SelectEntry| {
            let family = e.value.clone();
            let title = e.title.to_string();
            let held = family.clone();
            let selected = std::rc::Rc::new(move || {
                current().unwrap_or_else(|| SYSTEM.to_owned()).as_str() == held.as_str()
            });
            let on = selected.clone();
            view()
                .h(px(40.0))
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .style_when(move || on(), |st| st.bg(move || primary().opacity(0.12)))
                .child(
                    text(move || format!("{} · 歌词 Aa あ", title))
                        .flex_1()
                        .min_w(px(0.0))
                        .number_of_lines(1)
                        .font_family(move || family.clone())
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .text_color(fg()),
                )
                .child(when(
                    move || selected(),
                    || icon(Icon::Check, 14.0, primary),
                ))
        })
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
        .list_style(|st| st.max_h(px(216.0)))
        .item_class("focus:bg-hover")
        .item_style(|st| st.rounded(px(6.0)))
        .search_style(|st| {
            st.h(px(32.0))
                .px(px(10.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(fg())
        })
        .empty(
            view().h(px(64.0)).items_center().justify_center().child(
                text(move || {
                    if installed.get().is_none() {
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
        .trigger(trigger)
        .into_element()
}
