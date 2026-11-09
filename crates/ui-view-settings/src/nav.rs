use super::*;

/// 左栏顶部的搜索框（外观同主侧栏的搜索框；有词时出现清空按钮）。
pub(crate) fn search_box(query: Signal<String>) -> View {
    let empty = move || query.with(String::is_empty);
    view()
        .h(px(28.0))
        .flex_row()
        .items_center()
        .pl(px(10.0))
        .pr(px(1.0))
        .rounded(px(14.0))
        .border_1()
        .border_color(border())
        .bg(move || bg())
        .child(icon(Icon::Search, 13.0, faint).flex_shrink_0())
        .child(
            input()
                .flex_1()
                .min_w(px(0.0))
                .h(px(26.0))
                .px(px(6.0))
                .text_size(px(13.0))
                .text_color(fg())
                .value(move || query.get())
                .placeholder("搜索")
                .label("搜索设置")
                .on_change(move |t: &str| query.set(t.to_owned())),
        )
        .child(when(
            move || !empty(),
            move || {
                icon_button(
                    || Icon::Close,
                    10.0,
                    "清除搜索",
                    faint,
                    move || query.set(String::new()),
                )
                .flex_shrink_0()
            },
        ))
}

/// 左栏分类行（外观同主侧栏的导航行）。
pub(crate) fn pane_row(p: Pane, pane: Signal<Pane>) -> AnyElement {
    let active = move || pane.get() == p;
    nsh_base::button()
        .rounded(px(8.0))
        .accessibility_label(p.title())
        .selected(active)
        .selected_style(|st| st.bg(move || primary().opacity(0.12)))
        .hover(|st| st.bg(move || primary().opacity(0.08)))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| pane.set(p))
        .h(px(32.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(10.0))
        .w_full()
        .child(icon(pane_glyph(p), 15.0, move || {
            if active() { primary() } else { sidebar_fg() }
        }))
        .child(
            text(p.title())
                .text_size(px(13.0))
                .line_height(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if active() { fg() } else { sidebar_fg() }),
        )
        .into_element()
}

/// 搜索结果行：节的标题 + 所属分类，按下选中该分类（保留搜索词，右栏
/// 只显示命中的节）。
pub(crate) fn result_row(s: &'static Section, pane: Signal<Pane>) -> nsh_base::ButtonElement {
    nsh_base::button()
        .h(px(40.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(10.0))
        .rounded(px(8.0))
        .accessibility_label(s.title)
        .hover(|st| st.bg(move || primary().opacity(0.08)))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| pane.set(s.pane))
        .child(icon(pane_glyph(s.pane), 15.0, sidebar_fg))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(1.0))
                .child(
                    text(s.title)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(sidebar_fg()),
                )
                .child(
                    text(s.pane.title())
                        .text_size(px(11.0))
                        .line_height(px(11.0))
                        .text_color(faint()),
                ),
        )
}

/// 左栏分组的小标题（首个与搜索框之间不留大间隔，其余组间拉开）。
pub(crate) fn group_heading(name: &'static str, spaced: bool) -> View {
    view()
        .px(px(10.0))
        .pt(if spaced { px(18.0) } else { px(4.0) })
        .pb(px(4.0))
        .child(
            text(name)
                .text_size(px(11.0))
                .line_height(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(faint()),
        )
}

/// 左栏：顶部搜索框，下面是分类列表（有搜索词时为命中的节列表）。
pub(crate) fn nav_column(pane: Signal<Pane>, query: Signal<String>) -> View {
    let rows = dynamic(move || {
        let q = query.get().trim().to_lowercase();
        if q.is_empty() {
            return Pane::GROUPS
                .iter()
                .enumerate()
                .flat_map(|(i, &(name, panes))| {
                    let heading = group_heading(name, i > 0).into_element();
                    [heading]
                        .into_iter()
                        .chain(panes.iter().map(|&p| pane_row(p, pane)))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
        }
        SECTIONS
            .iter()
            .filter(|s| section_hits(&q, s))
            .map(|s| result_row(s, pane).into_element())
            .collect()
    });
    view()
        .w(px(220.0))
        .flex_shrink_0()
        .bg(move || sidebar())
        .transition_colors()
        .duration(300.0)
        .test_id("settings-nav")
        .child(view().p(px(10.0)).child(search_box(query)))
        .child(
            scroll()
                .flex_1()
                .child(view().px(px(10.0)).pb(px(10.0)).gap(px(2.0)).child(rows)),
        )
}
