//! 设置 →「标签」分类的各节。

use std::collections::{HashMap, HashSet};

use nsh_core::prelude::*;

use super::super::super::components::button::{Size, Variant, outline_button, text_button};
use super::super::super::components::icons::{Icon, icon};
use super::super::super::store::store;
use super::super::super::theme::*;
use super::setting_section;

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
pub(super) fn genres_section() -> View {
    let s = store();
    let draft = signal(String::new());
    let separators = move || s.library.with(|l| l.config.genre_separators.clone());
    let toggle = move |sep: String| {
        let mut list = separators();
        if let Some(i) = list.iter().position(|x| *x == sep) {
            list.remove(i);
        } else {
            list.push(sep);
        }
        super::super::super::store::set_separators(list);
    };
    let add_custom = move || {
        let sep = draft.get_untracked();
        if sep.is_empty() {
            return;
        }
        let mut list = separators();
        if !list.contains(&sep) {
            list.push(sep);
            super::super::super::store::set_separators(list);
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

    let custom = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            super::super::super::components::form::input_field(true)
                .w(px(160.0))
                .value(move || draft.get())
                .placeholder("其它分隔符，如 ・")
                .on_change(move |text: &str| draft.set(text.to_owned()))
                .on_submit(move |_: &str| add_custom()),
        )
        .child(outline_button(Icon::Plus, "添加", add_custom))
        .child(view().flex_1())
        .child(text_button(
            Variant::Ghost,
            Size::Sm,
            None,
            "恢复默认",
            || {
                super::super::super::store::set_separators(
                    crate::config::DEFAULT_SEPARATORS
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect(),
                )
            },
        ));

    // 预览：曲库里最常见的、含多个值的原始流派标签，按当前分隔符拆开。
    let samples = computed(move || {
        let tracks = s.tracks.get();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for t in tracks.iter() {
            if let Ok(meta) =
                crate::metadata::MetaCache::global().get(std::path::Path::new(&t.path))
            {
                for raw in &meta.genres {
                    *counts.entry(raw.clone()).or_default() += 1;
                }
            }
        }
        let mut list: Vec<(String, usize)> = counts.into_iter().collect();
        let busy = |raw: &str| {
            raw.chars()
                .filter(|c| !c.is_alphanumeric() && !c.is_whitespace())
                .count()
        };
        list.sort_by(|a, b| {
            busy(&b.0)
                .cmp(&busy(&a.0))
                .then(b.1.cmp(&a.1))
                .then(a.0.cmp(&b.0))
        });
        list.into_iter()
            .take(4)
            .map(|(raw, _)| raw)
            .collect::<Vec<_>>()
    });
    let preview = dynamic(move || {
        let seps = separators();
        let rows: Vec<AnyElement> = samples
            .get()
            .into_iter()
            .map(|raw| {
                let parts = crate::config::split_genres(std::slice::from_ref(&raw), &seps);
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0))
                    .min_h(px(32.0))
                    .child(
                        text(raw)
                            .number_of_lines(1)
                            .w(px(220.0))
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(muted_fg()),
                    )
                    .child(icon(Icon::ChevronRight, 12.0, faint))
                    .child(
                        view()
                            .flex_1()
                            .flex_row()
                            .flex_wrap()
                            .gap(px(6.0))
                            .children(parts.into_iter().map(|g| {
                                view()
                                    .rounded_full()
                                    .bg(muted())
                                    .px(px(10.0))
                                    .py(px(3.0))
                                    .child(
                                        text(g)
                                            .text_size(px(12.0))
                                            .line_height(px(12.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(fg()),
                                    )
                            })),
                    )
                    .into_element()
            })
            .collect();
        (!rows.is_empty()).then(|| {
            view()
                .gap(px(4.0))
                .p(px(12.0))
                .rounded(px(10.0))
                .bg(move || bg())
                .child(
                    text(move || {
                        let total: HashSet<String> = s
                            .tracks
                            .with(|t| t.iter().flat_map(|t| t.genres.iter().cloned()).collect());
                        format!("预览 · 当前共 {} 个标签", total.len())
                    })
                    .text_size(px(11.0))
                    .line_height(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .letter_spacing(px(0.6))
                    .text_color(faint())
                    .pb(px(4.0)),
                )
                .children(rows)
        })
    });

    setting_section(
        "标签",
        "标签解析自曲目的流派（genre）信息。一个流派里有多个值时，按下面选中的分隔符拆开，并去掉首尾空格。修改后立即生效，不用重新扫描。",
        None,
    )
    .child(view().flex_row().flex_wrap().gap(px(8.0)).child(chips))
    .child(custom)
    .child(preview)
}
