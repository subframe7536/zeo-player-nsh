use super::*;

// --- 侧栏 ---------------------------------------------------------------------

/// 「新建歌单」菜单（侧栏歌单区表头「+」的弹层内容）。
pub fn add_playlist_menu(trigger: View) -> AnyElement {
    let gate = store::menu_control();
    let add = menu()
        .offset(px(4.0))
        .trigger(trigger)
        .open(gate.open())
        .on_open_change(gate.menu_change());
    popup!(add, 200.0)
        .child(item("新建歌单", Some(Icon::Playlist), || {
            store::new_playlist(false, Vec::new())
        }))
        .child(item("新建智能歌单", Some(Icon::Smart), || {
            store::new_playlist(true, Vec::new())
        }))
        .into_element()
}

/// 侧栏的歌单区：小标题 + 新建按钮，下面是歌单列表（可滚动）。
/// 添加歌单的按钮（带下拉菜单）。
pub fn add_playlist_button() -> AnyElement {
    add_playlist_menu(
        view()
            .size(px(22.0))
            .rounded(px(6.0))
            .items_center()
            .justify_center()
            .role(AccessibilityRole::Button)
            .label("添加歌单")
            .window_drag(false)
            .hover(|st| st.bg(move || primary().opacity(0.08)))
            .active(|st| st.opacity(0.6))
            .child(icon(Icon::Plus, 14.0, faint)),
    )
}

/// 侧栏歌单列表（滚动区）。
pub fn sidebar_list() -> impl IntoElement {
    let s = store();
    scroll()
        .flex_1()
        .child(
            view()
                .px(px(10.0))
                .pb(px(8.0))
                .gap(px(2.0))
                .child(dynamic(move || {
                    let lists = s.playlists.get();
                    // 「我喜欢」固定在最前（`ensure_favorites`），其余按创建顺序。
                    let mut rows: Vec<AnyElement> = lists.iter().map(|p| sidebar_row(p)).collect();
                    if lists.iter().all(|p| p.favorite) {
                        rows.push(
                            text("点 + 新建歌单，或在曲目上点右键添加。")
                                .text_size(px(12.0))
                                .line_height(px(18.0))
                                .text_color(faint())
                                .px(px(10.0))
                                .py(px(4.0))
                                .into_element(),
                        );
                    }
                    rows
                })),
        )
}

pub(crate) fn sidebar_row(p: &Playlist) -> AnyElement {
    let (id, name, smart, favorite) = (p.id, p.name.clone(), p.is_smart(), p.favorite);
    let nav = global::<Nav>();
    let active = move || nav.get() == Nav::Playlist(id);
    let glyph = if favorite {
        Icon::Heart
    } else if smart {
        Icon::Smart
    } else {
        Icon::Playlist
    };
    // 按钮不能挂 `window_drag(false)`（`Interactive` 不在组件元素上），
    // 同 `page_header` 的返回键：裸 view 包一层。
    let row = view()
        .window_drag(false)
        .child(zeo_ui_components::nav::nav_row(
            glyph,
            name.clone().into(),
            active,
            move || {
                store().search.set(String::new());
                store().search_open.set(false);
                store::navigate(Nav::Playlist(id));
            },
        ));
    // 「我喜欢」不能改名或删除：没有右键菜单。
    if favorite {
        return row.into_element();
    }
    let gate = store::menu_control();
    let menu = context_menu()
        .trigger(row)
        .open(gate.open())
        .on_open_change(gate.menu_change());
    popup!(menu, 180.0)
        .child(item(
            if smart {
                "编辑条件…"
            } else {
                "重命名…"
            },
            Some(Icon::Edit),
            move || store::edit_playlist(id),
        ))
        .child(separator())
        .child(item("删除歌单", Some(Icon::Trash), move || {
            confirm_delete(id)
        }))
        .into_element()
}

/// 删除前确认（系统对话框）。
pub fn confirm_delete(id: u64) {
    let Some(p) = store::playlist(id) else { return };
    let body = if p.is_smart() {
        "只删除歌单本身，曲库里的文件不受影响。".to_owned()
    } else {
        "歌单里的曲目只会从歌单中移除，文件不受影响。".to_owned()
    };
    let answer = nsh_base::alert_dialog()
        .title(format!("删除「{}」？", p.name))
        .description(body)
        .cancel("cancel", "取消")
        .destructive("delete", "删除")
        .show();
    spawn(async move {
        if answer.await.map(|r| r.action) == Ok(Some("delete".into())) {
            store::delete_playlist(id);
        }
    });
}

// --- 歌单页 -------------------------------------------------------------------
