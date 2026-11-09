use super::*;

pub(crate) fn palette_content(close: impl Fn() + Copy + 'static) -> CommandElement {
    let s = store::store();
    let query = signal(String::new());

    let matching_tracks = computed(move || {
        let q = query.get();
        let q_trimmed = q.trim();
        if q_trimmed.is_empty() {
            return Vec::new();
        }
        let all = s.tracks.get();
        let hits = zeo_ui_views_base::search_hits(&q_trimmed.to_lowercase(), &all);
        let limit = hits.len().min(60);
        hits[..limit].to_vec()
    });

    let items = computed({
        let matching_tracks = matching_tracks.clone();
        move || {
            let q = query.get();
            let q_trimmed = q.trim();
            let q_lower = q_trimmed.to_lowercase();
            let mut list: Vec<CommandItem> = Vec::new();

            // 1. 页面导航
            let nav_defs = [
                ("nav:home", "首页", &["home", "shouye"][..]),
                (
                    "nav:library",
                    "音乐库",
                    &["library", "music", "yinyueku", "gequ", "歌曲"][..],
                ),
                (
                    "nav:artists",
                    "艺术家",
                    &["artist", "singer", "yishujia", "geshou", "歌手"][..],
                ),
                ("nav:albums", "专辑", &["album", "zhuanji"][..]),
                (
                    "nav:metadata",
                    "元数据",
                    &["metadata", "tag", "yuanshuju", "biaoqian", "标签"][..],
                ),
                (
                    "nav:folders",
                    "文件夹",
                    &["folder", "wenjianjia", "mulu", "目录"][..],
                ),
                ("nav:genres", "标签 / 流派", &["tag", "genre", "liupai"][..]),
            ];
            for (id, label, kws) in nav_defs {
                if matches_keyword(&q_lower, label, kws) {
                    list.push(
                        CommandItem::new(id, label)
                            .group("页面")
                            .keywords(kws.iter().copied()),
                    );
                }
            }

            // 2. 常用操作
            let is_playing = store::shown_playing();
            let action_defs = [
                (
                    "action:settings",
                    "偏好设置",
                    &["settings", "shezhi", "preferences", "config"][..],
                    Some("mod+,"),
                ),
                (
                    "action:play_pause",
                    if is_playing {
                        "暂停播放"
                    } else {
                        "开始播放"
                    },
                    &["play", "pause", "bofang", "zanting"][..],
                    Some("space"),
                ),
                ("action:next", "下一首", &["next", "xiayishou"][..], None),
                (
                    "action:prev",
                    "上一首",
                    &["prev", "previous", "shangyishou"][..],
                    None,
                ),
                (
                    "action:scan",
                    "扫描更新曲库",
                    &["scan", "refresh", "rescan", "saomiao", "gengxin"][..],
                    None,
                ),
            ];
            for (id, label, kws, shortcut) in action_defs {
                if matches_keyword(&q_lower, label, kws) {
                    let mut it = CommandItem::new(id, label)
                        .group("操作")
                        .keywords(kws.iter().copied());
                    if let Some(sc) = shortcut {
                        it = it.shortcut(sc);
                    }
                    list.push(it);
                }
            }

            // 3. 曲目搜索
            let tracks = matching_tracks.get();
            for t in tracks.iter() {
                let title = if t.title.is_empty() {
                    t.file_name.clone()
                } else {
                    t.title.clone()
                };
                list.push(
                    CommandItem::new(format!("track:{}", t.path), title)
                        .group("曲目")
                        .keywords(vec![
                            t.artist.clone(),
                            t.album.clone(),
                            t.file_name.clone(),
                            store::format_time(t.duration),
                        ]),
                );
            }

            list
        }
    });

    let item_count = computed(move || items.with(|it| it.len()));

    let footer_view = view()
        .h(px(34.0))
        .px(px(14.0))
        .border_t_1()
        .border_t_color(border())
        .bg(theme::muted().opacity(0.2))
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(shortcut_pill("↑↓", "导航"))
                .child(shortcut_pill("↵", "选择"))
                .child(shortcut_pill("Esc", "关闭")),
        )
        .child(dynamic(move || {
            let count = item_count.get();
            if count > 0 {
                text(format!("{count} 项结果"))
                    .text_size(px(11.0))
                    .line_height(px(11.0))
                    .text_color(faint())
                    .into_element()
            } else {
                view().into_element()
            }
        }));

    let empty_view = view()
        .py(px(36.0))
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(icon(Icon::Search, 22.0, faint))
        .child(
            text("未找到匹配的结果")
                .text_size(px(13.0))
                .line_height(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted_fg()),
        )
        .child(
            text("尝试搜索曲目标题、歌手、专辑、页面或操作")
                .text_size(px(11.0))
                .line_height(px(11.0))
                .text_color(faint()),
        );

    let on_select = {
        let matching_tracks = matching_tracks.clone();
        move |id: &SharedString| {
            let s = store::store();
            let id_str = id.as_ref();
            if let Some(target) = id_str.strip_prefix("nav:") {
                let nav = global::<Nav>();
                match target {
                    "home" => nav.set(Nav::Home),
                    "library" => nav.set(Nav::Library),
                    "artists" => nav.set(Nav::Artists),
                    "albums" => nav.set(Nav::Albums),
                    "metadata" => nav.set(Nav::Metadata),
                    "folders" => nav.set(Nav::Folders),
                    "genres" => nav.set(Nav::Genres),
                    _ => {}
                }
                close();
            } else if let Some(action) = id_str.strip_prefix("action:") {
                match action {
                    "settings" => {
                        s.settings.set(true);
                    }
                    "play_pause" => store::toggle(),
                    "next" => store::player().next(),
                    "prev" => store::player().previous(),
                    "scan" => store::scan(),
                    _ => {}
                }
                close();
            } else if let Some(path_str) = id_str.strip_prefix("track:") {
                let list = matching_tracks.get();
                if let Some(pos) = list.iter().position(|t| t.path == path_str) {
                    store::play_list(&list, pos);
                } else {
                    let all = s.tracks.get();
                    if let Some(pos) = all.iter().position(|t| t.path == path_str) {
                        store::play_list(&all, pos);
                    }
                }
                close();
            }
        }
    };

    command()
        .auto_focus(true)
        .placeholder(Some(SharedString::from(
            "搜索音乐、歌手、专辑、页面与操作...",
        )))
        .accessibility_label(Some(SharedString::from("命令面板")))
        .search_prefix(
            icon(Icon::Search, 15.0, faint)
                .flex_shrink_0()
                .ml(px(14.0))
                .mr(px(2.0)),
        )
        .search_bar_style(|st| {
            st.h(px(46.0))
                .flex_row()
                .items_center()
                .border_b_1()
                .border_b_color(border())
                .bg(nsh_base::theme().card)
        })
        .search_style(move |st| {
            st.flex_1()
                .min_w(px(0.0))
                .h(px(44.0))
                .px(px(8.0))
                .text_size(px(14.0))
                .text_color(fg())
        })
        .list_style(|st| st.max_h(px(360.0)).p(px(6.0)))
        .item_class("focus:bg-primary/10 rounded-lg")
        .item_style(|st| {
            st.px(px(8.0))
                .py(px(6.0))
                .rounded(px(8.0))
                .cursor(Cursor::Pointer)
        })
        .group_style(|st| {
            st.px(px(10.0))
                .pt(px(10.0))
                .pb(px(4.0))
                .text_size(px(11.0))
                .line_height(px(11.0))
                .text_color(faint())
                .font_weight(FontWeight::SEMIBOLD)
        })
        .separator_style(|st| st.h(px(1.0)).my(px(4.0)).bg(border().opacity(0.5)))
        .filterable(false)
        .items(items)
        .on_query_change(move |q: &SharedString| query.set(q.to_string()))
        .on_select(on_select)
        .on_cancel(move || close())
        .render_item(move |entry: &CommandEntry| command_item_view(entry))
        .empty(empty_view)
        .footer(footer_view)
}
