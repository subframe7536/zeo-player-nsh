//! 曲库四视图（文件夹 / 歌手 / 专辑 / 标签）与设置。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use nsh_core::element::input;
use nsh_core::prelude::*;

use super::icons::{icon, Icon};
use super::parts::*;
use super::store::{format_total, play_list, scan, store, cover_of, Same};
use super::theme::*;
use super::Nav;
use crate::library::{library_store, TrackEntry};

// --- 共用 ---------------------------------------------------------------------

/// 专辑详情：头部已有封面与专辑名。
const ALBUM_COLS: Columns = Columns { cover: false, album: false, size: false, format: false };
/// 文件夹：面向文件，带大小列。
const FOLDER_COLS: Columns = Columns { cover: true, album: true, size: true, format: true };

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
    empty_state(Icon::Folder, "还没有音乐文件夹", "添加一个包含 MP3、FLAC 或 WAV 的文件夹，Zeo Player 会扫描并整理它们。")
        .child(
            view()
                .mt(px(12.0))
                .child(pill_button(Icon::Plus, "添加文件夹…", || global::<Nav>().set(Nav::Settings))),
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
                track.with(|t| t.as_ref().and_then(|t| t.track).map(|n| n.to_string()).unwrap_or_default())
            } else {
                (index + 1).to_string()
            }
        });
        let play = Rc::new(move || items.with_untracked(|v| play_list(v, untracked(|| at.index()))));
        let row = track_row(track, number, cols, {
            let play = play.clone();
            move || play()
        })
        .mx(px(20.0));
        super::playlists::track_menu(row, track, from, move || play())
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
            view()
                .flex_row()
                .items_center()
                .gap(px(2.0))
                .self_start()
                .h(px(28.0))
                .pl(px(4.0))
                .pr(px(10.0))
                .ml(px(-8.0))
                .rounded(px(8.0))
                .role(AccessibilityRole::Button)
                .label("返回")
                .window_drag(false)
                .hover(|s| s.bg(hover()))
                .active(|s| s.opacity(0.6))
                .on_press(move || back())
                .child(icon(Icon::ChevronLeft, 16.0, muted_fg))
                .child(text("返回").text_size(px(13.0)).text_color(muted_fg())),
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
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(fg()),
                        )
                        .child(text(meta).number_of_lines(1).text_size(px(13.0)).text_color(muted_fg()))
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
        .child(page_header(|| "搜索".into(), move || format!("{} 个结果", count.get()), None, None))
        .child(track_columns(Columns::ALL));
    view().flex_1().child(
        track_list(items, false, Columns::ALL, header.into_element())
            .empty(empty_state(Icon::Search, "没有匹配的曲目", "搜索匹配曲目的标题、歌手、专辑和文件名。")),
    )
}

// --- 文件夹 -------------------------------------------------------------------

#[derive(Default)]
struct Folder {
    path: String,
    name: String,
    children: BTreeMap<String, Folder>,
    tracks: Vec<usize>,
    /// 子树曲目数、字节数、时长（秒）。
    total: usize,
    bytes: u64,
    seconds: f64,
}

impl Folder {
    fn finish(&mut self, all: &[TrackEntry]) -> usize {
        let one_album = self.tracks.windows(2).all(|w| all[w[0]].album == all[w[1]].album);
        if one_album {
            self.tracks.sort_by(|a, b| by_disc_track(&all[*a], &all[*b]));
        } else {
            self.tracks.sort_by(|a, b| compare(&all[*a].file_name, &all[*b].file_name));
        }
        self.total = self.tracks.len() + self.children.values_mut().map(|c| c.finish(all)).sum::<usize>();
        self.bytes = self.tracks.iter().map(|&i| all[i].size_bytes).sum::<u64>()
            + self.children.values().map(|c| c.bytes).sum::<u64>();
        self.seconds = self.tracks.iter().map(|&i| all[i].duration).sum::<f64>()
            + self.children.values().map(|c| c.seconds).sum::<f64>();
        self.total
    }

    fn find(&self, path: &str) -> Option<&Folder> {
        if self.path == path {
            return Some(self);
        }
        self.children.values().find_map(|c| c.find(path))
    }

    /// 文件夹内（递归）的曲目，按显示顺序：子文件夹在前。
    fn subtree(&self, out: &mut Vec<usize>) {
        for child in self.children.values() {
            child.subtree(out);
        }
        out.extend(&self.tracks);
    }
}

/// 路径分隔符：曲库路径来自文件系统，Windows 上是 `\\`。
pub(crate) const SEP: char = std::path::MAIN_SEPARATOR;

/// 路径最后一段（文件夹名）；`C:\\` 这样的盘符根本身即名字。
pub(crate) fn last_segment(path: &str) -> &str {
    path.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(path)
}

fn build_tree(roots: &[String], tracks: &[TrackEntry]) -> Vec<Folder> {
    build_tree_with(roots, tracks, SEP)
}

fn build_tree_with(roots: &[String], tracks: &[TrackEntry], sep: char) -> Vec<Folder> {
    let mut nodes: Vec<Folder> = roots
        .iter()
        .map(|root| Folder {
            path: root.clone(),
            name: last_segment(root).to_owned(),
            ..Folder::default()
        })
        .collect();
    for (index, track) in tracks.iter().enumerate() {
        // 根可能以分隔符结尾（`C:\\`）。
        let Some(node) = nodes.iter_mut().find(|n| {
            let root = n.path.trim_end_matches(sep);
            track.path.starts_with(root) && track.path[root.len()..].starts_with(sep)
        }) else {
            continue;
        };
        let rest = &track.path[node.path.trim_end_matches(sep).len() + 1..];
        let parts: Vec<&str> = rest.split(sep).collect();
        let mut current = node;
        for part in &parts[..parts.len().saturating_sub(1)] {
            let path = format!("{}{sep}{}", current.path.trim_end_matches(sep), part);
            current = current.children.entry(compare_key(part)).or_insert_with(|| Folder {
                path,
                name: (*part).to_owned(),
                ..Folder::default()
            });
        }
        current.tracks.push(index);
    }
    for node in &mut nodes {
        node.finish(tracks);
    }
    nodes.sort_by(|a, b| compare(&a.name, &b.name));
    nodes
}

fn compare_key(name: &str) -> String {
    format!("{}\u{0}{name}", name.to_lowercase())
}

/// 文件夹页的一行。
#[derive(Clone, PartialEq)]
enum FolderRow {
    Folder {
        path: String,
        name: String,
        /// 顶层的根文件夹（说明里显示完整路径）。
        root: bool,
        folders: usize,
        count: usize,
        bytes: u64,
        seconds: f64,
    },
    Track { index: usize, number: usize },
}

/// 所在位置：`None` 为顶层，列出全部根文件夹（一个也列）。
#[derive(Clone, PartialEq)]
struct Place {
    path: Option<String>,
    title: String,
    trail: String,
    folders: usize,
    tracks: usize,
    bytes: u64,
    rows: Rc<Vec<FolderRow>>,
    /// 本文件夹直接包含的曲目（点播队列）。
    queue: Rc<Vec<usize>>,
    /// 子树全部曲目（「播放全部」）。
    all: Rc<Vec<usize>>,
    /// 返回去哪：`Some(None)` 为顶层；顶层自身为 `None`。
    parent: Option<Option<String>>,
}

fn folder_row(node: &Folder, root: bool) -> FolderRow {
    FolderRow::Folder {
        path: node.path.clone(),
        name: node.name.clone(),
        root,
        folders: node.children.len(),
        count: node.total,
        bytes: node.bytes,
        seconds: node.seconds,
    }
}

