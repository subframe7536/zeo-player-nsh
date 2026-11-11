use super::*;

/// 歌单页：头部（名称、种类与曲目数、播放 / 编辑 / 删除）+ 曲目列表。
pub fn page(id: u64) -> View {
    let s = store();
    let list = computed(move || s.playlists.with(|l| l.iter().find(|p| p.id == id).cloned()));
    let items = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(list.with(|p| p.as_ref().map(|p| p.tracks(&tracks)).unwrap_or_default()))
    });
    let smart = move || list.with(|p| p.as_ref().is_some_and(Playlist::is_smart));
    let favorite = move || list.with(|p| p.as_ref().is_some_and(|p| p.favorite));
    let title = move || list.with(|p| p.as_ref().map(|p| p.name.clone()).unwrap_or_default());
    let subtitle = move || {
        let kind = if smart() { "智能歌单" } else { "歌单" };
        items.with(|v| {
            let total: f64 = v.iter().map(|t| t.duration).sum();
            if v.is_empty() {
                kind.to_owned()
            } else {
                format!("{kind} · {} 首 · {}", v.len(), format_total(total))
            }
        })
    };
    // 「我喜欢」只有「播放」，不能改名或删除。
    let actions = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(when(
            move || items.with(|v| !v.is_empty()),
            move || {
                pill_button(Icon::Play, "播放", move || {
                    items.with_untracked(|v| play_list(v, 0))
                })
            },
        ))
        .child(when(
            move || !favorite(),
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        when(smart, move || {
                            outline_button(Icon::Edit, "编辑条件", move || {
                                store::edit_playlist(id)
                            })
                        })
                        .otherwise(move || {
                            outline_button(Icon::Edit, "重命名", move || {
                                store::edit_playlist(id)
                            })
                        }),
                    )
                    .child(icon_button(
                        || Icon::Trash,
                        16.0,
                        "删除歌单",
                        muted_fg,
                        move || confirm_delete(id),
                    ))
            },
        ));
    let header = view()
        .child(page_header(
            title,
            subtitle,
            None,
            Some(actions.into_element()),
        ))
        .child(track_columns(Columns::ALL));
    let empty = dynamic(move || {
        if smart() {
            empty_state(
                Icon::Smart,
                "没有符合条件的曲目",
                "调整条件后，符合的曲目会自动出现在这里。",
            )
            .child(view().mt(px(12.0)).child(outline_button(
                Icon::Edit,
                "编辑条件",
                move || store::edit_playlist(id),
            )))
        } else if favorite() {
            empty_state(
                Icon::Heart,
                "还没有喜欢的曲目",
                "点播放条上的心形按钮，喜欢的曲目会出现在这里。",
            )
        } else {
            empty_state(
                Icon::Playlist,
                "歌单是空的",
                "在任意曲目上点右键，选「添加到歌单」。",
            )
        }
    });
    let from = untracked(|| list.with(|p| p.as_ref().filter(|p| !p.is_smart()).map(|p| p.id)));
    view().flex_1().child(
        zeo_ui_views_base::track_list_in(items, Columns::ALL, header.into_element(), from)
            .empty(empty),
    )
}

// --- 编辑弹窗 -----------------------------------------------------------------
