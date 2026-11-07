//! 路由页：曲库（文件夹 / 歌手 / 专辑 / 流派）、歌单、设置。跨页共享的
//! （曲目列表、详情页头、空曲库、路径工具、搜索结果）在本文件。

pub(super) mod browse;
mod edit_info;
pub(super) mod folders;
mod home;
pub(super) mod playlists;
mod settings;

use std::rc::Rc;

use nsh_base::{menu_item, menu_separator};
use nsh_core::prelude::*;

use super::components::button::pill_button;
use super::components::icons::{Icon, icon};
use super::components::page::{empty_state, page_header};
use super::components::tracks::{Columns, ROW, track_columns, track_row};
use super::store::{play_list, store};
use super::theme::*;
use crate::config::{TrackEntry, library_store};

pub(super) use self::browse::{albums, artists, genres, tags_sidebar_list};
pub(super) use self::edit_info::edit_info_dialog;
pub(super) use self::folders::{folders, tree_sidebar_list};
pub(super) use self::home::home;
pub(super) use self::settings::dialog as settings_dialog;

// --- 菜单部件 -----------------------------------------------------------------

/// 菜单弹层的样子（下拉、右键菜单、子菜单共用）。
macro_rules! popup {
    ($menu:expr, $min:expr) => {
        popup!($menu, $min, 420.0)
    };
    ($menu:expr, $min:expr, $max_h:expr) => {
        $menu.popup_style(|st| {
            st.min_w(px($min))
                .max_h(px($max_h))
                .p(px(4.0))
                .rounded(px(10.0))
                .overflow_hidden()
                .border_1()
                .border_color(border())
                .bg(nsh_base::theme().popover)
                .shadow_lg()
        })
    };
}
pub(crate) use popup;

/// 菜单项的一行：图标（可无）+ 文字。
pub(crate) fn row_content(glyph: Option<Icon>, label: impl Fn() -> String + 'static) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(6.0))
        .hover(|st| st.bg(hover()))
        .active(|st| st.bg(hover()))
        .child(glyph.map(|g| icon(g, 15.0, muted_fg)))
        .child(
            text(label)
                .flex_1()
                .number_of_lines(1)
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(fg()),
        )
}

/// 一个菜单项。
pub(crate) fn item(
    label: impl Into<String>,
    glyph: Option<Icon>,
    on_select: impl Fn() + 'static,
) -> AnyElement {
    let label: String = label.into();
    let shown = label.clone();
    menu_item()
        .accessibility_label(SharedString::from(label))
        .on_select(on_select)
        .child(row_content(glyph, move || shown.clone()))
        .into_element()
}

pub(crate) fn separator() -> AnyElement {
    menu_separator()
        .h(px(1.0))
        .my(px(4.0))
        .bg(border())
        .into_element()
}

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
        "添加一个包含音乐文件（MP3、FLAC、M4A、OGG 等）的文件夹，Zeo Player 会扫描并整理它们。",
    )
    .child(
        view()
            .mt(px(12.0))
            .child(pill_button(Icon::Plus, "添加文件夹…", || {
                settings::open(settings::Pane::Library)
            })),
    )
}

/// 曲目列表（虚拟化），`header` 随内容滚动。序号列（`cols.number`，
/// 只有专辑详情开）显示标签曲目号，没有该列时 `number` 不会被读。
fn track_list(items: Memo<Rc<Vec<TrackEntry>>>, cols: Columns, header: AnyElement) -> VirtualList {
    list_of_tracks(items, cols, header, None)
}

/// 歌单页的曲目列表：`from` 为手动歌单时右键菜单可从中移除。
pub(super) fn track_list_in(
    items: Memo<Rc<Vec<TrackEntry>>>,
    cols: Columns,
    header: AnyElement,
    from: Option<u64>,
) -> VirtualList {
    list_of_tracks(items, cols, header, from)
}