fn place(tree: &[Folder], at: Option<&str>) -> Place {
    place_with(tree, at, SEP)
}

fn place_with(tree: &[Folder], at: Option<&str>, sep: char) -> Place {
    let node = at.and_then(|p| tree.iter().find_map(|root| root.find(p)));
    let Some(node) = node else {
        let mut all = Vec::new();
        for root in tree {
            root.subtree(&mut all);
        }
        return Place {
            path: None,
            title: "文件夹".into(),
            trail: String::new(),
            folders: tree.len(),
            tracks: all.len(),
            bytes: tree.iter().map(|r| r.bytes).sum(),
            rows: Rc::new(tree.iter().map(|n| folder_row(n, true)).collect()),
            queue: Rc::default(),
            all: Rc::new(all),
            parent: None,
        };
    };
    let under = |r: &Folder| {
        let root = r.path.trim_end_matches(sep);
        node.path == r.path || (node.path.starts_with(root) && node.path[root.len()..].starts_with(sep))
    };
    let root = tree.iter().find(|r| under(r));
    let is_root = root.is_some_and(|r| r.path == node.path);
    let parent = if is_root { Some(None) } else { node.path.rsplit_once(sep).map(|(p, _)| Some(p.to_owned())) };
    let trail = root
        .map(|r| {
            let rest = node.path[r.path.trim_end_matches(sep).len()..].trim_start_matches(sep);
            if rest.is_empty() { r.path.clone() } else { format!("{} / {}", r.name, rest.replace(sep, " / ")) }
        })
        .unwrap_or_default();
    let mut rows: Vec<FolderRow> = node.children.values().map(|c| folder_row(c, false)).collect();
    rows.extend(node.tracks.iter().enumerate().map(|(n, &index)| FolderRow::Track { index, number: n + 1 }));
    let mut all = Vec::new();
    node.subtree(&mut all);
    Place {
        path: Some(node.path.clone()),
        title: node.name.clone(),
        trail,
        folders: node.children.len(),
        tracks: node.total,
        bytes: node.bytes,
        rows: Rc::new(rows),
        queue: Rc::new(node.tracks.clone()),
        all: Rc::new(all),
        parent,
    }
}

fn play_indices(indices: &[usize], start: usize) {
    let all = store().tracks.get_untracked();
    let queue: Vec<TrackEntry> = indices.iter().filter_map(|&i| all.get(i).cloned()).collect();
    play_list(&queue, start);
}

#[component]
pub fn Folders() -> impl IntoElement {
    let s = store();
    let cwd = signal(None::<String>);
    let tree = computed(move || {
        let roots = s.library.with(|l| l.roots.clone());
        let tracks = s.tracks.get();
        Same(Arc::new(build_tree(&roots, &tracks)))
    });
    let here = computed(move || {
        let tree = tree.get();
        // 文件夹已不存在（重扫、移除根目录后）时回到顶层。
        place(&tree, cwd.get().as_deref())
    });

    let body = move || {
        let p = here.get_untracked();
        // 标题与说明跟着数据更新（扫描完成、文件变化），位置不变时不重建。
        let subtitle = move || {
            here.with(|p| {
                let mut parts = Vec::new();
                if !p.trail.is_empty() && p.trail != p.title {
                    parts.push(p.trail.clone());
                }
                if p.path.is_none() {
                    parts.push(format!("{} 个根文件夹", p.folders));
                } else if p.folders > 0 {
                    parts.push(format!("{} 个文件夹", p.folders));
                }
                parts.push(format!("{} 首", p.tracks));
                parts.push(format_bytes(p.bytes));
                parts.join(" · ")
            })
        };
        let back: Option<Rc<dyn Fn()>> = p.parent.clone().map(|parent| Rc::new(move || cwd.set(parent.clone())) as Rc<dyn Fn()>);
        let actions = view()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .child(rescan_button())
            .child(when(move || here.with(|p| !p.all.is_empty()), move || {
                pill_button(Icon::Play, "播放全部", move || here.with_untracked(|p| play_indices(&p.all, 0)))
            }));
        let title = move || here.with(|p| p.title.clone());
        let header = view()
            .child(page_header(title, subtitle, back, Some(actions.into_element())))
            .child(track_columns(FOLDER_COLS));

        let rows = computed(move || here.with(|p| p.rows.clone()));
        let count = computed(move || rows.with(|r| r.len()));
        let list = virtual_list(count, move |at| {
            let row = computed(move || rows.with(|r| r.get(at.index()).cloned()));
            let is_folder = computed(move || row.with(|r| matches!(r, Some(FolderRow::Folder { .. }))));
            when(is_folder, move || folder_cell(row, cwd))
                .otherwise(move || {
                    let track = computed(move || match row.get() {
                        Some(FolderRow::Track { index, .. }) => s.tracks.with(|t| t.get(index).cloned()),
                        _ => None,
                    });
                    let number = computed(move || match row.get() {
                        Some(FolderRow::Track { number, .. }) => number.to_string(),
                        _ => String::new(),
                    });
                    let play = Rc::new(move || {
                        let Some(FolderRow::Track { number, .. }) = row.get_untracked() else { return };
                        let queue = here.with_untracked(|p| p.queue.clone());
                        play_indices(&queue, number - 1);
                    });
                    let row = track_row(track, number, FOLDER_COLS, {
                        let play = play.clone();
                        move || play()
                    });
                    super::playlists::track_menu(row, track, None, move || play())
                })
                .into_element()
        })
        .estimated_item_size(px(ROW))
        .content_padding_bottom(px(BAR_SPACE))
        .content_padding_x(px(20.0))
        .flex_1();
        view().flex_1().child(header).child(list)
    };

    view()
        .flex_1()
        .child(when(move || s.library.with(|l| l.roots.is_empty()), no_library).otherwise(move || {
            // 位置变了才重建页头与列表；同一位置内的数据变化由绑定更新。
            let at = computed(move || here.with(|p| p.path.clone()));
            dynamic(move || {
                at.get();
                untracked(body)
            })
        }))
}

fn rescan_button() -> View {
    let s = store();
    icon_button(|| Icon::Refresh, 16.0, "重新扫描", muted_fg, scan)
        .style_when(move || s.library.with(|l| l.scanning), |st| st.opacity(0.4))
}

