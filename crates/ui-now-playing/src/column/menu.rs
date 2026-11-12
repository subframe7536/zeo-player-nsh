use super::*;

/// 「⋯」：添加到歌单、在访达中显示、拷贝文件路径。
pub(crate) fn more_menu() -> AnyElement {
    let s = store();
    let path = move || s.track.with_untracked(|t| t.path.clone());
    let item = |label: &'static str, glyph: Icon, on_select: Box<dyn Fn()>| {
        menu_item()
            .on_select(on_select)
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .hover(|st| st.bg(hover()))
            .active(|st| st.bg(hover()))
            .child(icon(glyph, 15.0, muted_fg))
            .child(
                text(label)
                    .text_size(px(13.0))
                    .line_height(px(13.0))
                    .text_color(fg()),
            )
    };
    menu()
        .offset(px(4.0))
        .popup_style(|st| {
            st.min_w(px(200.0))
                .p(px(4.0))
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
                .items_center()
                .justify_center()
                .label("更多")
                .hover(|st| st.bg(white_a(0.1)))
                .active(|st| st.opacity(0.6))
                .child(icon(Icon::More, 18.0, || white_a(0.7))),
        )
        .child(playlists::add_to_submenu(move || {
            path().into_iter().collect()
        }))
        .child(menu_separator().h(px(1.0)).my(px(4.0)).bg(border()))
        .child(item(
            "编辑元数据…",
            Icon::Edit,
            Box::new(move || {
                if let Some(path) = path() {
                    open_edit_info(&path);
                }
            }),
        ))
        .child(item(
            REVEAL_LABEL,
            Icon::Reveal,
            Box::new(move || {
                if let Some(path) = path() {
                    reveal(&path);
                }
            }),
        ))
        .child(menu_separator().h(px(1.0)).my(px(4.0)).bg(border()))
        .child(item(
            "拷贝文件路径",
            Icon::Copy,
            Box::new(move || {
                if let Some(path) = path() {
                    set_clipboard(path);
                }
            }),
        ))
        .into_element()
}
