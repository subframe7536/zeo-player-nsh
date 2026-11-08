//! 命令面板（Command Palette）：浮动快捷搜索弹窗。
//! 可搜索并快速导航页面、执行常用操作（播放/暂停、下一首、设置等）、以及搜索并直接播放曲库曲目。

use nsh_base::{CommandElement, CommandEntry, CommandItem, command};
use nsh_core::prelude::*;

use crate::ui::components::icons::{Icon, icon};
use crate::ui::theme::{self, *};
use crate::ui::{Nav, store};

fn matches_keyword(query: &str, label: &str, keywords: &[&str]) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    label.to_lowercase().contains(&q) || keywords.iter().any(|k| k.to_lowercase().contains(&q))
}

fn shortcut_pill(key: &'static str, label: &'static str) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .child(
            text(key)
                .text_size(px(10.0))
                .line_height(px(10.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(faint())
                .px(px(4.0))
                .py(px(2.0))
                .rounded(px(3.0))
                .border_1()
                .border_color(border().opacity(0.5)),
        )
        .child(
            text(label)
                .text_size(px(11.0))
                .line_height(px(11.0))
                .text_color(faint()),
        )
}

fn command_item_view(entry: &CommandEntry) -> View {
    let id = entry.id.to_string();
    let label = entry.label.to_string();
    let group = entry.group.as_deref().unwrap_or_default();

    if group == "曲目" || id.starts_with("track:") {
        let artist = entry
            .keywords
            .first()
            .map(|s| s.as_ref())
            .unwrap_or_default();
        let album = entry
            .keywords
            .get(1)
            .map(|s| s.as_ref())
            .unwrap_or_default();
        let dur = entry
            .keywords
            .get(3)
            .map(|s| s.as_ref())
            .unwrap_or_default();
        let sub = match (!artist.is_empty(), !album.is_empty()) {
            (true, true) => format!("{artist} · {album}"),
            (true, false) => artist.to_owned(),
            (false, true) => album.to_owned(),
            (false, false) => String::new(),
        };

        let mut info = view().flex_1().min_w(px(0.0)).gap(px(2.0)).child(
            text(label)
                .number_of_lines(1)
                .text_size(px(13.0))
                .line_height(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg()),
        );
        if !sub.is_empty() {
            info = info.child(
                text(sub)
                    .number_of_lines(1)
                    .text_size(px(11.0))
                    .line_height(px(11.0))
                    .text_color(muted_fg()),
            );
        }

        let mut item_view = view()
            .flex_1()
            .min_w(px(0.0))
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .child(
                view()
                    .size(px(28.0))
                    .rounded(px(6.0))
                    .bg(primary().opacity(0.08))
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .child(icon(Icon::Note, 14.0, faint)),
            )
            .child(info);

        if !dur.is_empty() {
            item_view = item_view.child(
                text(dur.to_owned())
                    .text_size(px(11.0))
                    .line_height(px(11.0))
                    .text_color(faint())
                    .flex_shrink_0(),
            );
        }

        item_view
    } else {
        let glyph = match id.as_str() {
            "nav:home" => Icon::Home,
            "nav:library" => Icon::Note,
            "nav:artists" => Icon::Mic,
            "nav:albums" => Icon::Disc,
            "nav:metadata" => Icon::Edit,
            "nav:folders" => Icon::Folder,
            "nav:genres" => Icon::Tag,
            "action:settings" => Icon::Settings,
            "action:play_pause" => Icon::Play,
            "action:next" => Icon::NextLine,
            "action:prev" => Icon::PrevLine,
            "action:scan" => Icon::Refresh,
            _ => Icon::Search,
        };

        let badge_text = if group == "页面" {
            "页面"
        } else {
            "操作"
        };

        view()
            .flex_1()
            .min_w(px(0.0))
            .h(px(32.0))
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .child(
                view()
                    .size(px(28.0))
                    .rounded(px(6.0))
                    .bg(primary().opacity(0.08))
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .child(icon(glyph, 14.0, faint)),
            )
            .child(
                text(label)
                    .flex_1()
                    .min_w(px(0.0))
                    .number_of_lines(1)
                    .text_size(px(13.0))
                    .line_height(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg()),
            )
            .child(
                view()
                    .flex_shrink_0()
                    .child(if let Some(sc) = &entry.shortcut_label {
                        text(sc.clone())
                            .text_size(px(10.0))
                            .line_height(px(10.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(faint())
                            .px(px(5.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(border().opacity(0.5))
                            .into_element()
                    } else {
                        text(badge_text)
                            .text_size(px(10.0))
                            .line_height(px(10.0))
                            .text_color(faint())
                            .px(px(5.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(primary().opacity(0.06))
                            .into_element()
                    }),
            )
    }
}

fn palette_content(close: impl Fn() + Copy + 'static) -> CommandElement {
    let s = store::store();
    let query = signal(String::new());

    let matching_tracks = computed(move || {
        let q = query.get();
        let q_trimmed = q.trim();
        if q_trimmed.is_empty() {
            return Vec::new();
        }
        let all = s.tracks.get();
        let hits = super::search_hits(&q_trimmed.to_lowercase(), &all);
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

/// 命令面板弹窗（nsh-base `dialog`，Esc / 点遮罩关闭）。
pub fn dialog() -> AnyElement {
    let close = move || {
        let s = store::store();
        s.search_open.set(false);
        s.search.set(String::new());
    };

    let content = view()
        .w(px(580.0))
        .max_w(move || px((window().width.0 - 32.0).max(240.0)))
        .max_h(move || px((window().height.0 - 120.0).max(240.0)))
        .rounded(px(12.0))
        .border_1()
        .border_color(border())
        .bg(move || nsh_base::theme().card)
        .shadow_xl()
        .overflow_hidden()
        .on_press(|| {})
        .child(palette_content(close));

    nsh_base::dialog()
        .animation(ModalAnimation::Fade)
        .on_cancel(move || close())
        .child(
            view()
                .absolute()
                .inset(px(0.0))
                .bg(black_a(0.5))
                .items_center()
                .pt(px(80.0))
                .on_press(move || close())
                .child(content),
        )
        .into_element()
}