fn folder_cell(row: Memo<Option<FolderRow>>, cwd: Signal<Option<String>>) -> View {
    let pick = move |f: fn(&FolderRow) -> String| move || row.with(|r| r.as_ref().map(f).unwrap_or_default());
    let is_root = move || row.with(|r| matches!(r, Some(FolderRow::Folder { root: true, .. })));
    let tile = view()
        .size(px(W_COVER))
        .rounded(px(8.0))
        .items_center()
        .justify_center()
        .bg(move || if is_root() { primary().opacity(0.12) } else { muted() })
        .child(icon(Icon::Folder, 20.0, move || if is_root() { primary() } else { muted_fg() }));
    view()
        .h(px(ROW))
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .px(px(12.0))
        .rounded(px(10.0))
        .role(AccessibilityRole::Button)
        .label(pick(|r| match r {
            FolderRow::Folder { name, .. } => name.clone(),
            FolderRow::Track { .. } => String::new(),
        }))
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.7))
        .on_press(move || {
            if let Some(FolderRow::Folder { path, .. }) = row.get_untracked() {
                cwd.set(Some(path));
            }
        })
        .child(when(fits_number, || view().w(px(W_NUMBER)).items_center().child(icon(Icon::ChevronRight, 14.0, faint))))
        .child(tile)
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    text(pick(|r| match r {
                        FolderRow::Folder { name, .. } => name.clone(),
                        FolderRow::Track { .. } => String::new(),
                    }))
                    .number_of_lines(1)
                    .text_size(px(14.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg()),
                )
                .child(
                    text(pick(|r| match r {
                        FolderRow::Folder { root: true, path, .. } => path.clone(),
                        FolderRow::Folder { folders, count, .. } if *folders > 0 => {
                            format!("{folders} 个文件夹 · {count} 首")
                        }
                        FolderRow::Folder { count, .. } => format!("{count} 首"),
                        FolderRow::Track { .. } => String::new(),
                    }))
                    .number_of_lines(1)
                    .text_size(px(12.0))
                    .text_color(muted_fg()),
                ),
        )
        .child(when(fits_album, move || {
            view().w(px(W_ALBUM)).child(
                text(pick(|r| match r {
                    FolderRow::Folder { root: true, folders, count, .. } if *folders > 0 => {
                        format!("{folders} 个文件夹 · {count} 首")
                    }
                    FolderRow::Folder { root: true, count, .. } => format!("{count} 首"),
                    _ => String::new(),
                }))
                .number_of_lines(1)
                .text_size(px(13.0))
                .text_color(muted_fg()),
            )
        }))
        // 文件夹行的格式列留空，保持与曲目行对齐。
        .child(when(fits_size, || view().w(px(W_FORMAT))))
        .child(when(fits_size, move || {
            value_cell(W_SIZE, pick(|r| match r {
                FolderRow::Folder { bytes, .. } => format_bytes(*bytes),
                FolderRow::Track { .. } => String::new(),
            }))
        }))
        .child(value_cell(W_TIME, pick(|r| match r {
            FolderRow::Folder { seconds, .. } => format_total_short(*seconds),
            FolderRow::Track { .. } => String::new(),
        })))
}

/// 文件夹总时长（列内的短格式）：3:25:10 / 42:05。
fn format_total_short(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

// --- 歌手 ---------------------------------------------------------------------

#[derive(Clone, PartialEq)]
struct Group {
    key: String,
    title: String,
    subtitle: String,
    year: Option<u32>,
    tracks: Rc<Vec<TrackEntry>>,
    cover: Option<String>,
}

impl Group {
    fn albums(&self) -> usize {
        self.tracks.iter().map(|t| t.album.as_str()).collect::<HashSet<_>>().len()
    }
}

fn group_by(tracks: &[TrackEntry], key: impl Fn(&TrackEntry) -> Vec<(String, String, String)>) -> Vec<Group> {
    let mut map: HashMap<String, (String, String, Vec<TrackEntry>)> = HashMap::new();
    for track in tracks {
        for (k, title, subtitle) in key(track) {
            map.entry(k).or_insert_with(|| (title, subtitle, Vec::new())).2.push(track.clone());
        }
    }
    let mut out: Vec<Group> = map
        .into_iter()
        .map(|(key, (title, subtitle, mut list))| {
            list.sort_by(|a, b| compare(&a.album, &b.album).then_with(|| by_disc_track(a, b)));
            let cover = list.iter().find(|t| t.has_art).map(|t| t.path.clone());
            let year = list.iter().filter_map(|t| t.year).max();
            Group { key, title, subtitle, year, tracks: Rc::new(list), cover }
        })
        .collect();
    out.sort_by(|a, b| compare(&a.title, &b.title).then_with(|| compare(&a.subtitle, &b.subtitle)));
    out
}

/// 圆形头像：封面或首字。
fn avatar(cover: impl Fn() -> Option<String> + Copy + 'static, name: impl Fn() -> String + 'static, size: f32) -> View {
    view()
        .size(px(size))
        .rounded_full()
        .overflow_hidden()
        .bg(muted())
        .items_center()
        .justify_center()
        .child(
            text(move || name().chars().next().map(String::from).unwrap_or_default())
                .text_size(px(size * 0.4))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted_fg()),
        )
        .child(when(move || cover().and_then(|p| cover_of(&p)).is_some(), move || {
            image(move || cover().and_then(|p| cover_of(&p)).unwrap_or_default())
                .absolute()
                .inset(px(0.0))
                .resize_mode(ResizeMode::Cover)
        }))
}

#[component]
pub fn Artists() -> impl IntoElement {
    let s = store();
    let selected = signal(None::<String>);
    let groups = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(group_by(&tracks, |t| vec![(t.artist.clone(), t.artist.clone(), String::new())]))
    });
    view().flex_1().child(dynamic(move || {
        if s.tracks.with(|t| t.is_empty()) {
            return no_library().into_element();
        }
        match selected.get() {
            Some(key) => {
                let group = untracked(|| groups.with(|g| g.iter().find(|g| g.key == key).cloned()));
                let Some(group) = group else {
                    selected.set(None);
                    return view().into_element();
                };
                let items = computed({
                    let list = group.tracks.clone();
                    move || list.clone()
                });
                let cover_path = stored(group.cover.clone());
                let name = group.title.clone();
                let header = hero(
                    "歌手",
                    avatar(move || cover_path.get(), move || name.clone(), 168.0),
                    group.title.clone(),
                    format!("{} 首 · {} 张专辑", group.tracks.len(), group.albums()),
                    format_total(total_duration(&group.tracks)),
                    Rc::new(move || selected.set(None)),
                    move || items.with_untracked(|v| play_list(v, 0)),
                )
                .child(track_columns(Columns::ALL).mx(px(-12.0)));
                view().flex_1().child(track_list(items, false, Columns::ALL, header.into_element())).into_element()
            }
            None => {
                let count = computed(move || groups.with(|g| g.len()));
                let list = virtual_list(count, move |at| {
                    let group = computed(move || groups.with(|g| g.get(at.index()).cloned()));
                    let cover = move || group.with(|g| g.as_ref().and_then(|g| g.cover.clone()));
                    let name = move || group.with(|g| g.as_ref().map(|g| g.title.clone()).unwrap_or_default());
                    view()
                        .h(px(64.0))
                        .flex_row()
                        .items_center()
                        .gap(px(14.0))
                        .px(px(12.0))
                        .mx(px(20.0))
                        .rounded(px(10.0))
                        .role(AccessibilityRole::Button)
                        .label(name)
                        .hover(|st| st.bg(hover()))
                        .active(|st| st.opacity(0.7))
                        .on_press(move || selected.set(group.get_untracked().map(|g| g.key)))
                        .child(avatar(cover, name, 44.0))
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap(px(2.0))
                                .child(text(name).number_of_lines(1).text_size(px(14.0)).font_weight(FontWeight::MEDIUM).text_color(fg()))
                                .child(
                                    text(move || group.with(|g| g.as_ref().map(|g| format!("{} 首 · {} 张专辑", g.tracks.len(), g.albums())).unwrap_or_default()))
                                        .text_size(px(12.0))
                                        .text_color(muted_fg()),
                                ),
                        )
                        .child(icon(Icon::ChevronRight, 14.0, faint))
                })
                .estimated_item_size(px(64.0))
                .content_padding_bottom(px(BAR_SPACE))
                .flex_1();
                view()
                    .flex_1()
                    .child(page_header(|| "歌手".into(), move || format!("{} 位歌手", count.get()), None, None))
                    .child(list)
                    .into_element()
            }
        }
    }))
}

// --- 专辑 ---------------------------------------------------------------------

