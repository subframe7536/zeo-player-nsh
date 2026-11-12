use super::*;

/// 「添加到歌单」子菜单：新建歌单…，再列出全部手动歌单。`paths` 在选中时取。
pub fn add_to_submenu(paths: impl Fn() -> Vec<String> + Clone + 'static) -> AnyElement {
    let s = store();
    let create = paths.clone();
    submenu()
        .trigger(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(6.0))
                .role(AccessibilityRole::Menuitem)
                .label("添加到歌单")
                .hover(|st| st.bg(hover()))
                .child(icon(Icon::Plus, 15.0, muted_fg))
                .child(
                    text("添加到歌单")
                        .flex_1()
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(fg()),
                )
                .child(icon(Icon::ChevronRight, 12.0, faint)),
        )
        .child(item("新建歌单…", Some(Icon::Plus), move || {
            store::new_playlist(false, create())
        }))
        .child(dynamic(move || {
            let manual: Vec<Playlist> = s
                .playlists
                .with(|l| l.iter().filter(|p| !p.is_smart()).cloned().collect());
            let mut out = Vec::new();
            if !manual.is_empty() {
                out.push(separator());
            }
            for p in manual {
                let paths = paths.clone();
                let id = p.id;
                let glyph = if p.favorite {
                    Icon::Heart
                } else {
                    Icon::Playlist
                };
                out.push(item(p.name, Some(glyph), move || {
                    store::add_to_playlist(id, paths())
                }));
            }
            out
        }))
        .into_element()
}

/// 曲目的右键菜单（包着 `row`）：播放、添加到歌单、从歌单移除（`from`：
/// 当前所在的手动歌单）、在访达中显示、重命名文件（`rename`，文件夹页
/// 给）、删除记录 / 删除文件。
pub fn track_menu(
    row: impl IntoElement,
    track: Memo<Option<TrackEntry>>,
    from: Option<u64>,
    play: impl Fn() + 'static,
    rename: Option<Rc<dyn Fn(&str)>>,
) -> AnyElement {
    track_menu_selectable(row, track, from, play, rename, None)
}

/// 支持外部多选曲目集合的曲目右键菜单变体。
pub fn track_menu_selectable(
    row: impl IntoElement,
    track: Memo<Option<TrackEntry>>,
    from: Option<u64>,
    play: impl Fn() + 'static,
    rename: Option<Rc<dyn Fn(&str)>>,
    selected_paths: Option<Rc<dyn Fn() -> Vec<String>>>,
) -> AnyElement {
    let single_path = move || track.with_untracked(|t| t.as_ref().map(|t| t.path.clone()));
    let multi_paths = {
        let selected_paths = selected_paths.clone();
        move || {
            let single = single_path();
            if let Some(sel_fn) = &selected_paths {
                let list = sel_fn();
                if let Some(ref s) = single {
                    if list.contains(s) && list.len() > 1 {
                        return list;
                    }
                }
            }
            single.into_iter().collect::<Vec<_>>()
        }
    };
    let menu = context_menu().trigger(row);
    let single_items = {
        let multi_paths = multi_paths.clone();
        let single_path = single_path.clone();
        let rename = rename.clone();
        dynamic(move || {
            if multi_paths().len() > 1 {
                return Vec::new();
            }
            let mut list = Vec::new();
            list.push(separator());
            list.push(item("编辑元数据…", Some(Icon::Edit), {
                let single_path = single_path.clone();
                move || {
                    if let Some(p) = single_path() {
                        store::open_edit_info(&p);
                    }
                }
            }));
            list.push(item(zeo_shell::REVEAL_LABEL, Some(Icon::Reveal), {
                let single_path = single_path.clone();
                move || {
                    if let Some(p) = single_path() {
                        zeo_shell::reveal(&p);
                    }
                }
            }));
            if let Some(rename) = &rename {
                let rename = rename.clone();
                let single_path = single_path.clone();
                list.push(item("重命名文件…", Some(Icon::Edit), move || {
                    if let Some(p) = single_path() {
                        rename(&p);
                    }
                }));
            }
            list
        })
    };
    popup!(menu, 220.0)
        .child(item("播放", Some(Icon::Play), play))
        .child(separator())
        .child(add_to_submenu(multi_paths.clone()))
        .child(from.map(|id| {
            let multi_paths = multi_paths.clone();
            item("从歌单中移除", Some(Icon::Close), move || {
                let paths = multi_paths();
                store::later(move || {
                    for p in paths {
                        store::remove_from_playlist(id, &p);
                    }
                });
            })
        }))
        .child(single_items)
        .child(separator())
        .child(item("从曲库中删除", Some(Icon::Close), {
            let multi_paths = multi_paths.clone();
            move || {
                let paths = multi_paths();
                store::later(move || {
                    for p in paths {
                        store::remove_record(&p);
                    }
                });
            }
        }))
        .child(item("删除文件…", Some(Icon::Trash), {
            let multi_paths = multi_paths.clone();
            move || {
                let paths = multi_paths();
                if !paths.is_empty() {
                    store::ask_delete_files(paths);
                }
            }
        }))
        .into_element()
}
