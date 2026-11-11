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

            // 1. 页面导航（键位标签读 `s.keybinds`，换绑即更新）。
            let nav_defs = [
                (
                    "nav:home",
                    "首页",
                    &["home", "shouye"][..],
                    KeyAction::NavHome,
                ),
                (
                    "nav:library",
                    "音乐库",
                    &["library", "music", "yinyueku", "gequ", "歌曲"][..],
                    KeyAction::NavLibrary,
                ),
                (
                    "nav:artists",
                    "艺术家",
                    &["artist", "singer", "yishujia", "geshou", "歌手"][..],
                    KeyAction::NavArtists,
                ),
                (
                    "nav:albums",
                    "专辑",
                    &["album", "zhuanji"][..],
                    KeyAction::NavAlbums,
                ),
                (
                    "nav:metadata",
                    "元数据",
                    &["metadata", "tag", "yuanshuju", "biaoqian", "标签"][..],
                    KeyAction::NavMetadata,
                ),
                (
                    "nav:folders",
                    "文件夹",
                    &["folder", "wenjianjia", "mulu", "目录"][..],
                    KeyAction::NavFolders,
                ),
                (
                    "nav:genres",
                    "标签 / 流派",
                    &["tag", "genre", "liupai"][..],
                    KeyAction::NavGenres,
                ),
            ];
            for (id, label, kws, action) in nav_defs {
                if matches_keyword(&q_lower, label, kws) {
                    let mut it = CommandItem::new(id, label)
                        .group("页面")
                        .keywords(kws.iter().copied());
                    let key = store::key_of(action);
                    if !key.is_empty() {
                        it = it.shortcut(key);
                    }
                    list.push(it);
                }
            }

            // 2. 常用操作
            let is_playing = store::shown_playing();
            let action_defs = [
                (
                    "action:settings",
                    "偏好设置",
                    &["settings", "shezhi", "preferences", "config"][..],
                    Some(KeyAction::SettingsOpen),
                ),
                (
                    "action:play_pause",
                    if is_playing {
                        "暂停播放"
                    } else {
                        "开始播放"
                    },
                    &["play", "pause", "bofang", "zanting"][..],
                    Some(KeyAction::Toggle),
                ),
                (
                    "action:next",
                    "下一首",
                    &["next", "xiayishou"][..],
                    Some(KeyAction::Next),
                ),
                (
                    "action:prev",
                    "上一首",
                    &["prev", "previous", "shangyishou"][..],
                    Some(KeyAction::Prev),
                ),
                (
                    "action:like",
                    "喜欢当前曲目",
                    &["like", "favorite", "xihuan", "shoucang", "喜欢"][..],
                    Some(KeyAction::Like),
                ),
                (
                    "action:loop",
                    "切换循环模式",
                    &["loop", "repeat", "shuffle", "xunhuan", "suiji"][..],
                    Some(KeyAction::LoopMode),
                ),
                (
                    "action:mute",
                    "静音 / 取消静音",
                    &["mute", "volume", "jingyin", "yinliang"][..],
                    Some(KeyAction::Mute),
                ),
                (
                    "action:fullscreen",
                    "全屏播放",
                    &["fullscreen", "nowplaying", "quanpin", "bofangye"][..],
                    Some(KeyAction::FullScreen),
                ),
                (
                    "action:scan",
                    "扫描更新曲库",
                    &["scan", "refresh", "rescan", "saomiao", "gengxin"][..],
                    None,
                ),
            ];
            for (id, label, kws, action) in action_defs {
                if matches_keyword(&q_lower, label, kws) {
                    let mut it = CommandItem::new(id, label)
                        .group("操作")
                        .keywords(kws.iter().copied());
                    if let Some(key) = action.map(store::key_of).filter(|k| !k.is_empty()) {
                        it = it.shortcut(key);
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
                match target {
                    "home" => store::navigate(Nav::Home),
                    "library" => store::navigate(Nav::Library),
                    "artists" => store::navigate(Nav::Artists),
                    "albums" => store::navigate(Nav::Albums),
                    "metadata" => store::navigate(Nav::Metadata),
                    "folders" => store::navigate(Nav::Folders),
                    "genres" => store::navigate(Nav::Genres),
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
                    "like" => store::toggle_like_current(),
                    "loop" => store::cycle_loop_mode(),
                    "mute" => store::toggle_mute(),
                    "fullscreen" => {
                        s.full_screen.set(true);
                    }
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