#[component]
pub fn Albums() -> impl IntoElement {
    let s = store();
    let selected = signal(None::<String>);
    let groups = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(group_by(&tracks, |t| {
            let artist = if t.album_artist.is_empty() { t.artist.clone() } else { t.album_artist.clone() };
            vec![(format!("{artist}\u{0}{}", t.album), t.album.clone(), artist)]
        }))
    });
    view().flex_1().child(dynamic(move || {
        if s.tracks.with(|t| t.is_empty()) {
            return no_library().into_element();
        }
        match selected.get() {
            Some(key) => {
                let group = untracked(|| groups.with(|g| g.iter().find(|g| g.key == key).cloned()));
                let Some(group) = group else {
                    selected.set(None);
                    return view().into_element();
                };
                let items = computed({
                    let list = group.tracks.clone();
                    move || list.clone()
                });
                let cover_path = group.cover.clone();
                let cover_src = computed(move || cover_path.as_deref().and_then(cover_of));
                let meta = [
                    group.year.map(|y| y.to_string()),
                    Some(format!("{} 首", group.tracks.len())),
                    Some(format_total(total_duration(&group.tracks))),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
                let header = hero(
                    "专辑",
                    cover(move || cover_src.get(), Some(200.0), 12.0),
                    group.title.clone(),
                    group.subtitle.clone(),
                    meta,
                    Rc::new(move || selected.set(None)),
                    move || items.with_untracked(|v| play_list(v, 0)),
                )
                .child(track_columns(ALBUM_COLS).mx(px(-12.0)));
                view().flex_1().child(track_list(items, true, ALBUM_COLS, header.into_element())).into_element()
            }
            None => {
                let count = computed(move || groups.with(|g| g.len()));
                let columns = move || {
                    let width = window().width.0 - SIDEBAR - 40.0;
                    ((width + 8.0) / (184.0 + 8.0)).floor().clamp(2.0, 10.0) as u32
                };
                let grid = virtual_list(count, move |at| {
                    let group = computed(move || groups.with(|g| g.get(at.index()).cloned()));
                    let field = move |f: fn(&Group) -> String| move || group.with(|g| g.as_ref().map(f).unwrap_or_default());
                    let cover_path = move || group.with(|g| g.as_ref().and_then(|g| g.cover.clone()));
                    view()
                        .p(px(8.0))
                        .gap(px(10.0))
                        .rounded(px(14.0))
                        .role(AccessibilityRole::Button)
                        .label(field(|g| g.title.clone()))
                        .hover(|st| st.bg(hover()))
                        .active(|st| st.opacity(0.75))
                        .on_press(move || selected.set(group.get_untracked().map(|g| g.key)))
                        .child(cover(move || cover_path().and_then(|p| cover_of(&p)), None, 10.0).shadow_md())
                        .child(
                            view()
                                .gap(px(2.0))
                                .px(px(2.0))
                                .child(
                                    text(field(|g| g.title.clone()))
                                        .number_of_lines(1)
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(fg()),
                                )
                                .child(text(field(|g| g.subtitle.clone())).number_of_lines(1).text_size(px(12.0)).text_color(muted_fg())),
                        )
                })
                .columns(columns)
                .estimated_item_size(px(240.0))
                .content_padding_x(px(20.0))
                .content_padding_bottom(px(BAR_SPACE))
                .flex_1();
                let grid_ref = element_ref();
                super::BENCH_GRID.with(|g| g.set(Some(grid_ref)));
                let grid = grid.element_ref(&grid_ref);
                view()
                    .flex_1()
                    .child(page_header(|| "专辑".into(), move || format!("{} 张专辑", count.get()), None, None))
                    .child(grid)
                    .into_element()
            }
        }
    }))
}

// --- 标签 ---------------------------------------------------------------------

#[component]
pub fn Genres() -> impl IntoElement {
    let s = store();
    let selected = signal(None::<String>);
    let groups = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(group_by(&tracks, |t| t.genres.iter().map(|g| (g.clone(), g.clone(), String::new())).collect()))
    });
    view().flex_1().child(dynamic(move || {
        if s.tracks.with(|t| t.is_empty()) {
            return no_library().into_element();
        }
        match selected.get() {
            Some(key) => {
                let group = untracked(|| groups.with(|g| g.iter().find(|g| g.key == key).cloned()));
                let Some(group) = group else {
                    selected.set(None);
                    return view().into_element();
                };
                let items = computed({
                    let list = group.tracks.clone();
                    move || list.clone()
                });
                let title = format!("# {}", group.title);
                let subtitle = format!("{} 首 · {}", group.tracks.len(), format_total(total_duration(&group.tracks)));
                let header = view()
                    .child(page_header(
                        move || title.clone(),
                        move || subtitle.clone(),
                        Some(Rc::new(move || selected.set(None))),
                        Some(pill_button(Icon::Play, "播放", move || items.with_untracked(|v| play_list(v, 0))).into_element()),
                    ))
                    .child(track_columns(Columns::ALL));
                view().flex_1().child(track_list(items, false, Columns::ALL, header.into_element())).into_element()
            }
            None => {
                let list = groups.get_untracked();
                let total = list.len();
                if total == 0 {
                    return empty_state(Icon::Tag, "没有流派标签", "曲目的流派标签会出现在这里（按 ; 拆分）。").into_element();
                }
                let chips = list.iter().map(|g| {
                    let key = g.key.clone();
                    let art = stored(g.cover.clone());
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .h(px(44.0))
                        .pl(px(6.0))
                        .pr(px(16.0))
                        .rounded_full()
                        .bg(muted())
                        .role(AccessibilityRole::Button)
                        .label(g.title.clone())
                        .hover(|st| st.bg(hover()))
                        .active(|st| st.opacity(0.7))
                        .on_press(move || selected.set(Some(key.clone())))
                        .child(cover_with(move || art.get().and_then(|p| cover_of(&p)), 32.0, 16.0, Icon::Tag))
                        .child(text(g.title.clone()).text_size(px(14.0)).font_weight(FontWeight::MEDIUM).text_color(fg()))
                        .child(text(g.tracks.len().to_string()).text_size(px(12.0)).tabular_nums().text_color(faint()))
                });
                view()
                    .flex_1()
                    .child(page_header(|| "标签".into(), move || format!("{total} 个流派"), None, None))
                    .child(
                        scroll().flex_1().child(
                            view()
                                .flex_row()
                                .flex_wrap()
                                .gap(px(10.0))
                                .px(px(32.0))
                                .pt(px(4.0))
                                .pb(px(BAR_SPACE))
                                .children(chips),
                        ),
                    )
                    .into_element()
            }
        }
    }))
}

// --- 设置 ---------------------------------------------------------------------

/// file:// URL → 路径（百分号解码）。Windows 的 `file:///C:/Music` 去掉
/// 盘符前的 `/`，并换成 `\\`。
fn url_to_path(uri: &str) -> String {
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

/// 添加为根目录：目录直接加；文件取所在目录。
fn add_as_root(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches(['/', '\\']);
    let lib = library_store();
    match lib.add_root(trimmed) {
        Ok(_) => {
            scan();
            format!("已添加 {trimmed}")
        }
        Err(error) => {
            let parent = std::path::Path::new(trimmed).parent().map(|p| p.to_string_lossy().into_owned());
            match parent.map(|p| lib.add_root(&p).map(|_| p)) {
                Some(Ok(p)) => {
                    scan();
                    format!("已添加 {p}")
                }
                _ => error,
            }
        }
    }
}

/// 设置里的一张卡片：标题、说明、右上角操作，然后是内容。
fn card_section(title: &'static str, body: &'static str, action: Option<AnyElement>) -> View {
    view()
        .w_full()
        .gap(px(16.0))
        .p(px(20.0))
        .rounded(px(14.0))
        .bg(card())
        .border_1()
        .border_color(border())
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .items_start()
                .gap(px(16.0))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(240.0))
                        .gap(px(4.0))
                        .child(text(title).text_size(px(15.0)).font_weight(FontWeight::SEMIBOLD).text_color(fg()))
                        .child(text(body).text_size(px(13.0)).text_color(muted_fg())),
                )
                .child(action.map(|a| view().flex_row().gap(px(8.0)).child(a))),
        )
}

