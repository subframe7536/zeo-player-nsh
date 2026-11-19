//! 设置 →「外观」分类的各节。

use nsh_core::prelude::*;

use super::setting_section;
use zeo_types::config::ThemeColor;
use zeo_ui_components::button::RADIUS_MD;
use zeo_ui_components::form::trigger_chevron;
use zeo_ui_components::icons::{Icon, icon};
use zeo_ui_store::theme::*;
use zeo_ui_store::{Same, set_theme_color, store};

/// 外观：跟随系统 / 固定浅色 / 固定深色（同一行右侧的下拉）。
pub(crate) fn appearance_section() -> View {
    setting_section(
        "外观",
        "跟随系统的浅色 / 深色外观，或为界面固定一种。",
        Some(appearance_select()),
    )
}
/// 「外观」的下拉：触发器显示当前值，复用通用 `select` 列三个带图标的选项。
fn appearance_select() -> AnyElement {
    use zeo_types::config::Appearance;
    use zeo_ui_components::form::{SelectOption, select_trigger, select_with_trigger};
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
    let trigger = select_trigger(
        current,
        move || current_mode().map(|(_, g, _)| g),
        "外观",
        110.0,
        "",
    );
    let options = OPTIONS
        .iter()
        .map(|&(mode, glyph, label)| {
            SelectOption::new(label, move || {
                zeo_ui_store::set_appearance(mode);
            })
            .with_glyph(glyph)
        })
        .collect::<Vec<_>>();
    select_with_trigger(current, "外观", options, 110.0, Some(trigger))
}
/// 主题色：「黑白」/「封面」/「自定义」卡片；选中「自定义」时标题右侧的
/// 色块打开取色器弹层（HSB 区域 + 色相条 + HEX 字段，无预设）。
/// 欢迎指南的「主题色」一步也复用它（`zeo-ui-view-welcome`）。
pub fn accent_section() -> View {
    use zeo_ui_components::radio;

    let s = store();
    let current_theme = move || s.library.with(|l| l.config.theme_color);
    let initial_custom = match s.library.get_untracked().config.theme_color {
        ThemeColor::Custom(rgb) => rgb,
        _ => 0x2563eb,
    };
    let last_custom = signal(initial_custom);

    let is_neutral = move || matches!(current_theme(), ThemeColor::Neutral);
    let is_cover = move || matches!(current_theme(), ThemeColor::Cover);
    let is_custom = move || matches!(current_theme(), ThemeColor::Custom(_));

    let radio_group = radio::group_cards()
        .child(radio::card(
            is_neutral,
            "黑白",
            "浅色下主黑、深色下主白",
            move || set_theme_color(ThemeColor::Neutral),
        ))
        .child(radio::card(
            is_cover,
            "封面",
            "取当前曲目封面主色调",
            move || set_theme_color(ThemeColor::Cover),
        ))
        .child(radio::card(
            is_custom,
            "自定义",
            "预设色或颜色代码",
            move || set_theme_color(ThemeColor::Custom(last_custom.get())),
        ));

    setting_section(
        "主题色",
        "按钮与选中态的颜色。",
        Some(when(is_custom, move || custom_picker(last_custom)).into_element()),
    )
    .child(radio_group)
}

