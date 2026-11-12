//! 设置 →「外观」分类的各节。

use nsh_core::prelude::*;

use super::setting_section;
use zeo_types::config::ThemeColor;
use zeo_ui_components::button::RADIUS_MD;
use zeo_ui_components::form::trigger_chevron;
use zeo_ui_components::icons::{Icon, icon};
use zeo_ui_store::theme::*;
use zeo_ui_store::{Same, accent, set_theme_color, store};

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
pub const PRESETS: &[(u32, &'static str)] = &[
    (0xe5263f, "红"),
    (0xea580c, "橙"),
    (0x16a34a, "绿"),
    (0x2563eb, "蓝"),
    (0x9333ea, "紫"),
];

/// 主题色：支持配置「黑白」、「封面」、「自定义」（预设强调色与十六进制颜色代码）。
pub(crate) fn accent_section() -> View {
    use zeo_ui_components::form::input_field;
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

    let active_rgb = move || match current_theme() {
        ThemeColor::Custom(rgb) => rgb,
        _ => last_custom.get(),
    };

    let hex_draft = signal(format!("#{:06X}", active_rgb()));
    let computed_rgb = computed(active_rgb);
    effect(move || {
        hex_draft.set(format!("#{:06X}", computed_rgb.get()));
    });

    let radio_group = radio::group_row()
        .child(radio::item(is_neutral, "黑白", move || {
            set_theme_color(ThemeColor::Neutral)
        }))
        .child(radio::item(is_cover, "封面", move || {
            set_theme_color(ThemeColor::Cover)
        }))
        .child(radio::item(is_custom, "自定义", move || {
            set_theme_color(ThemeColor::Custom(last_custom.get()))
        }));

    let cover_hint = move || {
        view()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .child(
                view()
                    .size(px(20.0))
                    .rounded_full()
                    .bg(move || accent().unwrap_or_else(muted))
                    .transition_colors()
                    .duration(300.0)
                    .items_center()
                    .justify_center()
                    .child(icon(Icon::Disc, 12.0, move || {
                        if accent().is_some() {
                            AdaptiveColor::rgb(0xffffff, 0xffffff)
                        } else {
                            muted_fg()
                        }
                    })),
            )
            .child(
                text(move || {
                    if accent().is_some() {
                        "当前曲目封面主色调（已启用动态强调色与底色微调）"
                    } else {
                        "提取当前曲目封面主色调；未播放或无封面时使用黑白"
                    }
                })
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg()),
            )
    };

    let neutral_hint = move || {
        view().flex_row().items_center().gap(px(10.0)).child(
            text("使用主题默认的黑白色调，在浅色外观下为主黑色、深色外观下为主白色。")
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg()),
        )
    };

    let custom_panel = move || {
        view().gap(px(12.0)).child(
            view()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(16.0))
                .child(
                    // 预设颜色圆形色块
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .children(PRESETS.iter().map(|&(rgb, label)| {
                            let on = move || is_custom() && active_rgb() == rgb;
                            nsh_base::button()
                                .size(px(24.0))
                                .rounded_full()
                                .items_center()
                                .justify_center()
                                .bg(move || custom_accent(rgb))
                                .transition_colors()
                                .duration(300.0)
                                .accessibility_label(SharedString::from(format!("主题色：{label}")))
                                .selected(on)
                                .hover(|st| st.opacity(0.85))
                                .active(|st| st.opacity(0.7))
                                .on_press(move |_: &()| {
                                    last_custom.set(rgb);
                                    set_theme_color(ThemeColor::Custom(rgb));
                                })
                                .child(
                                    icon(Icon::Check, 12.0, move || {
                                        AdaptiveColor::rgb(0xffffff, 0xffffff)
                                    })
                                    .opacity(move || if on() { 1.0_f32 } else { 0.0_f32 }),
                                )
                                .into_element()
                        })),
                )
                .child(view().w(px(1.0)).h(px(18.0)).bg(border()).flex_shrink_0())
                .child(
                    // 自定义 HEX 输入与预览色块
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            view()
                                .size(px(24.0))
                                .rounded(px(6.0))
                                .border_1()
                                .border_color(border())
                                .bg(move || custom_accent(active_rgb()))
                                .transition_colors()
                                .duration(300.0)
                                .shadow_xs(),
                        )
                        .child(
                            input_field(true)
                                .w(px(96.0))
                                .value(move || hex_draft.get())
                                .placeholder("#2563EB")
                                .label("自定义颜色代码")
                                .on_change(move |t: &str| {
                                    hex_draft.set(t.to_owned());
                                    let clean = t.trim().trim_start_matches('#');
                                    if clean.len() == 6 {
                                        if let Ok(rgb) = u32::from_str_radix(clean, 16) {
                                            last_custom.set(rgb);
                                            set_theme_color(ThemeColor::Custom(rgb));
                                        }
                                    }
                                })
                                .on_blur(move || {
                                    hex_draft.set(format!("#{:06X}", active_rgb()));
                                }),
                        ),
                ),
        )
    };

    setting_section(
        "主题色",
        "按钮与选中态的强调色：支持黑白、跟随当前曲目封面或自定义颜色。",
        None,
    )
    .child(radio_group)
    .child(when(is_neutral, neutral_hint))
    .child(when(is_cover, cover_hint))
    .child(when(is_custom, custom_panel))
}
/// 字体：系统字体或任一已安装字体（同一行右侧的下拉，带搜索与逐字体预览）。
/// 字体列表懒加载：打开弹层才读取已安装字体；关闭或离开设置页时整块销毁
/// （列表数据、读取任务、各行的字体预览一起释放）。
pub(crate) fn font_section() -> View {
    setting_section(
        "字体",
        "整个界面（包括歌词）换用所选字体，立即生效。",
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