/// 一行说明文字（操作结果）。
fn note(message: Signal<String>) -> AnyElement {
    dynamic(move || {
        let now = message.get();
        (!now.is_empty()).then(|| text(now).text_size(px(12.0)).text_color(muted_fg()))
    })
}

fn folders_card() -> View {
    let s = store();
    let message = signal(String::new());
    let dragging = signal(false);
    let picking = signal(false);

    let pick = move || {
        if picking.get_untracked() {
            return;
        }
        picking.set(true);
        spawn(async move {
            if let Some(result) = super::store::pick_folders().await {
                message.set(result);
            }
            picking.set(false);
        });
    };

    let counts = computed(move || {
        let roots = s.library.with(|l| l.roots.clone());
        let tracks = s.tracks.get();
        roots
            .iter()
            .map(|r| {
                let prefix = format!("{}{SEP}", r.trim_end_matches(SEP));
                tracks.iter().filter(|t| t.path.starts_with(&prefix)).count()
            })
            .collect::<Vec<_>>()
    });

    let rows = dynamic(move || {
        let roots = s.library.with(|l| l.roots.clone());
        if roots.is_empty() {
            return vec![
                view()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .h(px(120.0))
                    .rounded(px(12.0))
                    .border_2()
                    .border_dashed()
                    .border_color(move || if dragging.get() { primary() } else { border() })
                    .bg(move || if dragging.get() { hover() } else { AdaptiveColor::TRANSPARENT })
                    .child(icon(Icon::Folder, 24.0, faint))
                    .child(
                        text(move || if dragging.get() { "松开以添加" } else { "还没有文件夹。点「添加文件夹…」，或把文件夹拖到这里" })
                            .text_size(px(13.0))
                            .text_color(muted_fg()),
                    )
                    .into_element(),
            ];
        }
        roots
            .into_iter()
            .enumerate()
            .map(|(i, root)| {
                let remove = root.clone();
                let name = last_segment(&root).to_owned();
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(12.0))
                    .h(px(56.0))
                    .px(px(12.0))
                    .rounded(px(10.0))
                    .bg(muted())
                    .child(
                        view()
                            .size(px(36.0))
                            .rounded(px(9.0))
                            .bg(card())
                            .items_center()
                            .justify_center()
                            .child(icon(Icon::Folder, 18.0, primary)),
                    )
                    .child(
                        view()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap(px(2.0))
                            .child(text(name).number_of_lines(1).text_size(px(14.0)).font_weight(FontWeight::MEDIUM).text_color(fg()))
                            .child(text(root.clone()).number_of_lines(1).text_size(px(12.0)).text_color(muted_fg())),
                    )
                    .child(
                        text(move || counts.with(|c| c.get(i).map(|n| format!("{n} 首")).unwrap_or_default()))
                            .text_size(px(12.0))
                            .tabular_nums()
                            .text_color(faint()),
                    )
                    .child(icon_button(|| Icon::Close, 14.0, "移除", muted_fg, move || {
                        library_store().remove_root(&remove);
                        message.set(format!("已移除 {remove}"));
                        scan();
                    }))
                    .into_element()
            })
            .collect()
    });

    let add = view()
        .style_when(move || picking.get(), |st| st.opacity(0.5))
        .child(pill_button(Icon::Plus, "添加文件夹…", pick));

    card_section(
        "音乐文件夹",
        "Zeo Player 会扫描这些文件夹里的 MP3、FLAC 和 WAV，包括子文件夹。也可以把文件夹拖进这张卡片。",
        Some(add.into_element()),
    )
    .border_color(move || if dragging.get() { primary() } else { border() })
    .on_file_drag_enter(move || dragging.set(true))
    .on_file_drag_leave(move || dragging.set(false))
    .on_file_drop(move |event: &FileDropEvent| {
        dragging.set(false);
        let results: Vec<String> = event.uris.iter().map(|uri| add_as_root(&url_to_path(uri))).collect();
        message.set(results.join("\n"));
    })
    .child(view().gap(px(8.0)).child(rows))
    .child(note(message))
}

fn library_card() -> View {
    let s = store();
    let progress = move || s.library.with(|l| l.progress.clone());
    let scanning = move || s.library.with(|l| l.scanning);
    let fraction = move || {
        let p = progress();
        if p.total > 0 { p.done as f64 / p.total as f64 } else { 0.0 }
    };
    let actions = view()
        .flex_row()
        .gap(px(8.0))
        .style_when(scanning, |st| st.opacity(0.4))
        .child(outline_button(Icon::Refresh, "扫描更新", scan))
        .child(outline_button(Icon::Disc, "重建曲库", super::store::rebuild));

    let stats = move || view()
        .flex_row()
        .items_end()
        .gap(px(24.0))
        .child(stat(move || s.tracks.with(|t| t.len()).to_string(), "首曲目"))
        .child(stat(
            move || {
                s.tracks.with(|t| {
                    t.iter()
                        .map(|t| (t.album_artist.as_str(), t.album.as_str()))
                        .collect::<HashSet<_>>()
                        .len()
                        .to_string()
                })
            },
            "张专辑",
        ))
        .child(stat(
            move || s.tracks.with(|t| t.iter().map(|t| t.artist.as_str()).collect::<HashSet<_>>().len().to_string()),
            "位歌手",
        ))
        .child(stat(move || format_total(s.tracks.with(|t| total_duration(t))), "总时长"));

    let bar = move || view()
        .gap(px(8.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(
                    text(move || {
                        let p = progress();
                        if p.total == 0 {
                            "正在查找音频文件…".to_owned()
                        } else {
                            format!("正在读取 {} / {}", p.done, p.total)
                        }
                    })
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .tabular_nums()
                    .text_color(fg()),
                )
                .child(view().flex_1())
                .child(
                    text(move || format!("{:.0}%", fraction() * 100.0))
                        .text_size(px(12.0))
                        .tabular_nums()
                        .text_color(muted_fg()),
                ),
        )
        .child(
            view().h(px(6.0)).w_full().rounded_full().bg(muted()).overflow_hidden().child(
                view()
                    .h_full()
                    .rounded_full()
                    .bg(primary())
                    .w(move || relative(fraction() as f32))
                    .transition_all()
                    .duration(150.0),
            ),
        )
        .child(
            text(move || progress().current)
                .number_of_lines(1)
                .text_size(px(12.0))
                .text_color(faint()),
        );

    card_section(
        "曲库",
        "索引保存在本机，启动时自动扫描。「重建曲库」会清空标签和封面缓存，从文件重新读取全部曲目。",
        Some(actions.into_element()),
    )
    .child(when(scanning, bar).otherwise(stats))
    .child(view().h(px(1.0)).bg(border()))
    .child(watch_row())
}

/// 文件夹监视开关与状态。
fn watch_row() -> View {
    let s = store();
    let on = move || s.library.with(|l| l.watching);
    let status = move || {
        s.library.with(|l| {
            let w = &l.watch;
            if !l.watching {
                return "关闭时，文件夹里的变化要手动「扫描更新」才会出现。".to_owned();
            }
            if let Some(error) = &w.error {
                return error.clone();
            }
            let mut text = format!("正在监视 {} 个文件夹，新增、修改、删除或改名音频文件后自动扫描", w.roots);
            if let Some(at) = w.last_rescan {
                let ago = crate::library::unix_now().saturating_sub(at);
                let when = match ago {
                    0..60 => "刚刚".to_owned(),
                    60..3600 => format!("{} 分钟前", ago / 60),
                    _ => format!("{} 小时前", ago / 3600),
                };
                text = format!("{text}。上次：{when}，{} 处变化", w.last_changes);
            }
            text
        })
    };
    let toggle = nsh_base::switch()
        .checked(on)
        .accessibility_label("监视文件夹变化")
        .on_change(move |next: &bool| super::store::set_watch(*next))
        .track_style(|st| {
            st.w(px(42.0))
                .h(px(24.0))
                .p(px(2.0))
                .rounded_full()
                .bg(muted())
                .border_1()
                .border_color(border())
                .transition_colors()
                .duration(150.0)
                .checked(|st| st.bg(primary()).border_color(primary()))
        })
        .thumb_style(|st| {
            st.size(px(18.0))
                .rounded_full()
                .bg(AdaptiveColor::rgb(0xffffff, 0xffffff))
                .shadow_sm()
                .transition_transform()
                .duration(150.0)
                .checked(|st| st.translate_x(px(18.0)))
        });
    view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(16.0))
        .child(
            view()
                .flex_1()
                .min_w(px(200.0))
                .gap(px(2.0))
                .child(text("监视文件夹变化").text_size(px(14.0)).font_weight(FontWeight::MEDIUM).text_color(fg()))
                .child(
                    text(status)
                        .text_size(px(12.0))
                        .text_color(move || if s.library.with(|l| l.watching && l.watch.error.is_some()) { nsh_base::theme().destructive } else { muted_fg() }),
                ),
        )
        .child(toggle)
}

