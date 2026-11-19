//! 设置 →「其他」分类的各节。

use nsh_core::prelude::*;

use super::setting_section;
use zeo_ui_components::button::{Size, Variant, text_button};
use zeo_ui_components::icons::{Icon, icon};
use zeo_ui_store::store;
use zeo_ui_store::theme::*;

/// 常用分隔符（可直接点选）。
const PRESETS: &[(&str, &str)] = &[
    (";", "分号"),
    ("/", "斜杠"),
    (",", "逗号"),
    ("、", "顿号"),
    ("|", "竖线"),
    ("&", "和号"),
];

fn separator_label(sep: &str) -> String {
    match sep {
        " " => "空格".into(),
        s if s.trim().is_empty() => format!("{:?}", s),
        s => s.to_owned(),
    }
}

fn separators_section(
    title: &'static str,
    desc: &'static str,
    separators: impl Fn() -> Vec<String> + Copy + 'static,
    set_separators: impl Fn(Vec<String>) + Copy + 'static,
    defaults: &'static [&'static str],
) -> View {
    let draft = signal(String::new());
    let toggle = move |sep: String| {
        let mut list = separators();
        if let Some(i) = list.iter().position(|x| *x == sep) {
            list.remove(i);
        } else {
            list.push(sep);
        }
        set_separators(list);
    };
    let add_custom = move || {
        let sep = draft.get_untracked();
        if sep.is_empty() {
            return;
        }
        let mut list = separators();
        if !list.contains(&sep) {
            list.push(sep);
            set_separators(list);
        }
        draft.set(String::new());
    };

    let chip = move |sep: String, caption: Option<&'static str>| {
        let on = {
            let sep = sep.clone();
            move || separators().contains(&sep)
        };
        let press = sep.clone();
        nsh_base::button()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .h(px(32.0))
            .px(px(12.0))
            .rounded_full()
            .border_1()
            .border_color(border())
            .selected(on)
            .selected_style(|st| {
                st.border_color(move || primary())
                    .bg(move || primary().opacity(0.12))
            })
            .transition_colors()
            .duration(300.0)
            .accessibility_label(caption.unwrap_or("自定义分隔符"))
            .hover(|st| st.bg(hover()))
            .active(|st| st.opacity(0.7))
            .on_press(move |_: &()| toggle(press.clone()))
            .child(
                text(separator_label(&sep))
                    .text_size(px(14.0))
                    .line_height(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(fg()),
            )
            .child(caption.map(|c| {
                text(c)
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(muted_fg())
            }))
    };

    let chips = dynamic(move || {
        let current = separators();
        let mut out: Vec<AnyElement> = PRESETS
            .iter()
            .map(|(sep, name)| chip((*sep).to_owned(), Some(*name)).into_element())
            .collect();
        for sep in current
            .iter()
            .filter(|c| !PRESETS.iter().any(|(p, _)| p == c))
        {
            out.push(
                chip(sep.clone(), None)
                    .child(icon(Icon::Close, 11.0, muted_fg))
                    .into_element(),
            );
        }
        out
    });

    setting_section(
        title,
        desc,
        Some(
            text_button(
                Variant::Outline,
                Size::Sm,
                None,
                "恢复默认",
                move || {
                    set_separators(defaults.iter().map(|s| (*s).to_owned()).collect());
                },
            )
            .into_element(),
        ),
    )
    .child(
        // chips + 行尾输入：与 32px 高的 chips 同排同高。
        view()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(8.0))
            .child(chips)
            .child(
                zeo_ui_components::form::input_field(true)
                    .w(px(128.0))
                    .h(px(32.0))
                    .value(move || draft.get())
                    .placeholder("其它分隔符，如 ・")
                    .on_change(move |text: &str| draft.set(text.to_owned()))
                    .on_submit(move |_: &str| add_custom())
                    .on_blur(move || add_custom()),
            )
            .child(text_button(
                Variant::Outline,
                Size::Sm,
                Some(Icon::Plus),
                "添加",
                add_custom,
            )),
    )
}

pub(crate) fn artist_separators_section() -> View {
    let s = store();
    separators_section(
        "艺术家分隔符",
        "多值艺术家按选中的分隔符拆分，立即生效。",
        move || s.library.with(|l| l.config.artist_separators.clone()),
        zeo_ui_store::set_artist_separators,
        zeo_types::config::DEFAULT_ARTIST_SEPARATORS,
    )
}

pub(crate) fn genre_separators_section() -> View {
    let s = store();
    separators_section(
        "标签分隔符",
        "多值流派按选中的分隔符拆分（去首尾空格），立即生效。",
        move || s.library.with(|l| l.config.genre_separators.clone()),
        zeo_ui_store::set_separators,
        zeo_types::config::DEFAULT_SEPARATORS,
    )
}
