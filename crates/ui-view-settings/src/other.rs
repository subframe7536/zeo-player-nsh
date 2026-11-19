//! 设置 →「其他」分类的各节。

use nsh_core::prelude::*;

use super::setting_section;
use zeo_ui_components::button::{RADIUS_MD, Size, Variant, text_button};
use zeo_ui_components::form::trigger_chevron;
use zeo_ui_components::icons::{Icon, icon};
use zeo_ui_store::store;
use zeo_ui_store::theme::*;

/// 常用分隔符（下拉里直接勾选）。
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

/// 预设符号的中文名（自定义的没有）。
fn preset_name(sep: &str) -> Option<&'static str> {
    PRESETS.iter().find(|(p, _)| *p == sep).map(|(_, n)| *n)
}

/// 分隔符多选：与「编辑元数据」的标签输入器同一套 nsh-base 多选
/// combobox。已选的以 chips 排在框内（`prefix`），下拉列出预设符号 +
/// 已选的自定义符号，打勾即选中、再点取消；输入即按标题过滤，查询不是
/// 已有符号时最上面多一项「添加“…”」，回车 / 点选自由新增。
fn separators_section(
    title: &'static str,
    desc: &'static str,
    label: &'static str,
    separators: impl Fn() -> Vec<String> + Copy + 'static,
    set_separators: impl Fn(Vec<String>) + Copy + 'static,
    defaults: &'static [&'static str],
) -> View {
    let t = nsh_base::theme();
    let (input_c, ring, popover_c, card_c) = (t.input, t.ring, t.popover, t.card);
    let query = signal(String::new());
    let items = move || {
        let q = query.get();
        let current = separators();
        let mut items = Vec::with_capacity(PRESETS.len() + current.len() + 1);
        let known =
            |x: &str| PRESETS.iter().any(|(p, _)| *p == x) || current.iter().any(|c| c == x);
        if !q.trim().is_empty() && !known(&q) {
            items.push(nsh_base::SelectItem::new(
                q.clone(),
                format!("添加“{}”", separator_label(&q)),
            ));
        }
        items.extend(
            PRESETS
                .iter()
                .map(|(sep, name)| nsh_base::SelectItem::new(*sep, format!("{sep} {name}"))),
        );
        items.extend(
            current
                .iter()
                .filter(|c| preset_name(c).is_none())
                .map(|c| nsh_base::SelectItem::new(c.clone(), separator_label(c))),
        );
        items
    };
    let remove = move |sep: String| {
        let mut list = separators();
        list.retain(|x| *x != sep);
        set_separators(list);
    };
    let chips = move || {
        view()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(4.0))
            .child(dynamic(move || {
                separators()
                    .into_iter()
                    .map(|sep| separator_chip(sep, remove).into_element())
                    .collect::<Vec<_>>()
            }))
    };
    let is_default = move || {
        separators()
            .iter()
            .map(String::as_str)
            .eq(defaults.iter().copied())
    };

    setting_section(
        title,
        desc,
        Some(
            text_button(Variant::Ghost, Size::Sm, None, "恢复默认", move || {
                set_separators(defaults.iter().map(|s| (*s).to_owned()).collect());
            })
            .disabled(is_default)
            .into_element(),
        ),
    )
    .child(
        nsh_base::combobox()
            .w(percentage(1.0))
            .min_h(px(36.0))
            // 根行折行：chips 顶满整行时输入框折到下一行；右内边距给绝对
            // 定位的 chevron 留位。
            .flex_wrap()
            .items_center()
            .pl(px(6.0))
            .pr(px(30.0))
            .py(px(5.0))
            .gap(px(6.0))
            .rounded(px(RADIUS_MD))
            .border_1()
            .border_color(input_c)
            .shadow_xs()
            .bg(card_c)
            .focus_within(move |st| {
                st.border_color(ring)
                    .ring(px(3.0))
                    .ring_color(ring.opacity(0.5))
            })
            .items(items)
            .multiple(true)
            .value(move || {
                Some(
                    separators()
                        .into_iter()
                        .map(SharedString::from)
                        .collect::<Vec<_>>(),
                )
            })
            .query(move || Some(SharedString::from(query.get())))
            .on_query_change(move |q: &SharedString| query.set(q.to_string()))
            .on_change(move |v: &Vec<SharedString>| {
                let mut list: Vec<String> = Vec::new();
                for x in v {
                    if !x.trim().is_empty() && !list.iter().any(|k| k == x.as_ref()) {
                        list.push(x.to_string());
                    }
                }
                set_separators(list);
                query.set(String::new());
            })
            .placeholder(Some(SharedString::from("输入符号，回车添加")))
            .accessibility_label(Some(SharedString::from(label)))
            .placement(Placement::Bottom)
            .align(Align::Start)
            .offset(px(4.0))
            .margin(px(8.0))
            .popup_style(move |st| {
                st.min_w(px(200.0))
                    .p(px(4.0))
                    .rounded(px(10.0))
                    .overflow_hidden()
                    .border_1()
                    .border_color(border())
                    .bg(popover_c)
                    .shadow_lg()
            })
            .item_class("focus:bg-hover")
            .item_style(|st| st.px(px(8.0)).py(px(6.0)).rounded(px(6.0)))
            .render_item(move |entry: &nsh_base::SelectEntry| separator_item(separators, entry))
            .input_style(move |st| {
                // 紧跟最后一个 chip、吃掉该行剩余宽度；放不下时折到下一行。
                st.h(px(24.0))
                    .flex_1()
                    .min_w(px(140.0))
                    .text_size(px(13.0))
                    .text_color(fg())
            })
            .prefix(when(move || !separators().is_empty(), move || chips()))
            .trigger_button(
                view()
                    .absolute()
                    .top(px(5.0))
                    .right(px(6.0))
                    .h(px(24.0))
                    .items_center()
                    .justify_center()
                    .child(trigger_chevron()),
            )
            .empty(
                text("没有匹配的符号")
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(faint()),
            ),
    )
}

