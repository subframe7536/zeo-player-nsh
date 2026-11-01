//! 路由页：曲库（文件夹 / 歌手 / 专辑 / 流派）、歌单、设置。跨页共享的
//! （曲目列表、详情页头、空曲库、路径工具、搜索结果）在本文件。

mod browse;
mod folders;
pub(super) mod playlists;
mod settings;

use std::rc::Rc;

use nsh_core::prelude::*;

use super::Nav;
use super::components::button::pill_button;
use super::components::icons::{Icon, icon};
use super::components::page::{empty_state, page_header};
use super::components::tracks::{Columns, ROW, track_columns, track_row};
use super::store::{play_list, store};
use super::theme::*;
use crate::config::TrackEntry;

pub(super) use self::browse::{albums, artists, genres};
pub(super) use self::folders::folders;
pub(super) use self::settings::settings;

fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

fn by_disc_track(a: &TrackEntry, b: &TrackEntry) -> std::cmp::Ordering {
    (a.disk.unwrap_or(0), a.track.unwrap_or(0))
        .cmp(&(b.disk.unwrap_or(0), b.track.unwrap_or(0)))
        .then_with(|| compare(&a.title, &b.title))
}

fn total_duration(tracks: &[TrackEntry]) -> f64 {
    tracks.iter().map(|t| t.duration).sum()
}

/// 曲库为空时：去设置添加文件夹。
fn no_library() -> View {
    empty_state(
        Icon::Folder,
        "还没有音乐文件夹",
        "添加一个包含 MP3、FLAC 或 WAV 的文件夹，Zeo Player 会扫描并整理它们。",
    )
    .child(
        view()
            .mt(px(12.0))
            .child(pill_button(Icon::Plus, "添加文件夹…", || {
                global::<Nav>().set(Nav::Settings)
            })),
    )
}

/// 曲目列表（虚拟化），`header` 随内容滚动。
fn track_list(
    items: Memo<Rc<Vec<TrackEntry>>>,
    use_track_numbers: bool,
    cols: Columns,
    header: AnyElement,
) -> VirtualList {
    list_of_tracks(items, use_track_numbers, cols, header, None)
}

/// 歌单页的曲目列表：`from` 为手动歌单时右键菜单可从中移除。
pub(super) fn track_list_in(
    items: Memo<Rc<Vec<TrackEntry>>>,
    cols: Columns,
    header: AnyElement,
    from: Option<u64>,
) -> VirtualList {
    list_of_tracks(items, false, cols, header, from)
}

fn list_of_tracks(
    items: Memo<Rc<Vec<TrackEntry>>>,
    use_track_numbers: bool,
    cols: Columns,
    header: AnyElement,
    from: Option<u64>,
) -> VirtualList {
    let count = computed(move || items.with(|v| v.len()));
    virtual_list(count, move |at| {
        let track = computed(move || items.with(|v| v.get(at.index()).cloned()));
        let number = computed(move || {
            let index = at.index();
            if use_track_numbers {
                track.with(|t| {
                    t.as_ref()
                        .and_then(|t| t.track)
                        .map(|n| n.to_string())
                        .unwrap_or_default()
                })
            } else {
                (index + 1).to_string()
            }
        });
        let play =
            Rc::new(move || items.with_untracked(|v| play_list(v, untracked(|| at.index()))));
        let row = track_row(track, number, cols, {
            let play = play.clone();
            move || play()
        })
        .mx(px(20.0));
        playlists::track_menu(row, track, from, move || play())
    })
    .estimated_item_size(px(ROW))
    .header(header)
    .content_padding_bottom(px(BAR_SPACE))
    .flex_1()
}

/// 详情页的大头部：封面（或头像）、类别、标题、说明、播放按钮。
fn hero(
    eyebrow: &'static str,
    art: View,
    title: String,
    subtitle: String,
    meta: String,
    back: Rc<dyn Fn()>,
    play: impl Fn() + 'static,
) -> View {
    view()
        .px(px(32.0))
        .pt(px(12.0))
        .pb(px(20.0))
        .gap(px(16.0))
        .window_drag(true)
        .child(
            view().self_start().ml(px(-8.0)).window_drag(false).child(
                nsh_base::button()
                    .flex_row()
                    .items_center()
                    .gap(px(2.0))
                    .h(px(28.0))
                    .pl(px(4.0))
                    .pr(px(10.0))
                    .rounded(px(8.0))
                    .accessibility_label("返回")
                    .hover(|s| s.bg(hover()))
                    .active(|s| s.opacity(0.6))
                    .on_press(move |_: &()| back())
                    .child(icon(Icon::ChevronLeft, 16.0, muted_fg))
                    .child(
                        text("返回")
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(muted_fg()),
                    ),
            ),
        )
        .child(
            view()
                .flex_row()
                .items_end()
                .gap(px(24.0))
                .child(art.shadow_xl())
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(6.0))
                        .child(
                            text(eyebrow)
                                .text_size(px(11.0))
                                .line_height(px(11.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .letter_spacing(px(0.8))
                                .text_color(primary()),
                        )
                        .child(
                            text(title)
                                .number_of_lines(2)
                                .text_size(px(32.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(fg()),
                        )
                        .child(
                            text(subtitle)
                                .number_of_lines(1)
                                .text_size(px(15.0))
                                .line_height(px(15.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(fg()),
                        )
                        .child(
                            text(meta)
                                .number_of_lines(1)
                                .text_size(px(13.0))
                                .line_height(px(13.0))
                                .text_color(muted_fg()),
                        )
                        .child(
                            view()
                                .flex_row()
                                .gap(px(10.0))
                                .mt(px(10.0))
                                .window_drag(false)
                                .child(pill_button(Icon::Play, "播放", play)),
                        ),
                ),
        )
}
// --- 搜索 ---------------------------------------------------------------------

/// 标题栏搜索的结果页：标题 / 歌手 / 专辑 / 文件名匹配（不分大小写）。
/// 点一首播放时，播放队列就是这份结果列表。
pub fn search_results() -> View {
    let s = store();
    let items = computed(move || {
        let query = s.search.with(|q| q.trim().to_lowercase());
        let tracks = s.tracks.get();
        let hits: Vec<TrackEntry> = if query.is_empty() {
            Vec::new()
        } else {
            tracks
                .iter()
                .filter(|t| {
                    [&t.title, &t.artist, &t.album, &t.file_name]
                        .iter()
                        .any(|f| f.to_lowercase().contains(&query))
                })
                .cloned()
                .collect()
        };
        Rc::new(hits)
    });
    let count = computed(move || items.with(|v| v.len()));
    let header = view()
        .child(page_header(
            || "搜索".into(),
            move || format!("{} 个结果", count.get()),
            None,
            None,
        ))
        .child(track_columns(Columns::ALL));
    view().flex_1().child(
        track_list(items, false, Columns::ALL, header.into_element()).empty(empty_state(
            Icon::Search,
            "没有匹配的曲目",
            "搜索匹配曲目的标题、歌手、专辑和文件名。",
        )),
    )
}

// --- 路径 ---------------------------------------------------------------------


/// 路径分隔符：曲库路径来自文件系统，Windows 上是 `\\`。
pub(crate) const SEP: char = std::path::MAIN_SEPARATOR;

/// 路径最后一段（文件夹名）；`C:\\` 这样的盘符根本身即名字。
pub(crate) fn last_segment(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(path)
}