/// 「自定义」取色器：标题右侧的色块触发器 + 弹层（HSB 区域 + 色相条 +
/// HEX 字段，无预设）。拖动中只写 `s.accent` 实时预览（不广播
/// `s.library`、不落盘，同 `apply_eq_live`），`on_change_end` 才走
/// `set_theme_color` 全量收尾。
fn custom_picker(last_custom: Signal<u32>) -> AnyElement {
    let s = store();
    nsh_base::popover()
        .placement(Placement::Bottom)
        .align(Align::End)
        .offset(px(8.0))
        .popup_style(|st| {
            st.p(px(12.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(border())
                .bg(nsh_base::theme().popover)
                .shadow_lg()
        })
        .trigger(
            view()
                .size(px(28.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(border())
                .bg(move || custom_accent(last_custom.get()))
                .transition_colors()
                .duration(150.0)
                .shadow_xs()
                .role(AccessibilityRole::Button)
                .label("自定义颜色"),
        )
        .child(
            nsh_base::color_picker()
                .value(move || Some(rgb(last_custom.get()).to_hsla()))
                .on_change(move |e: &nsh_base::ColorPickerEvent| {
                    if let nsh_base::ColorPickerEvent::Change(v, _) = e {
                        let rgb_v = u32::from(v.to_rgb()) >> 8;
                        last_custom.set(rgb_v);
                        s.accent.set(Some(custom_accent(rgb_v)));
                    }
                })
                .on_change_end(move |e: &nsh_base::ColorPickerEvent| {
                    if let nsh_base::ColorPickerEvent::ChangeEnd(v, _) = e {
                        let rgb_v = u32::from(v.to_rgb()) >> 8;
                        last_custom.set(rgb_v);
                        set_theme_color(ThemeColor::Custom(rgb_v));
                    }
                })
                .child(
                    view()
                        .gap(px(10.0))
                        .child(
                            nsh_base::color_area()
                                .accessibility_label("自定义颜色")
                                .w(px(216.0))
                                .h(px(140.0))
                                .panel_style(|st| st.rounded(px(6.0)))
                                .fill_thumb(true)
                                .thumb_style(|st| {
                                    st.size(px(14.0))
                                        .rounded_full()
                                        .border_2()
                                        .border_color(AdaptiveColor::rgb(0xffffff, 0xffffff))
                                        .shadow_xs()
                                }),
                        )
                        .child(
                            nsh_base::color_slider()
                                .accessibility_label("色相")
                                .w(px(216.0))
                                .h(px(16.0))
                                .justify_center()
                                .track_style(|st| st.h(px(10.0)).rounded_full())
                                .fill_thumb(true)
                                .thumb_style(|st| {
                                    st.size(px(14.0))
                                        .rounded_full()
                                        .border_2()
                                        .border_color(AdaptiveColor::rgb(0xffffff, 0xffffff))
                                        .shadow_xs()
                                }),
                        )
                        .child(
                            nsh_base::color_field()
                                .accessibility_label("自定义颜色代码")
                                .h(px(32.0))
                                .px(px(10.0))
                                .rounded(px(RADIUS_MD))
                                .border_1()
                                .border_color(nsh_base::theme().input)
                                .bg(nsh_base::theme().card)
                                .shadow_xs(),
                        ),
                ),
        )
        .into_element()
}
/// 字体：系统字体或任一已安装字体（同一行右侧的下拉，带搜索与逐字体预览）。
/// 字体列表懒加载：打开弹层才读取已安装字体；关闭或离开设置页时整块销毁
/// （列表数据、读取任务、各行的字体预览一起释放）。
pub(crate) fn font_section() -> View {
    setting_section(
        "字体",
        "整个界面换用所选字体，立即生效。",
        Some(font_select()),
    )
}
/// 「字体」的 combobox：输入框即搜索框，输入即过滤已安装字体（`on_open`
/// 才枚举，离开设置页随本节销毁），点选 / 回车确认；「系统字体」固定第
/// 一项，清除钮回到它。输入框显示选中项的名字、用它自己的字体渲染
/// （根上的 `font_family` 继承进内部 input），所见即所得。行内容自绘：
/// 显示名 + 「歌词 Aa あ」样例，选中行有主题色底与对勾。
fn font_select() -> AnyElement {
    const SYSTEM: &str = "system-ui";
    let s = store();
    let current = move || s.library.with(|l| l.config.font.clone());
    // None：还没读取过（打开弹层才枚举已安装字体）。
    let installed = signal(None::<Same<Vec<String>>>);
    let t = nsh_base::theme();
    let (input_c, ring, card_c) = (t.input, t.ring, t.card);
    nsh_base::combobox()
        // combobox 的根是 input 所在的行：外壳复刻输入框（同编辑信息的
        // 流派行），chevron 在行内、清除钮在其左。
        .w(px(200.0))
        .h(px(32.0))
        .items_center()
        .gap(px(4.0))
        .pl(px(12.0))
        .pr(px(8.0))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(input_c)
        .shadow_xs()
        .bg(card_c)
        .font_family(move || SharedString::from(current().unwrap_or_else(|| SYSTEM.to_owned())))
        .focus_within(move |st| {
            st.border_color(ring)
                .ring(px(3.0))
                .ring_color(ring.opacity(0.5))
        })
        .items(move || {
            let mut items = vec![nsh_base::SelectItem::new(SYSTEM, "系统字体")];
            if let Some(list) = installed.get() {
                items.extend(
                    list.iter()
                        .map(|f| nsh_base::SelectItem::new(f.clone(), f.clone())),
                );
            }
            // 当前字体还没枚举到 / 已卸载时也留一项：输入框显示选中项的
            // title，找不到项会空白。
            if let Some(font) = current()
                && !items.iter().any(|i| i.value.as_str() == font)
            {
                items.push(nsh_base::SelectItem::new(font.clone(), font));
            }
            items
        })
        .value(move || {
            Some(vec![SharedString::from(
                current().unwrap_or_else(|| SYSTEM.to_owned()),
            )])
        })
        .placeholder(Some(SharedString::from("搜索或输入字体")))
        .accessibility_label(Some(SharedString::from("字体")))
        // 清除 = 回到系统字体（受控值复位为 system-ui）；已是系统默认时
        // 不出清除钮。
        .cleanable(move || current().is_some())
        .clear_button(
            view()
                .w(px(20.0))
                .h(px(20.0))
                .items_center()
                .justify_center()
                .child(icon(Icon::Close, 12.0, muted_fg)),
        )
        .clear_button_accessibility_label(Some(SharedString::from("重置为系统字体")))
        .placement(Placement::Bottom)
        .align(Align::End)
        .offset(px(4.0))
        .margin(px(8.0))
        // 打开才枚举已安装字体；离开设置页本节销毁，数据与任务一起释放。
        .on_open(move || {
            if installed.get_untracked().is_some() {
                return;
            }
            spawn(async move {
                if let Ok(list) = blocking(zeo_ui_store::fonts::installed_families).await {
                    installed.set(Some(Same(std::sync::Arc::new(list))));
                }
            });
        })
        .on_change(
            move |v: &Vec<SharedString>| match v.first().map(|v| v.as_str()) {
                Some(SYSTEM) | None => zeo_ui_store::set_font(None),
                Some(f) => zeo_ui_store::set_font(Some(f.to_owned())),
            },
        )
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
        .input_style(move |st| {
            st.flex_1()
                .min_w(px(0.0))
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(fg())
        })
        .trigger_button(trigger_chevron())
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
        .into_element()
}