/// 下拉行：复选框 + 符号 + 中文名。「添加“…”」项不在已选集里，显示空框。
fn separator_item(
    separators: impl Fn() -> Vec<String> + Copy + 'static,
    entry: &nsh_base::SelectEntry,
) -> View {
    let value = entry.value.to_string();
    let name = preset_name(&value);
    let checked = {
        let value = value.clone();
        move || separators().contains(&value)
    };
    // 预设拆成「符号 + 名称」两段；自定义符号与「添加“…”」照标题显示。
    let symbol = match name {
        Some(_) => separator_label(&value),
        None => entry.title.to_string(),
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            view()
                .w(px(16.0))
                .h(px(16.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(border())
                .items_center()
                .justify_center()
                .child(when(checked, || icon(Icon::Check, 10.0, fg))),
        )
        .child(
            text(symbol)
                .min_w(px(14.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg()),
        )
        .child(name.map(|n| {
            text(n)
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg())
        }))
}

/// 一个已选符号 chip：符号 + 中文名 + × 按钮。
fn separator_chip(sep: String, remove: impl Fn(String) + Copy + 'static) -> View {
    let name = preset_name(&sep);
    let press = sep.clone();
    let remove_label = format!("移除「{}」", separator_label(&sep));
    view()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .h(px(24.0))
        .pl(px(8.0))
        .pr(px(4.0))
        .rounded(px(6.0))
        .bg(muted())
        .child(
            text(separator_label(&sep))
                .text_size(px(12.0))
                .line_height(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg()),
        )
        .child(name.map(|n| {
            text(n)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg())
        }))
        .child(
            view()
                .h(px(16.0))
                .w(px(16.0))
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .hover(|st| st.bg(hover()))
                .role(AccessibilityRole::Button)
                .label(remove_label)
                .on_press(move || remove(press.clone()))
                .child(icon(Icon::Close, 10.0, muted_fg)),
        )
}

pub(crate) fn artist_separators_section() -> View {
    let s = store();
    separators_section(
        "艺术家分隔符",
        "一首歌有多位歌手时（如「歌手 A / 歌手 B」），按选中的符号拆成各自的艺术家。",
        "艺术家分隔符",
        move || s.library.with(|l| l.config.artist_separators.clone()),
        zeo_ui_store::set_artist_separators,
        zeo_types::config::DEFAULT_ARTIST_SEPARATORS,
    )
}

pub(crate) fn genre_separators_section() -> View {
    let s = store();
    separators_section(
        "标签分隔符",
        "一首歌标了多个流派时（如「流行; 摇滚」），按选中的符号拆成多个标签。",
        "标签分隔符",
        move || s.library.with(|l| l.config.genre_separators.clone()),
        zeo_ui_store::set_separators,
        zeo_types::config::DEFAULT_SEPARATORS,
    )
}