fn list_of_tracks(
    items: Memo<Rc<Vec<TrackEntry>>>,
    cols: Columns,
    header: AnyElement,
    from: Option<u64>,
) -> VirtualList {
    let count = computed(move || items.with(|v| v.len()));
    virtual_list(count, move |at| {
        let track = computed(move || items.with(|v| v.get(at.index()).cloned()));
        let number = computed(move || {
            let index = at.index();
            if cols.number {
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
        playlists::track_menu(row, track, from, move || play(), None)
    })
    .estimated_item_size(px(ROW))
    .header(header)
    .content_padding_bottom(px(BAR_SPACE))
    .flex_1()
}

/// 详情页的大头部：封面（或头像）、类别、标题、说明、播放按钮。
/// `art` 自己带阴影等样式；「在列表中显示封面」关掉时传 `when` 空位。
fn hero(
    eyebrow: &'static str,
    art: impl IntoElement,
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
                .child(art)
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
                                .text_color(move || primary())
                                .transition_colors()
                                .duration(300.0),
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

/// 搜索命中：标题 / 歌手 / 专辑 / 文件名包含 `query`（已转小写）的曲目。
/// 空串返回空。侧栏结果页与搜索弹窗共用。
pub(crate) fn search_hits(query: &str, tracks: &[TrackEntry]) -> Vec<TrackEntry> {
    if query.is_empty() {
        return Vec::new();
    }
    tracks
        .iter()
        .filter(|t| {
            [&t.title, &t.artist, &t.album, &t.file_name]
                .iter()
                .any(|f| f.to_lowercase().contains(query))
        })
        .cloned()
        .collect()
}

/// 搜索页的结果列表：标题 / 歌手 / 专辑 / 文件名匹配（不分大小写）。
/// 点一首播放时，播放队列就是这份结果列表。页顶的输入框在 `page()` 里
/// （放在 `dynamic` 外面，输入时不重建）。
pub fn search_results() -> View {
    let s = store();
    let items = computed(move || {
        let query = s.search.with(|q| q.trim().to_lowercase());
        let tracks = s.tracks.get();
        Rc::new(search_hits(&query, &tracks))
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
        track_list(items, Columns::ALL, header.into_element()).empty(empty_state(
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

// --- 拖放进曲库 ---------------------------------------------------------------

/// `file://` URL → 路径（百分号解码）。Windows 的 `file:///C:/Music` 去掉
/// 盘符前的 `/`，并换成 `\\`。
pub(super) fn url_to_path(uri: &str) -> String {
    let path = decode_url_path(uri);
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        return path[1..].replace('/', "\\");
    }
    path
}

fn decode_url_path(uri: &str) -> String {
    let rest = uri.strip_prefix("file://").unwrap_or(uri);
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `add_dropped` 的返回值。
pub(super) struct DroppedPath {
    /// 给用户看的一行结果。
    pub note: String,
    /// 拖入内容所在（或新加）的目录——文件夹页据此定位。
    pub dir: Option<String>,
    /// 是否真的新加了根目录（调用方据此决定要不要重扫）。
    pub added: bool,
}

/// `dir` 是否已在某个曲库根目录下（含根本身）。
fn under_root(dir: &str, roots: &[String]) -> bool {
    roots.iter().any(|r| {
        let root = r.trim_end_matches(SEP);
        dir == root || dir.starts_with(&format!("{root}{SEP}"))
    })
}

/// 拖进来的本地路径进曲库：目录已在某个根目录下就只定位过去（不重复加
/// 嵌套的根），否则加为新根；文件取所在目录，规则一样。不重扫——调用
/// 方处理完所有路径后统一 [`super::store::scan`]。
pub(super) fn add_dropped(path: &str) -> DroppedPath {
    let trimmed = path.trim().trim_end_matches(['/', '\\']);
    let canonical = match dunce::canonicalize(trimmed) {
        Ok(c) => c,
        Err(error) => {
            return DroppedPath {
                note: format!("无法访问 {trimmed}: {error}"),
                dir: None,
                added: false,
            };
        }
    };
    let lib = library_store();
    let dir = if canonical.is_dir() {
        canonical
    } else {
        match canonical.parent() {
            Some(p) => p.to_path_buf(),
            None => {
                return DroppedPath {
                    note: format!("{trimmed} 不是目录"),
                    dir: None,
                    added: false,
                };
            }
        }
    };
    let dir = dir.to_string_lossy().into_owned();
    if under_root(&dir, &lib.roots()) {
        return DroppedPath {
            note: format!("已在曲库中 {dir}"),
            dir: Some(dir),
            added: false,
        };
    }
    // 磁盘根目录（`C:\`）不加：拖根目录下的文件进来不该扫整块盘。
    if std::path::Path::new(&dir).parent().is_none() {
        return DroppedPath {
            note: format!("{dir} 是磁盘根目录，不加入曲库"),
            dir: None,
            added: false,
        };
    }
    match lib.add_root(&dir) {
        Ok(_) => DroppedPath {
            note: format!("已添加 {dir}"),
            dir: Some(dir),
            added: true,
        },
        Err(error) => DroppedPath {
            note: error,
            dir: None,
            added: false,
        },
    }
}