fn stat(value: impl Fn() -> String + 'static, label: &'static str) -> View {
    view()
        .gap(px(2.0))
        .child(text(value).text_size(px(22.0)).font_weight(FontWeight::BOLD).tabular_nums().text_color(fg()))
        .child(text(label).text_size(px(12.0)).text_color(muted_fg()))
}

/// 常用分隔符（可直接点选）。
const PRESETS: &[(&str, &str)] = &[(";", "分号"), ("/", "斜杠"), (",", "逗号"), ("、", "顿号"), ("|", "竖线"), ("&", "和号")];

fn separator_label(sep: &str) -> String {
    match sep {
        " " => "空格".into(),
        s if s.trim().is_empty() => format!("{:?}", s),
        s => s.to_owned(),
    }
}

fn genres_card() -> View {
    let s = store();
    let draft = signal(String::new());
    let separators = move || s.library.with(|l| l.separators.clone());
    let toggle = move |sep: String| {
        let mut list = separators();
        if let Some(i) = list.iter().position(|x| *x == sep) {
            list.remove(i);
        } else {
            list.push(sep);
        }
        super::store::set_separators(list);
    };
    let add_custom = move || {
        let sep = draft.get_untracked();
        if sep.is_empty() {
            return;
        }
        let mut list = separators();
        if !list.contains(&sep) {
            list.push(sep);
            super::store::set_separators(list);
        }
        draft.set(String::new());
    };

    let chip = move |sep: String, caption: Option<&'static str>| {
        let on = {
            let sep = sep.clone();
            move || separators().contains(&sep)
        };
        let on2 = on.clone();
        let press = sep.clone();
        view()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .h(px(32.0))
            .px(px(12.0))
            .rounded_full()
            .border_1()
            .border_color(move || if on() { primary() } else { border() })
            .bg(move || if on2() { primary().opacity(0.12) } else { AdaptiveColor::TRANSPARENT })
            .role(AccessibilityRole::Button)
            .label(caption.unwrap_or("自定义分隔符"))
            .hover(|st| st.bg(hover()))
            .active(|st| st.opacity(0.7))
            .on_press(move || toggle(press.clone()))
            .child(
                text(separator_label(&sep))
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(fg()),
            )
            .child(caption.map(|c| text(c).text_size(px(12.0)).text_color(muted_fg())))
    };

    let chips = dynamic(move || {
        let current = separators();
        let mut out: Vec<AnyElement> = PRESETS.iter().map(|(sep, name)| chip((*sep).to_owned(), Some(*name)).into_element()).collect();
        for sep in current.iter().filter(|c| !PRESETS.iter().any(|(p, _)| p == c)) {
            out.push(chip(sep.clone(), None).child(icon(Icon::Close, 11.0, muted_fg)).into_element());
        }
        out
    });

    let custom = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            input()
                .w(px(160.0))
                .h(px(32.0))
                .px(px(12.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme_input())
                .bg(bg())
                .text_size(px(14.0))
                .text_color(fg())
                .value(move || draft.get())
                .placeholder("其它分隔符，如 ・")
                .on_change(move |text: &str| draft.set(text.to_owned()))
                .on_submit(move |_: &str| add_custom()),
        )
        .child(outline_button(Icon::Plus, "添加", add_custom))
        .child(view().flex_1())
        .child(
            view()
                .role(AccessibilityRole::Button)
                .label("恢复默认")
                .px(px(10.0))
                .h(px(32.0))
                .justify_center()
                .rounded(px(8.0))
                .hover(|st| st.bg(hover()))
                .active(|st| st.opacity(0.7))
                .on_press(|| {
                    super::store::set_separators(
                        crate::library::DEFAULT_SEPARATORS.iter().map(|s| (*s).to_owned()).collect(),
                    )
                })
                .child(text("恢复默认").text_size(px(13.0)).text_color(muted_fg())),
        );

    // 预览：曲库里最常见的、含多个值的原始流派标签，按当前分隔符拆开。
    let samples = computed(move || {
        let tracks = s.tracks.get();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for t in tracks.iter() {
            if let Ok(meta) = crate::metadata::MetaCache::global().get(std::path::Path::new(&t.path)) {
                for raw in &meta.genres {
                    *counts.entry(raw.clone()).or_default() += 1;
                }
            }
        }
        let mut list: Vec<(String, usize)> = counts.into_iter().collect();
        let busy = |raw: &str| raw.chars().filter(|c| !c.is_alphanumeric() && !c.is_whitespace()).count();
        list.sort_by(|a, b| busy(&b.0).cmp(&busy(&a.0)).then(b.1.cmp(&a.1)).then(a.0.cmp(&b.0)));
        list.into_iter().take(4).map(|(raw, _)| raw).collect::<Vec<_>>()
    });
    let preview = dynamic(move || {
        let seps = separators();
        let rows: Vec<AnyElement> = samples
            .get()
            .into_iter()
            .map(|raw| {
                let parts = crate::library::split_genres(std::slice::from_ref(&raw), &seps);
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0))
                    .min_h(px(32.0))
                    .child(text(raw).number_of_lines(1).w(px(220.0)).text_size(px(13.0)).text_color(muted_fg()))
                    .child(icon(Icon::ChevronRight, 12.0, faint))
                    .child(view().flex_1().flex_row().flex_wrap().gap(px(6.0)).children(parts.into_iter().map(|g| {
                        view()
                            .rounded_full()
                            .bg(muted())
                            .px(px(10.0))
                            .py(px(3.0))
                            .child(text(g).text_size(px(12.0)).font_weight(FontWeight::MEDIUM).text_color(fg()))
                    })))
                    .into_element()
            })
            .collect();
        (!rows.is_empty()).then(|| {
            view()
                .gap(px(4.0))
                .p(px(12.0))
                .rounded(px(10.0))
                .bg(bg())
                .child(
                    text(move || {
                        let total: HashSet<String> = s.tracks.with(|t| t.iter().flat_map(|t| t.genres.iter().cloned()).collect());
                        format!("预览 · 当前共 {} 个标签", total.len())
                    })
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .letter_spacing(px(0.6))
                    .text_color(faint())
                    .pb(px(4.0)),
                )
                .children(rows)
        })
    });

    card_section(
        "流派标签",
        "一个流派标签里有多个值时，按下面选中的分隔符拆开，并去掉首尾空格。修改后立即生效，不用重新扫描。",
        None,
    )
    .child(view().flex_row().flex_wrap().gap(px(8.0)).child(chips))
    .child(custom)
    .child(preview)
}

/// 字体：系统字体或任一已安装的字体家族，整个界面立即换用。
/// 字体列表懒加载：点「更改字体…」才读取；收起或离开设置页时整块销毁
/// （列表数据、读取任务、各行的字体预览一起释放）。
fn font_card() -> View {
    let s = store();
    let expanded = signal(false);
    let current = move || s.library.with(|l| l.font.clone());

    let system = view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .h(px(48.0))
        .px(px(12.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(move || if current().is_none() { primary() } else { border() })
        .bg(move || if current().is_none() { primary().opacity(0.12) } else { AdaptiveColor::TRANSPARENT })
        .role(AccessibilityRole::Button)
        .label("系统字体")
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.7))
        .on_press(|| super::store::set_font(None))
        .child(
            view()
                .flex_1()
                .gap(px(2.0))
                .child(text("系统字体").font_family("system-ui").text_size(px(14.0)).font_weight(FontWeight::MEDIUM).text_color(fg()))
                .child(
                    text(move || match current() {
                        Some(font) => format!("当前：{font}"),
                        None => "当前使用系统字体".to_owned(),
                    })
                    .text_size(px(12.0))
                    .text_color(muted_fg()),
                ),
        );

    let toggle = move || view().self_start().child(outline_button(Icon::ChevronDown, "更改字体…", move || expanded.set(true)));

    card_section("字体", "整个界面（包括歌词）换用所选字体，立即生效。", None)
        .child(system)
        .child(when(move || expanded.get(), move || font_browser(expanded)).otherwise(toggle))
}

/// 展开后的字体列表：在后台读取系统已安装的字体，读完再显示。
fn font_browser(expanded: Signal<bool>) -> View {
    let s = store();
    let query = signal(String::new());
    let current = move || s.library.with(|l| l.font.clone());
    // None：还在读取。任务属于这一块，块销毁时一起取消。
    let installed = signal(None::<Same<Vec<String>>>);
    spawn(async move {
        if let Ok(list) = blocking(crate::fonts::installed_families).await {
            installed.set(Some(Same(std::sync::Arc::new(list))));
        }
    });
    let loading = move || installed.with(Option::is_none);

    // 按搜索词过滤。
    let rows = computed(move || {
        let q = query.get().trim().to_lowercase();
        let out: Vec<String> = installed.with(|list| {
            list.iter()
                .flat_map(|list| list.iter())
                .filter(|f| q.is_empty() || f.to_lowercase().contains(&q))
                .cloned()
                .collect()
        });
        Rc::new(out)
    });
    let count = computed(move || rows.with(|r| r.len()));

    let list = virtual_list(count, move |at| {
        let family = computed(move || rows.with(|r| r.get(at.index()).cloned().unwrap_or_default()));
        let family = move || family.get();
        let selected = move || current().as_deref() == Some(family().as_str());
        view()
            .h(px(44.0))
            .flex_row()
            .items_center()
            .gap(px(12.0))
            .px(px(12.0))
            .rounded(px(8.0))
            .role(AccessibilityRole::Button)
            .label(family)
            .style_when(selected, |st| st.bg(primary().opacity(0.12)))
            .hover(|st| st.bg(hover()))
            .active(|st| st.opacity(0.7))
            .on_press(move || super::store::set_font(Some(family())))
            // 每一行用它自己的字体预览。
            .child(
                text(move || format!("{} · 歌词 Aa あ", family()))
                    .flex_1()
                    .number_of_lines(1)
                    .font_family(move || SharedString::from(family()))
                    .text_size(px(15.0))
                    .text_color(fg()),
            )
            .child(when(selected, || icon(Icon::Play, 10.0, primary)))
    })
    .estimated_item_size(px(44.0))
    .empty(
        view()
            .h(px(80.0))
            .items_center()
            .justify_center()
            .child(text("没有匹配的字体").text_size(px(13.0)).text_color(muted_fg())),
    )
    .flex_1();

    let search = input()
        .flex_1()
        .h(px(32.0))
        .px(px(12.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(theme_input())
        .bg(bg())
        .text_size(px(14.0))
        .text_color(fg())
        .value(move || query.get())
        .placeholder("搜索已安装的字体")
        .on_change(move |text: &str| query.set(text.to_owned()));

    view()
        .gap(px(12.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(search)
                .child(
                    text(move || if loading() { "正在读取字体…".to_owned() } else { format!("{} 个", count.get()) })
                        .text_size(px(12.0))
                        .tabular_nums()
                        .text_color(faint()),
                )
                .child(icon_button(|| Icon::ChevronDown, 14.0, "收起字体列表", muted_fg, move || expanded.set(false))),
        )
        .child(view().h(px(264.0)).rounded(px(10.0)).bg(bg()).p(px(4.0)).overflow_hidden().child(list))
}

/// 播放：淡入淡出时长。
fn playback_card() -> View {
    let s = store();
    const STEPS: &[(u32, &str)] = &[(0, "关闭"), (100, "0.1 秒"), (200, "0.2 秒"), (400, "0.4 秒"), (800, "0.8 秒"), (1500, "1.5 秒")];
    let chip = move |ms: u32, label: &'static str| {
        let on = move || s.library.with(|l| l.fade_ms == ms);
        view()
            .h(px(32.0))
            .px(px(14.0))
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(move || if on() { primary() } else { border() })
            .bg(move || if on() { primary().opacity(0.12) } else { AdaptiveColor::TRANSPARENT })
            .role(AccessibilityRole::Button)
            .label(label)
            .accessibility_state(move || AccessibilityState { selected: on(), ..AccessibilityState::default() })
            .hover(|st| st.bg(hover()))
            .active(|st| st.opacity(0.7))
            .on_press(move || super::store::set_fade_ms(ms))
            .child(text(label).text_size(px(13.0)).font_weight(FontWeight::MEDIUM).text_color(fg()))
    };
    card_section(
        "淡入淡出",
        "播放、暂停、拖动进度和切歌时，音量在这段时间里渐变，不会突然响起或戛然而止。一首自然播完接下一首时不淡变。",
        None,
    )
    .child(view().flex_row().flex_wrap().gap(px(8.0)).children(STEPS.iter().map(|&(ms, label)| chip(ms, label))))
}

/// 全屏歌词的对齐方式。
fn lyrics_card() -> View {
    use crate::library::LyricsAlign;
    let s = store();
    const OPTIONS: &[(LyricsAlign, &str)] =
        &[(LyricsAlign::Left, "左对齐"), (LyricsAlign::Center, "居中"), (LyricsAlign::Right, "右对齐")];
    let chip = move |align: LyricsAlign, label: &'static str| {
        let on = move || s.library.with(|l| l.lyrics_align == align);
        view()
            .h(px(32.0))
            .px(px(14.0))
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(move || if on() { primary() } else { border() })
            .bg(move || if on() { primary().opacity(0.12) } else { AdaptiveColor::TRANSPARENT })
            .role(AccessibilityRole::Button)
            .label(label)
            .accessibility_state(move || AccessibilityState { selected: on(), ..AccessibilityState::default() })
            .hover(|st| st.bg(hover()))
            .active(|st| st.opacity(0.7))
            .on_press(move || super::store::set_lyrics_align(align))
            .child(text(label).text_size(px(13.0)).font_weight(FontWeight::MEDIUM).text_color(fg()))
    };
    card_section("歌词对齐", "全屏播放页里歌词的对齐方式。", None)
        .child(view().flex_row().flex_wrap().gap(px(8.0)).children(OPTIONS.iter().map(|&(a, label)| chip(a, label))))
}

/// 关闭窗口时：每次询问 / 最小化到托盘 / 退出。
fn window_card() -> View {
    use crate::library::CloseAction;
    let s = store();
    let option = move |action: CloseAction, label: &'static str, hint: &'static str| {
        let on = move || s.library.with(|l| l.close_action == action);
        view()
            .flex_1()
            .min_w(px(140.0))
            .gap(px(2.0))
            .p(px(12.0))
            .rounded(px(10.0))
            .border_1()
            .border_color(move || if on() { primary() } else { border() })
            .bg(move || if on() { primary().opacity(0.12) } else { AdaptiveColor::TRANSPARENT })
            .role(AccessibilityRole::Button)
            .label(label)
            .accessibility_state(move || AccessibilityState { selected: on(), ..AccessibilityState::default() })
            .hover(|st| st.bg(hover()))
            .active(|st| st.opacity(0.7))
            .on_press(move || super::store::set_close_action(action))
            .child(text(label).text_size(px(14.0)).font_weight(FontWeight::MEDIUM).text_color(fg()))
            .child(text(hint).text_size(px(12.0)).text_color(muted_fg()))
    };
    let supported = cfg!(any(target_os = "macos", target_os = "windows"));
    card_section(
        "关闭窗口时",
        if supported {
            "最小化到托盘后音乐继续播放，界面释放以节省内存；托盘图标可以打开窗口、播放 / 暂停、切歌和退出。"
        } else {
            "这个平台还没有托盘，关闭窗口即退出。"
        },
        None,
    )
    .child(supported.then(|| {
        view()
            .flex_row()
            .flex_wrap()
            .gap(px(8.0))
            .child(option(CloseAction::Ask, "每次询问", "第一次关闭时选择"))
            .child(option(CloseAction::Tray, "最小化到托盘", "继续播放"))
            .child(option(CloseAction::Quit, "退出", "停止播放并退出"))
    }))
}

#[component]
pub fn Settings() -> impl IntoElement {
    view().flex_1().child(page_header(|| "设置".into(), || "曲库、播放、窗口、标签与字体".into(), None, None)).child(
        scroll().flex_1().child(
            view()
                .px(px(32.0))
                .pb(px(BAR_SPACE))
                .gap(px(16.0))
                .max_w(px(820.0))
                .child(folders_card())
                .child(library_card())
                .child(playback_card())
                .child(lyrics_card())
                .child(window_card())
                .child(genres_card())
                .child(font_card()),
        ),
    )
}

fn theme_input() -> AdaptiveColor {
    nsh_base::theme().input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_decode() {
        assert_eq!(url_to_path("file:///Users/me/My%20Music/"), "/Users/me/My Music/");
        assert_eq!(url_to_path("file:///tmp/%E9%9F%B3%E4%B9%90"), "/tmp/音乐");
        assert_eq!(url_to_path("file:///C:/Users/me/My%20Music"), "C:\\Users\\me\\My Music");
        assert_eq!(last_segment("C:\\Users\\me\\Music\\"), "Music");
        assert_eq!(last_segment("/Users/me/Music"), "Music");
    }

    fn entry(path: &str) -> TrackEntry {
        TrackEntry {
            path: path.into(),
            file_name: String::new(),
            title: path.rsplit('/').next().unwrap().into(),
            artist: String::new(),
            album: String::new(),
            album_artist: String::new(),
            genres: Vec::new(),
            year: None,
            track: None,
            disk: None,
            duration: 0.0,
            bit_rate: None,
            sample_rate: None,
            channels: None,
            has_art: false,
            size_bytes: 0,
            modified: 0,
            stamp: 0,
        }
    }

    #[test]
    fn tree_groups_by_root_and_folder() {
        let tracks = vec![entry("/m/a/1.mp3"), entry("/m/a/b/2.mp3"), entry("/m/3.mp3"), entry("/x/4.mp3")];
        let tree = build_tree_with(&["/m".into()], &tracks, '/');
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].total, 3);
        assert_eq!(tree[0].tracks, vec![2]);
        let a = tree[0].find("/m/a").unwrap();
        assert_eq!(a.total, 2);
        let mut order = Vec::new();
        a.subtree(&mut order);
        assert_eq!(order, vec![1, 0]);
    }

    #[test]
    fn windows_paths_build_the_same_tree() {
        let tracks = vec![entry("C:\\m\\a\\1.mp3"), entry("C:\\m\\2.mp3"), entry("D:\\3.mp3")];
        let tree = build_tree_with(&["C:\\m".into(), "D:\\".into()], &tracks, '\\');
        // 根按名字排序：「D:」在「m」前。
        assert_eq!(tree.iter().map(|r| (r.name.as_str(), r.total)).collect::<Vec<_>>(), vec![("D:", 1), ("m", 2)]);
        let m = place_with(&tree, Some("C:\\m"), '\\');
        assert!(matches!(&m.rows[0], FolderRow::Folder { name, path, .. } if name == "a" && path == "C:\\m\\a"));
        let a = place_with(&tree, Some("C:\\m\\a"), '\\');
        assert_eq!(a.parent, Some(Some("C:\\m".into())));
        assert_eq!(a.trail, "m / a");
    }

    #[test]
    fn roots_are_listed_and_navigable() {
        let mut tracks = vec![entry("/m/a/1.mp3"), entry("/m/a/b/2.mp3"), entry("/m/3.mp3")];
        for (t, size) in tracks.iter_mut().zip([100, 200, 300]) {
            t.size_bytes = size;
            t.duration = 60.0;
        }
        let tree = build_tree_with(&["/m".into()], &tracks, '/');
        // 顶层：一个根也列出来。
        let top = place_with(&tree, None, '/');
        assert_eq!(top.path, None);
        assert_eq!(top.parent, None);
        assert!(matches!(&top.rows[..], [FolderRow::Folder { root: true, count: 3, bytes: 600, .. }]));
        // 根：子文件夹在前，再是本层曲目；返回顶层。
        let m = place_with(&tree, Some("/m"), '/');
        assert_eq!(m.parent, Some(None));
        assert_eq!(m.trail, "/m");
        assert!(matches!(&m.rows[0], FolderRow::Folder { name, root: false, count: 2, bytes: 300, .. } if name == "a"));
        assert!(matches!(m.rows[1], FolderRow::Track { index: 2, number: 1 }));
        let a = place_with(&tree, Some("/m/a"), '/');
        assert_eq!(a.parent, Some(Some("/m".into())));
        assert_eq!(a.trail, "m / a");
        assert_eq!(*a.queue, vec![0]);
        assert_eq!(*a.all, vec![1, 0]);
        // 多个根；不存在的位置回到顶层。
        let both = vec![entry("/m/1.mp3"), entry("/x/2.mp3")];
        let two = build_tree_with(&["/m".into(), "/x".into()], &both, '/');
        assert_eq!(place_with(&two, None, '/').rows.len(), 2);
        assert_eq!(place_with(&two, Some("/x"), '/').parent, Some(None));
        assert_eq!(place_with(&two, Some("/gone"), '/').path, None);
        assert_eq!(format_total_short(3725.0), "1:02:05");
        assert_eq!(audio_format("/m/a.flac"), "FLAC");
        assert_eq!(audio_format("C:\\m\\b.Mp3"), "MP3");
        assert_eq!(format_bytes(1536 * 1024), "1.5 MB");
    }
}
