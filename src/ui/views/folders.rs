//! 「文件夹」页：左侧是曲库根目录的目录树（只列文件夹，可折叠；选中项
//! 的祖先总是展开），右侧是所选文件夹直接包含的曲目。把文件夹 / 音频
//! 文件拖进来会加进曲库：已在某个根目录下的只定位过去，否则它（或它
//! 所在的目录）加为新根。右侧的曲目行与树里的文件夹行也可以拖到某个
//! 树行上，把文件 / 文件夹真正移动过去（`fs::rename`，被移走的曲库根
//! 目录会同步改路径）。树行右键菜单做增删改：新建子文件夹、重命名、
//! 删除（回收站 / 直接删按「删除文件」设置）；曲目行也能重命名文件。

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use nsh_base::context_menu;
use nsh_core::prelude::*;

use super::super::components::button::{Size, Variant, icon_button, pill_button, text_button};
use super::super::components::form::{
    dialog_content, dialog_footer, dialog_header, input_field, modal,
};
use super::super::components::icons::{Icon, icon, icon_dyn};
use super::super::components::page::{empty_state, page_header};
use super::super::components::tracks::{Columns, ROW, format_bytes, track_columns, track_row};
use super::super::now_playing::{REVEAL_LABEL, reveal};
use super::super::store::{self, Same, play_list, player, scan, store};
use super::super::theme::*;
use super::{
    SEP, add_dropped, by_disc_track, compare, item, last_segment, no_library, popup, separator,
    url_to_path,
};
use crate::config::{TrackEntry, library_store};

/// 文件夹：面向文件，带大小列。
const FOLDER_COLS: Columns = Columns {
    cover: true,
    album: true,
    size: true,
    format: true,
};

/// 目录树行高与每层缩进。
const TREE_ROW: f32 = 30.0;
const INDENT: f32 = 14.0;

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
        let one_album = self
            .tracks
            .windows(2)
            .all(|w| all[w[0]].album == all[w[1]].album);
        if one_album {
            self.tracks
                .sort_by(|a, b| by_disc_track(&all[*a], &all[*b]));
        } else {
            self.tracks
                .sort_by(|a, b| compare(&all[*a].file_name, &all[*b].file_name));
        }
        self.total = self.tracks.len()
            + self
                .children
                .values_mut()
                .map(|c| c.finish(all))
                .sum::<usize>();
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

fn build_tree(roots: &[String], tracks: &[TrackEntry], extra: &[String]) -> Vec<Folder> {
    build_tree_with(roots, tracks, extra, SEP)
}

fn build_tree_with(
    roots: &[String],
    tracks: &[TrackEntry],
    extra: &[String],
    sep: char,
) -> Vec<Folder> {
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
            current = current
                .children
                .entry(compare_key(part))
                .or_insert_with(|| Folder {
                    path,
                    name: (*part).to_owned(),
                    ..Folder::default()
                });
        }
        current.tracks.push(index);
    }
    // 曲目推不出的空文件夹（本次会话新建 / 移进来的）也挂进树，按同
    // 样的路径逐级保证链存在（已有的不重挂）。
    for dir in extra {
        let Some(node) = nodes.iter_mut().find(|n| {
            let root = n.path.trim_end_matches(sep);
            dir.starts_with(root) && dir[root.len()..].starts_with(sep)
        }) else {
            continue;
        };
        let rest = dir[node.path.trim_end_matches(sep).len()..].trim_matches(sep);
        let mut current = node;
        for part in rest.split(sep).filter(|p| !p.is_empty()) {
            let path = format!("{}{sep}{part}", current.path.trim_end_matches(sep));
            current = current
                .children
                .entry(compare_key(part))
                .or_insert_with(|| Folder {
                    path,
                    name: (*part).to_owned(),
                    ..Folder::default()
                });
        }
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

/// 右侧显示的位置：`path` 为实际显示的文件夹（选中项失效或还没选时回
/// 退到第一个根文件夹；没有根目录为 `None`，由 `no_library` 接住）。
#[derive(Clone, PartialEq)]
struct Place {
    path: Option<String>,
    title: String,
    trail: String,
    folders: usize,
    tracks: usize,
    bytes: u64,
    /// 本文件夹直接包含的曲目（列表与点播队列）。
    files: Rc<Vec<usize>>,
    /// 子树全部曲目（「播放全部」）。
    all: Rc<Vec<usize>>,
}

fn place(tree: &[Folder], at: Option<&str>) -> Place {
    place_with(tree, at, SEP)
}

fn place_with(tree: &[Folder], at: Option<&str>, sep: char) -> Place {
    let node = at
        .and_then(|p| tree.iter().find_map(|root| root.find(p)))
        .or_else(|| tree.first());
    let Some(node) = node else {
        return Place {
            path: None,
            title: "文件夹".into(),
            trail: String::new(),
            folders: 0,
            tracks: 0,
            bytes: 0,
            files: Rc::default(),
            all: Rc::default(),
        };
    };
    let under = |r: &Folder| {
        let root = r.path.trim_end_matches(sep);
        node.path == r.path
            || (node.path.starts_with(root) && node.path[root.len()..].starts_with(sep))
    };
    let root = tree.iter().find(|r| under(r));
    let trail = root
        .map(|r| {
            let rest = node.path[r.path.trim_end_matches(sep).len()..].trim_start_matches(sep);
            if rest.is_empty() {
                r.path.clone()
            } else {
                format!("{} / {}", r.name, rest.replace(sep, " / "))
            }
        })
        .unwrap_or_default();
    let mut all = Vec::new();
    node.subtree(&mut all);
    Place {
        path: Some(node.path.clone()),
        title: node.name.clone(),
        trail,
        folders: node.children.len(),
        tracks: node.total,
        bytes: node.bytes,
        files: Rc::new(node.tracks.clone()),
        all: Rc::new(all),
    }
}

/// 树里可见的一行（折叠状态已算进列表）。
#[derive(Clone, PartialEq)]
struct TreeRow {
    path: String,
    name: String,
    depth: usize,
    has_children: bool,
    open: bool,
    count: usize,
}

/// `expanded` 里的文件夹展开；`selected` 的祖先总是展开（选中项留在视野里）。
fn flatten_with(
    tree: &[Folder],
    expanded: &BTreeSet<String>,
    selected: Option<&str>,
    sep: char,
) -> Vec<TreeRow> {
    fn walk(
        node: &Folder,
        depth: usize,
        expanded: &BTreeSet<String>,
        selected: Option<&str>,
        sep: char,
        out: &mut Vec<TreeRow>,
    ) {
        // 严格后代（`path/` 前缀）才算祖先展开。
        let under = selected
            .is_some_and(|s| s.starts_with(&format!("{}{sep}", node.path.trim_end_matches(sep))));
        let open = expanded.contains(&node.path) || under;
        out.push(TreeRow {
            path: node.path.clone(),
            name: node.name.clone(),
            depth,
            has_children: !node.children.is_empty(),
            open,
            count: node.total,
        });
        if open {
            for child in node.children.values() {
                walk(child, depth + 1, expanded, selected, sep, out);
            }
        }
    }
    let mut out = Vec::new();
    for root in tree {
        walk(root, 0, expanded, selected, sep, &mut out);
    }
    out
}

fn play_indices(indices: &[usize], start: usize) {
    let all = store().tracks.get_untracked();
    let queue: Vec<TrackEntry> = indices
        .iter()
        .filter_map(|&i| all.get(i).cloned())
        .collect();
    play_list(&queue, start);
}

// --- 应用内拖动 ----------------------------------------------------------------

/// 拖过这段距离（pt）才接管手势：更小的位移仍是点击。
const DRAG_SLOP: f32 = 6.0;

/// `source`（文件或文件夹路径）能不能落到 `target` 文件夹：不能落到自
/// 身、原来的父目录（等于没动），文件夹也不能进自己或自己的子树。
fn drop_ok(source: &str, dir: bool, target: &str, sep: char) -> bool {
    let src = source.trim_end_matches(sep);
    let dst = target.trim_end_matches(sep);
    if dst.is_empty() || src == dst {
        return false;
    }
    let parent = src.rfind(sep).map(|i| &src[..i]).unwrap_or("");
    if parent == dst {
        return false;
    }
    if dir && dst.starts_with(&format!("{src}{sep}")) {
        return false;
    }
    true
}

/// 把 `source` 移进 `target` 目录，返回新路径；目标里已有同名项、或文件
/// 系统拒绝（如跨磁盘 rename）时报错，由调用方提示。
fn move_into(source: &str, target: &str, sep: char) -> Result<String, String> {
    let name = last_segment(source);
    let dest = format!("{}{sep}{name}", target.trim_end_matches(sep));
    if std::path::Path::new(&dest).exists() {
        return Err(format!("{target} 里已有 {name}"));
    }
    std::fs::rename(source, &dest).map_err(|e| format!("移动 {name} 失败：{e}"))?;
    Ok(dest)
}

/// 一次拖动中的源条目。
#[derive(Clone, PartialEq)]
struct DragState {
    /// 被拖的文件 / 文件夹的绝对路径。
    source: String,
    /// 拖的是文件夹（移动后要修根目录、校验子树）。
    dir: bool,
    /// 拖动浮层上的名字。
    name: String,
}

/// 文件行与树行共享的拖动会话：手势接管、ghost 跟随、命中树行、落下移动。
struct DragCtx {
    /// `Some` 表示正在拖。
    active: Signal<Option<DragState>>,
    /// 命中且合法的目标树行路径（树行据此高亮）。
    over: Signal<Option<String>>,
    /// ghost 的锚点（拖拽起点在本页根 view 里的位置）。
    anchor: Signal<(f32, f32)>,
    /// 本次会话建出来还没曲目的空文件夹（移动时要跟着换前缀）。
    extra: Signal<BTreeSet<String>>,
    /// 当前可见树行（命中判定用）。
    rows: Memo<Rc<Vec<TreeRow>>>,
    /// 树列表的滚动 y 偏移。
    scroll: Signal<f32>,
    /// 树列表在 surface 里的位置（x, y, w），开始拖时量。
    geo: Rc<Cell<Option<(f32, f32, f32)>>>,
    /// 接管时记下的按下点（surface 坐标）：原生 pan 的位移基准点，
    /// ghost 锚在这里再加 nudge 才贴着指针。
    down: Rc<Cell<Option<Point<Points>>>>,
    /// ghost 的 translate：行的 `pan_values` 绑到这两个值，拖动的位移
    /// 由平台原生写入，每帧零引擎开销（别用 `set` 逐帧推它）。
    gx: AnimatedValue,
    gy: AnimatedValue,
    /// 测量用：树列表与本页根 view。
    list: ElementRef,
    root: ElementRef,
    /// 移动成功后定位（新路径或目标文件夹）。
    select: Rc<dyn Fn(&str)>,
    /// 结果提示（页头副标题短暂显示）。
    message: Signal<String>,
}

impl DragCtx {
    /// 行的拖动手势：超过 `DRAG_SLOP` 就接管；桌面端鼠标拖动不滚列表，
    /// 不会和滚动争。`block_native(false)`：行的 `pan_values` 也要跟着
    /// 这个拖走原生位移（接管默认会杀掉它）。
    fn claim(&self) -> TouchClaim {
        let down = self.down.clone();
        TouchClaim::on_move()
            .block_native(false)
            .ask(move |req: &ClaimRequest| {
                let d = req.event.gesture.delta;
                if d.x.0.hypot(d.y.0) > DRAG_SLOP {
                    down.set(Some(req.event.gesture.origin));
                    req.claim();
                }
            })
    }

    /// 指针（surface 坐标）下的树行路径。
    fn target_at(&self, page: Point<Points>) -> Option<String> {
        let (ox, oy, w) = self.geo.get()?;
        if !(ox..=ox + w).contains(&page.x.0) {
            return None;
        }
        let index = ((page.y.0 - oy + self.scroll.get_untracked()) / TREE_ROW).floor();
        if index < 0.0 {
            return None;
        }
        self.rows
            .with_untracked(|r| r.get(index as usize).map(|r| r.path.clone()))
    }

    fn begin(&self, source: String, dir: bool, name: String) {
        self.active.set(Some(DragState { source, dir, name }));
        self.over.set(None);
        // 量出来之前把 ghost 停在窗外，免得沿上次的锚点闪一下。
        self.anchor.set((-10000.0, -10000.0));
        let (down, geo, anchor) = (self.down.clone(), self.geo.clone(), self.anchor);
        let (list, root) = (self.list, self.root);
        spawn(async move {
            if let Some(m) = root.measure().await {
                if let Some(d) = down.get() {
                    // ghost 锚在按下点（本页根 view 坐标）+ nudge；pan
                    // 值是「当前位置 - 按下点」，加起来正好贴着指针。
                    anchor.set((d.x.0 - m.page.x.0 + 10.0, d.y.0 - m.page.y.0 + 12.0));
                }
            }
            if let Some(m) = list.measure().await {
                geo.set(Some((m.page.x.0, m.page.y.0, m.frame.size.width.0)));
            }
        });
    }

    fn move_to(&self, page: Point<Points>) {
        let over = self.target_at(page).filter(|target| {
            self.active.with_untracked(|a| {
                a.as_ref()
                    .is_some_and(|d| drop_ok(&d.source, d.dir, target, SEP))
            })
        });
        self.over.set(over);
    }

    /// 落下：按松手位置重算目标（`over` 只是拖动中的高亮），命中就移动；
    /// 失败留一行提示。
    fn finish(&self, page: Point<Points>) {
        self.move_to(page);
        let (Some(d), Some(target)) = (self.active.get_untracked(), self.over.get_untracked())
        else {
            self.cancel();
            return;
        };
        self.cancel();
        // 正在播的条目先停掉：Windows 上解码器还握着文件会移不动。
        stop_if_playing_under(&d.source);
        match move_into(&d.source, &target, SEP) {
            Ok(dest) => {
                // 被移走的是曲库根目录、装着根目录或在歌单 / 空文件夹
                // 记录里：引用与索引记录跟着改路径（不重扫不重读）。
                move_refs(&d.source, &dest, &self.extra);
                (self.select)(if d.dir { &dest } else { &target });
                show_message(self.message, format!("已把 {} 移到 {target}", d.name));
            }
            Err(error) => show_message(self.message, error),
        }
    }

    fn cancel(&self) {
        self.active.set(None);
        self.over.set(None);
        // pan 值回到 0，下次 `pan_began` 从 0 起算。release 先到引擎
        // 再到平台的最后一次写入，这个 set 落地在最后。
        self.gx.set(0.0);
        self.gy.set(0.0);
        self.down.set(None);
    }
}

/// 拖放结果 / 操作提示：副标题显示几秒后自动清掉（期间被新提示顶掉则
/// 不动）。
fn show_message(message: Signal<String>, text: String) {
    message.set(text.clone());
    spawn(async move {
        timer(Duration::from_secs(5)).await;
        message.update(|m| {
            if *m == text {
                m.clear();
            }
        });
    });
}

// --- 文件树增删改 --------------------------------------------------------------

/// 树行 / 曲目行的操作请求：`Some` 时对应弹窗挂着。
#[derive(Clone, PartialEq)]
enum FileOp {
    /// 在 `parent` 下新建文件夹。
    NewFolder { parent: String },
    /// 重命名 `path`（`dir`：文件夹；否则是文件，输入框只改主名、扩展名
    /// 沿用）。
    Rename { path: String, dir: bool },
    /// 删除文件夹 `path`（含其中全部内容）。
    DeleteFolder { path: String },
}

/// 新建 / 重命名的目标名：非空、不含路径分隔符、不是 `.` / `..`。
fn valid_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("名字不能为空".into());
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err("名字里不能含 / 或 \\".into());
    }
    Ok(name)
}

/// `path` 的父目录（去掉尾部分隔符再取前缀）；根路径没有父目录。
fn parent_dir(path: &str, sep: char) -> Option<String> {
    let trimmed = path.trim_end_matches(sep);
    trimmed.rfind(sep).map(|i| {
        let parent = &trimmed[..i];
        if parent.is_empty() {
            // `/a` 的父目录是 `/`。
            sep.to_string()
        } else if parent.len() == 2 && parent.ends_with(':') {
            // `C:\a` 的父目录是 `C:\`。
            format!("{parent}{sep}")
        } else {
            parent.to_owned()
        }
    })
}

/// 重命名输入框的初值：文件夹是整个名字，文件只给主名（扩展名保留，
/// 不进输入框）。
fn rename_default(path: &str, dir: bool) -> String {
    let file = last_segment(path);
    if dir {
        return file.to_owned();
    }
    Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_owned())
}

/// 重命名的目标路径：文件夹整个名字换掉；文件只改主名、扩展名沿用
/// （输入已带相同扩展名时不重复拼）。
fn rename_dest(path: &str, dir: bool, name: &str, sep: char) -> Result<String, String> {
    let name = valid_name(name)?;
    let Some(parent) = parent_dir(path, sep) else {
        return Err("磁盘根目录不能重命名".into());
    };
    let file_name = if dir {
        name.to_owned()
    } else {
        let ext = Path::new(path.trim_end_matches(sep))
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        if ext.is_empty()
            || name
                .to_lowercase()
                .ends_with(&format!(".{}", ext.to_lowercase()))
        {
            name.to_owned()
        } else {
            format!("{name}.{ext}")
        }
    };
    Ok(format!("{}{sep}{file_name}", parent.trim_end_matches(sep)))
}

/// `src` 移 / 改名到 `dest`：曲库根、曲目索引与标签缓存 / 数据库
/// （`move_path`）、手动歌单与会话里的空文件夹记录一起换前缀（等于
/// `src` 或以 `src/` 为前缀的都算）——内容没变，不重读标签也不重扫。
fn move_refs(src: &str, dest: &str, extra: &Signal<BTreeSet<String>>) {
    let (src, dest) = (
        src.trim_end_matches(SEP).to_owned(),
        dest.trim_end_matches(SEP).to_owned(),
    );
    let under = format!("{src}{SEP}");
    library_store().move_path(&src, &dest);
    store::move_playlist_paths(&src, &dest);
    extra.update(|e| {
        let moved: Vec<String> = e
            .iter()
            .filter(|d| *d == &src || d.starts_with(&under))
            .cloned()
            .collect();
        for d in moved {
            e.remove(&d);
            e.insert(format!("{dest}{}", &d[src.len()..]));
        }
    });
}

/// `path` 下（含自身）的曲库根摘掉、索引与元数据记录清掉、歌单条目
/// 移除（文件已没，留着是死条目）。
fn remove_refs_under(path: &str) {
    let trimmed = path.trim_end_matches(SEP);
    let under = format!("{trimmed}{SEP}");
    let lib = library_store();
    for root in lib.roots() {
        let r = root.trim_end_matches(SEP);
        if r == trimmed || r.starts_with(&under) {
            lib.remove_root(&root);
        }
    }
    store::remove_playlist_paths_under(trimmed);
    lib.remove_under(trimmed);
}

/// 正在播的曲目在 `path` 里（含自身）就先停掉：文件马上要被移走或删
/// 掉，Windows 上解码器还握着文件会移不动。后台线程跑（`store()` 只
/// 在组件上下文里），用 `player()` 的快照看当前曲。
fn stop_if_playing_under(path: &str) {
    let trimmed = path.trim_end_matches(SEP);
    let under = format!("{trimmed}{SEP}");
    if player()
        .state()
        .path
        .is_some_and(|p| p == trimmed || p.starts_with(&under))
    {
        player().stop();
    }
}

/// 增删改的执行者与弹窗状态：文件操作在后台线程做（正在播放的文件在
/// Windows 上可能被解码器握着，先停掉再多试几次），完成后回来修曲库
/// 引用、重扫并定位。
struct Ops {
    /// `Some` 时对应弹窗挂着。
    op: Signal<Option<FileOp>>,
    /// 本次会话建出来还没曲目的空文件夹：靠它挂进树（树由曲目路径推
    /// 导，磁盘上已没了的在构建时滤掉）。
    extra: Signal<BTreeSet<String>>,
    /// 定位用：选中的文件夹。
    cwd: Signal<Option<String>>,
    /// 折叠集合（定位时把目标从里面摘掉让它展开）。
    expanded: Signal<BTreeSet<String>>,
    /// 结果提示（页头副标题短暂显示）。
    message: Signal<String>,
}

/// 操作完成后定位到某个文件夹：选中并保证它展开。
fn select_folder(cwd: Signal<Option<String>>, expanded: Signal<BTreeSet<String>>, path: &str) {
    cwd.set(Some(path.to_owned()));
    expanded.update(|e| {
        e.insert(path.to_owned());
    });
}

impl Ops {
    fn close(&self) {
        self.op.set(None);
    }

    fn select(&self, path: &str) {
        select_folder(self.cwd, self.expanded, path);
    }

    /// 「新建文件夹」/「重命名」的确定：校验名字并算出目标路径（不合
    /// 法 / 已存在时把错误文案留在弹窗里），合法就关掉弹窗开工。
    fn submit_name(&self, name: &str) -> Result<(), String> {
        let Some(op) = self.op.get_untracked() else {
            return Ok(());
        };
        match op {
            FileOp::NewFolder { parent } => {
                let name = valid_name(name)?.to_owned();
                let path = format!("{}{SEP}{name}", parent.trim_end_matches(SEP));
                if Path::new(&path).exists() {
                    return Err("已存在同名的文件或文件夹".into());
                }
                std::fs::create_dir(&path).map_err(|e| format!("创建失败：{e}"))?;
                self.extra.update(|e| {
                    e.insert(path.clone());
                });
                self.close();
                self.select(&path);
                show_message(self.message, format!("已创建文件夹 {name}"));
                Ok(())
            }
            FileOp::Rename { path, dir } => {
                let dest = rename_dest(&path, dir, name, SEP)?;
                if dest == path.trim_end_matches(SEP) {
                    // 名字没变：直接关，什么都不做。
                    self.close();
                    return Ok(());
                }
                if Path::new(&dest).exists() {
                    return Err("已存在同名的文件或文件夹".into());
                }
                self.close();
                self.rename(path, dest, dir);
                Ok(())
            }
            FileOp::DeleteFolder { .. } => Ok(()),
        }
    }

    /// 后台改名：文件可能被播放器握着（Windows），停掉后多试几次；成
    /// 功再修曲库引用、重扫并定位。
    fn rename(&self, src: String, dest: String, dir: bool) {
        let (cwd, expanded, message, extra) = (self.cwd, self.expanded, self.message, self.extra);
        let ui = store::poster();
        std::thread::spawn(move || {
            stop_if_playing_under(&src);
            if !Path::new(&src).exists() {
                return ui.post(move || {
                    show_message(message, "原路径已不存在".into());
                });
            }
            let mut done = false;
            for _ in 0..10 {
                if std::fs::rename(&src, &dest).is_ok() {
                    done = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            let name = last_segment(&dest).to_owned();
            let target = if dir {
                dest.clone()
            } else {
                parent_dir(&dest, SEP).unwrap_or_else(|| dest.clone())
            };
            ui.post(move || {
                if done {
                    move_refs(&src, &dest, &extra);
                    select_folder(cwd, expanded, &target);
                    show_message(message, format!("已重命名为 {name}"));
                } else {
                    show_message(message, format!("重命名 {name} 失败"));
                }
            });
        });
    }

    /// 「删除文件夹」确认：后台删（回收站 / 直接删按设置），成功后清
    /// 掉其中的曲库根、曲目记录与歌单条目，重扫并定位到父目录。
    fn run_delete(&self, path: String) {
        self.close();
        let permanent = library_store().delete_permanently();
        let (cwd, expanded, message, extra) = (self.cwd, self.expanded, self.message, self.extra);
        let ui = store::poster();
        std::thread::spawn(move || {
            stop_if_playing_under(&path);
            let dir = Path::new(&path);
            let mut gone = !dir.exists();
            for _ in 0..10 {
                if gone {
                    break;
                }
                if crate::provider::local::delete_dir(dir, permanent).is_ok() {
                    gone = true;
                } else {
                    std::thread::sleep(Duration::from_millis(150));
                }
            }
            let name = last_segment(&path).to_owned();
            let parent = parent_dir(&path, SEP).unwrap_or_else(|| path.clone());
            ui.post(move || {
                if gone {
                    remove_refs_under(&path);
                    let under = format!("{}{SEP}", path.trim_end_matches(SEP));
                    extra.update(|e| {
                        e.retain(|d| !(d == &path || d.starts_with(&under)));
                    });
                    select_folder(cwd, expanded, &parent);
                    show_message(message, format!("已删除 {name}"));
                } else {
                    show_message(message, format!("删除 {name} 失败"));
                }
            });
        });
    }
}

/// 「新建子文件夹」/「重命名」/「删除文件夹」的弹窗（`ops.op` 有请求
/// 时挂载，关掉即卸）。
fn op_dialog(ops: Rc<Ops>) -> AnyElement {
    let Some(op) = ops.op.get_untracked() else {
        return view().into_element();
    };
    let close = {
        let op = ops.op;
        move || op.set(None)
    };
    match op {
        FileOp::DeleteFolder { path } => delete_folder_dialog(ops, path, close),
        FileOp::NewFolder { .. } | FileOp::Rename { .. } => name_dialog(ops, op, close),
    }
}

/// 新建 / 重命名：一个输入框 + 取消 / 确定；回车即确定（`on_submit`），
/// 名字不合法 / 已存在时错误文案留在弹窗里。
fn name_dialog(ops: Rc<Ops>, op: FileOp, close: impl Fn() + Copy + 'static) -> AnyElement {
    let (heading, default, confirm) = match &op {
        FileOp::NewFolder { .. } => ("新建文件夹", String::new(), "创建"),
        FileOp::Rename { path, dir } => (
            if *dir {
                "重命名文件夹"
            } else {
                "重命名文件"
            },
            rename_default(path, *dir),
            "重命名",
        ),
        FileOp::DeleteFolder { .. } => return view().into_element(),
    };
    let draft = signal(default);
    let err = signal(String::new());
    let submit = Rc::new({
        let ops = ops.clone();
        move || {
            let name = draft.get_untracked();
            if let Err(e) = ops.submit_name(&name) {
                err.set(e);
            }
        }
    });
    let content = dialog_content(425.0)
        .child(dialog_header(heading, None))
        .child(
            input_field(false)
                .w_full()
                .label("名称")
                .auto_focus(true)
                .default_value(untracked(|| draft.get()))
                .placeholder("名称")
                .on_change(move |t: &str| {
                    draft.set(t.to_owned());
                    err.set(String::new());
                })
                .on_submit({
                    let submit = submit.clone();
                    move |_: &str| submit()
                }),
        )
        .child(
            text(move || err.get())
                .number_of_lines(1)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(move || nsh_base::theme().destructive),
        )
        .child(
            dialog_footer()
                .child(text_button(
                    Variant::Outline,
                    Size::Default,
                    None,
                    "取消",
                    close,
                ))
                .child(text_button(
                    Variant::Default,
                    Size::Default,
                    None,
                    confirm,
                    move || submit(),
                )),
        );
    modal(content, close)
}

/// 「删除文件夹」确认弹窗（回收站 / 直接删按「删除文件」设置）：整棵
/// 子树都没了，曲库里的曲目记录也一并移除。
fn delete_folder_dialog(
    ops: Rc<Ops>,
    path: String,
    close: impl Fn() + Copy + 'static,
) -> AnyElement {
    let permanent = library_store().delete_permanently();
    let (desc, confirm) = if permanent {
        (
            "将永久删除整个文件夹（含其中的文件，不经过回收站，无法恢复），曲库记录一并移除。",
            "永久删除",
        )
    } else {
        ("将把整个文件夹移到回收站，曲库记录一并移除。", "移到回收站")
    };
    let content = dialog_content(440.0)
        .child(dialog_header("删除文件夹", Some(desc)))
        .child(
            text(last_segment(&path).to_owned())
                .number_of_lines(1)
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(
            dialog_footer()
                .child(text_button(
                    Variant::Outline,
                    Size::Default,
                    None,
                    "取消",
                    close,
                ))
                .child(text_button(
                    Variant::Destructive,
                    Size::Default,
                    None,
                    confirm,
                    move || ops.run_delete(path.clone()),
                )),
        );
    modal(content, close)
}

/// 树行右键菜单：新建子文件夹 / 重命名 / 在文件管理器中显示 / 删除。
/// 根目录行特殊：它是曲库根（设置页管），不给重命名 / 删除，改成
/// 「从曲库移除」（只摘配置，不动文件）。
fn tree_menu(trigger: View, row: Memo<Option<TreeRow>>, ops: Rc<Ops>) -> AnyElement {
    let gate = store::menu_control();
    let menu = context_menu()
        .trigger(trigger)
        .open(gate.open())
        .on_open_change(gate.menu_change());
    let path = move || row.with_untracked(|r| r.as_ref().map(|r| r.path.clone()));
    let is_root = row.with_untracked(|r| r.as_ref().is_some_and(|r| r.depth == 0));
    let message = ops.message;
    let open = move |make: fn(String) -> FileOp| {
        let ops = ops.clone();
        move || {
            if let Some(p) = path() {
                ops.op.set(Some(make(p)));
            }
        }
    };
    popup!(menu, 200.0)
        .child(item(
            "新建子文件夹…",
            Some(Icon::Plus),
            open(|p| FileOp::NewFolder { parent: p }),
        ))
        .child((!is_root).then(|| {
            item(
                "重命名…",
                Some(Icon::Edit),
                open(|p| FileOp::Rename { path: p, dir: true }),
            )
        }))
        .child(item(REVEAL_LABEL, Some(Icon::Reveal), {
            let path = path;
            move || {
                if let Some(p) = path() {
                    reveal(&p);
                }
            }
        }))
        .child(separator())
        .child(if is_root {
            Some(item("从曲库移除", Some(Icon::Close), move || {
                if let Some(p) = path() {
                    // 等菜单关掉再摘：行会随树重建一起拆掉。
                    store::later(move || {
                        library_store().remove_root(&p);
                        // 根摘掉了：其下的索引与元数据记录一起清（文件还
                        // 在，歌单条目保留——重新加根后又出现）。
                        library_store().remove_under(&p);
                        show_message(message, format!("已移除 {}", last_segment(&p)));
                    });
                }
            }))
        } else {
            Some(item(
                "删除文件夹…",
                Some(Icon::Trash),
                open(|p| FileOp::DeleteFolder { path: p }),
            ))
        })
        .into_element()
}

#[component]
pub fn Folders() -> impl IntoElement {
    let s = store();
    let cwd = signal(None::<String>);
    // 根文件夹默认展开（构造时从配置同步拿到根目录）。
    let expanded = signal(
        s.library
            .with_untracked(|l| l.roots.iter().cloned().collect::<BTreeSet<String>>()),
    );
    let message = signal(String::new());
    let dropping = signal(false);
    // 增删改：本次会话建出来还没曲目的空文件夹也挂进树（树本来由曲目
    // 路径推导）；磁盘上没了的在构建时滤掉。
    let extra = signal(BTreeSet::<String>::new());
    let op = signal(None::<FileOp>);
    let tree = computed(move || {
        let roots = s.library.with(|l| l.roots.clone());
        let tracks = s.tracks.get();
        let extra: Vec<String> = extra
            .get()
            .into_iter()
            .filter(|d| Path::new(d).is_dir())
            .collect();
        let t = build_tree(&roots, &tracks, &extra);
        Same(Arc::new(t))
    });
    let here = computed(move || {
        let tree = tree.get();
        place(&tree, cwd.get().as_deref())
    });

    // 选中并展开它；祖先总是展开（`flatten_with`），不用逐个写。
    let select: Rc<dyn Fn(&str)> = Rc::new(move |path: &str| {
        cwd.set(Some(path.to_owned()));
        expanded.update(|e| {
            e.insert(path.to_owned());
        });
    });
    let toggle: Rc<dyn Fn(&str, bool)> = Rc::new(move |path: &str, open: bool| {
        expanded.update(|e| {
            if open {
                e.remove(path);
            } else {
                e.insert(path.to_owned());
            }
        });
    });
    // 增删改：树行右键菜单与曲目行的「重命名文件…」都走它。
    let ops = Rc::new(Ops {
        op,
        extra,
        cwd,
        expanded,
        message,
    });
    // `otherwise` 的闭包会把 `ops` 搬走，弹窗这边先留一份。
    let ops_dialog = ops.clone();

    let rows = computed(move || {
        let selected = here.with(|p| p.path.clone());
        let r = flatten_with(&tree.get(), &expanded.get(), selected.as_deref(), SEP);
        Rc::new(r)
    });
    let files = computed(move || here.with(|p| p.files.clone()));

    // 应用内拖动（文件行 / 文件夹行 → 树行）：ghost 跟随 + 目标高亮 +
    // 落下移动。
    let active = signal(None::<DragState>);
    let over = signal(None::<String>);
    // 拖动浮层的锚点（按下点在本页根 view 里的位置），量出来前停在窗外。
    let anchor = signal((-10000.0, -10000.0));
    let scroll = signal(0.0_f32);
    let gx = animated_value(0.0);
    let gy = animated_value(0.0);
    let list_ref = element_ref();
    let root_ref = element_ref();
    let ctx = Rc::new(DragCtx {
        active,
        over,
        anchor,
        extra,
        rows,
        scroll,
        geo: Rc::new(Cell::new(None)),
        down: Rc::new(Cell::new(None)),
        gx,
        gy,
        list: list_ref,
        root: root_ref,
        select: select.clone(),
        message,
    });

    let on_drop = {
        let select = select.clone();
        move |event: &FileDropEvent| {
            dropping.set(false);
            let mut dir = None;
            let mut added = false;
            let notes: Vec<String> = event
                .uris
                .iter()
                .map(|uri| {
                    let dropped = add_dropped(&url_to_path(uri));
                    added |= dropped.added;
                    if dir.is_none() {
                        dir = dropped.dir;
                    }
                    dropped.note
                })
                .collect();
            if added {
                scan();
            }
            if let Some(dir) = dir {
                select(&dir);
            }
            // 页头副标题只有一行，多条结果用分号连。
            show_message(message, notes.join("；"));
        }
    };

    view()
        .flex_1()
        .element_ref(&root_ref)
        .border_1()
        .border_color(move || {
            if dropping.get() {
                primary()
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .bg(move || {
            if dropping.get() {
                hover()
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .transition_colors()
        .duration(300.0)
        .style_when(
            move || active.with(|a| a.is_some()),
            |st| st.cursor(Cursor::Grabbing),
        )
        .on_file_drag_enter(move || dropping.set(true))
        .on_file_drag_leave(move || dropping.set(false))
        .on_file_drop(on_drop)
        .child(
            when(move || s.library.with(|l| l.roots.is_empty()), no_library).otherwise(move || {
                let (select, toggle, ctx, ops) =
                    (select.clone(), toggle.clone(), ctx.clone(), ops.clone());
                let cell_ctx = ctx.clone();
                let cell_ops = ops.clone();
                let tree_list = virtual_list(computed(move || rows.with(|r| r.len())), move |at| {
                    let row = computed(move || rows.with(|r| r.get(at.index()).cloned()));
                    tree_cell(
                        row,
                        here,
                        cell_ctx.clone(),
                        cell_ops.clone(),
                        select.clone(),
                        toggle.clone(),
                    )
                })
                // 路径作 key：cell 按行绑定（回收时行不变），树行右键
                // 菜单里的行才不会错位。
                .keys(move || {
                    rows.get()
                        .iter()
                        .map(|r| r.path.clone())
                        .collect::<Vec<_>>()
                })
                .estimated_item_size(px(TREE_ROW))
                .content_padding_x(px(8.0))
                .content_padding_bottom(px(BAR_SPACE))
                .element_ref(&list_ref)
                .on_scroll(move |e: &ScrollEvent| scroll.set(e.offset.y.0))
                .empty(empty_state(Icon::Folder, "没有文件夹", ""))
                .flex_1();
                let tree_pane = view()
                    .w(px(FOLDERS_TREE))
                    .flex_shrink_0()
                    .border_r_1()
                    .border_r_color(border())
                    .child(
                        view()
                            .px(px(14.0))
                            .pt(px(14.0))
                            .pb(px(6.0))
                            .window_drag(true)
                            .child(
                                text("文件夹")
                                    .text_size(px(11.0))
                                    .line_height(px(11.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .letter_spacing(px(0.6))
                                    .text_color(faint()),
                            ),
                    )
                    .child(tree_list);

                let subtitle = move || {
                    let note = message.get();
                    if !note.is_empty() {
                        return note;
                    }
                    here.with(|p| {
                        let mut parts = Vec::new();
                        if !p.trail.is_empty() && p.trail != p.title {
                            parts.push(p.trail.clone());
                        }
                        if p.folders > 0 {
                            parts.push(format!("{} 个文件夹", p.folders));
                        }
                        parts.push(format!("{} 首", p.tracks));
                        parts.push(format_bytes(p.bytes));
                        parts.join(" · ")
                    })
                };
                let title = move || here.with(|p| p.title.clone());
                let actions = view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(rescan_button())
                    .child(when(
                        move || here.with(|p| !p.all.is_empty()),
                        move || {
                            pill_button(Icon::Play, "播放全部", move || {
                                here.with_untracked(|p| play_indices(&p.all, 0))
                            })
                        },
                    ));
                let header = view()
                    .child(page_header(
                        title,
                        subtitle,
                        None,
                        Some(actions.into_element()),
                    ))
                    .child(track_columns(FOLDER_COLS));
                let file_list =
                    virtual_list(computed(move || files.with(|f| f.len())), move |at| {
                        let index = computed(move || files.with(|f| f.get(at.index()).copied()));
                        let track = computed(move || {
                            index.with(|i| i.and_then(|i| s.tracks.with(|t| t.get(i).cloned())))
                        });
                        let number = computed(move || (at.index() + 1).to_string());
                        let play = Rc::new(move || {
                            let files = files.get_untracked();
                            play_indices(&files, at.index());
                        });
                        let row = track_row(track, number, FOLDER_COLS, {
                            let play = play.clone();
                            move || play()
                        })
                        .pan_values(Some(&ctx.gx), Some(&ctx.gy))
                        .claim_touches(ctx.claim())
                        .keep_touches(true)
                        .on_responder_grant({
                            let ctx = ctx.clone();
                            move |_: &ResponderEvent| {
                                if let Some(t) = track.get_untracked() {
                                    ctx.begin(t.path.clone(), false, t.title.clone());
                                }
                            }
                        })
                        .on_responder_move({
                            let ctx = ctx.clone();
                            move |e: &ResponderEvent| ctx.move_to(e.page)
                        })
                        .on_responder_release({
                            let ctx = ctx.clone();
                            move |e: &ResponderEvent| ctx.finish(e.page)
                        })
                        .on_responder_terminate({
                            let ctx = ctx.clone();
                            move |_: &ResponderEvent| ctx.cancel()
                        });
                        super::playlists::track_menu(row, track, None, move || play(), {
                            let ops = ops.clone();
                            Some(Rc::new(move |p: &str| {
                                ops.op.set(Some(FileOp::Rename {
                                    path: p.to_owned(),
                                    dir: false,
                                }))
                            }) as Rc<dyn Fn(&str)>)
                        })
                    })
                    .estimated_item_size(px(ROW))
                    .content_padding_bottom(px(BAR_SPACE))
                    .content_padding_x(px(20.0))
                    .flex_1()
                    .empty(empty_state(
                        Icon::Folder,
                        "这个文件夹没有曲目",
                        "它包含的音乐可能在子文件夹里，在左侧树里选一个子文件夹。",
                    ));
                let right = view()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(header)
                    .child(file_list);
                view()
                    .flex_1()
                    .flex_row()
                    .child(tree_pane)
                    .child(right)
                    .into_element()
            }),
        )
        // 拖动浮层：跟着指针的小条，不拦截事件。
        .child(when(
            move || active.with(|a| a.is_some()),
            move || {
                let name =
                    move || active.with(|a| a.as_ref().map(|d| d.name.clone()).unwrap_or_default());
                let dir = move || active.with(|a| a.as_ref().is_some_and(|d| d.dir));
                view()
                    .absolute()
                    .top(move || px(anchor.get().1))
                    .left(move || px(anchor.get().0))
                    .bind_translate_x(&gx)
                    .bind_translate_y(&gy)
                    .pointer_events(PointerEvents::None)
                    .h(px(28.0))
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .pl(px(8.0))
                    .pr(px(12.0))
                    .rounded(px(8.0))
                    .bg(bar())
                    .shadow_lg()
                    .child(icon_dyn(
                        move || {
                            if dir() { Icon::Folder } else { Icon::Note }
                        },
                        13.0,
                        primary,
                    ))
                    .child(
                        text(name)
                            .number_of_lines(1)
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(fg()),
                    )
            },
        ))
        // 增删改弹窗（新建 / 重命名输入名，删除先确认）。
        .child(when(move || op.with(|o| o.is_some()), {
            let ops = ops_dialog;
            move || op_dialog(ops.clone())
        }))
}

fn rescan_button() -> nsh_base::ButtonElement {
    let s = store();
    icon_button(|| Icon::Refresh, 16.0, "重新扫描", muted_fg, scan)
        .disabled(move || s.library.with(|l| l.scanning))
}

/// 树里的一行：缩进 + 折叠箭头（叶子为占位）+ 文件夹图标 + 名字 + 曲目
/// 数。行是整宽的可点区（选中），箭头单独可点（折叠 / 展开）；行也能拖
/// 起这个文件夹，或作为别的拖动落下的目标（`over` 命中时高亮）；右键
/// 是增删改菜单。
fn tree_cell(
    row: Memo<Option<TreeRow>>,
    here: Memo<Place>,
    ctx: Rc<DragCtx>,
    ops: Rc<Ops>,
    select: Rc<dyn Fn(&str)>,
    toggle: Rc<dyn Fn(&str, bool)>,
) -> AnyElement {
    let name = move || row.with(|r| r.as_ref().map(|r| r.name.clone()).unwrap_or_default());
    let depth = move || row.with(|r| r.as_ref().map(|r| r.depth).unwrap_or(0));
    let has_children = move || row.with(|r| r.as_ref().is_some_and(|r| r.has_children));
    let open = move || row.with(|r| r.as_ref().is_some_and(|r| r.open));
    let count = move || row.with(|r| r.as_ref().map(|r| r.count).unwrap_or(0));
    let selected = move || {
        row.with(|r| {
            r.as_ref()
                .is_some_and(|r| here.with(|p| p.path.as_deref() == Some(r.path.as_str())))
        })
    };
    let over = ctx.over;
    let drop_to = move || {
        row.with(|r| {
            r.as_ref()
                .is_some_and(|r| over.with(|o| o.as_deref() == Some(r.path.as_str())))
        })
    };
    let cell = view()
        .h(px(TREE_ROW))
        .pl(move || px(6.0 + depth() as f32 * INDENT))
        .pr(px(8.0))
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .rounded(px(8.0))
        .role(AccessibilityRole::Button)
        .label(move || SharedString::from(name()))
        .accessibility_state(move || AccessibilityState {
            selected: selected(),
            ..AccessibilityState::default()
        })
        .bg(move || {
            if drop_to() {
                primary().opacity(0.24)
            } else if selected() {
                primary().opacity(0.12)
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .hover(|st| st.bg(move || primary().opacity(0.08)))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| {
            if let Some(r) = row.get_untracked() {
                select(&r.path);
            }
        })
        .child(
            when(has_children, move || {
                let toggle = toggle.clone();
                view()
                    .size(px(16.0))
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .role(AccessibilityRole::Button)
                    .label(move || format!("{}{}", if open() { "收起 " } else { "展开 " }, name()))
                    .hover(|st| st.bg(hover()))
                    .on_press(move |_: &()| {
                        if let Some(r) = row.get_untracked() {
                            toggle(&r.path, r.open);
                        }
                    })
                    .child(icon_dyn(
                        move || {
                            if open() {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            }
                        },
                        12.0,
                        faint,
                    ))
            })
            .otherwise(|| view().w(px(16.0)).flex_shrink_0()),
        )
        .child(icon(Icon::Folder, 15.0, move || {
            if selected() { primary() } else { muted_fg() }
        }))
        .child(
            view().flex_1().min_w(px(0.0)).child(
                text(name)
                    .number_of_lines(1)
                    .text_size(px(13.0))
                    .line_height(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || if selected() { fg() } else { muted_fg() }),
            ),
        )
        .child(when(
            move || count() > 0,
            move || {
                text(move || count().to_string())
                    .text_size(px(11.0))
                    .line_height(px(11.0))
                    .tabular_nums()
                    .text_color(faint())
            },
        ));
    tree_menu(
        cell.pan_values(Some(&ctx.gx), Some(&ctx.gy))
            .claim_touches(ctx.claim())
            .keep_touches(true)
            .on_responder_grant({
                let ctx = ctx.clone();
                move |_: &ResponderEvent| {
                    if let Some(r) = row.get_untracked() {
                        ctx.begin(r.path.clone(), true, r.name.clone());
                    }
                }
            })
            .on_responder_move({
                let ctx = ctx.clone();
                move |e: &ResponderEvent| ctx.move_to(e.page)
            })
            .on_responder_release({
                let ctx = ctx.clone();
                move |e: &ResponderEvent| ctx.finish(e.page)
            })
            .on_responder_terminate({
                let ctx = ctx.clone();
                move |_: &ResponderEvent| ctx.cancel()
            }),
        row,
        ops,
    )
}

#[cfg(test)]
mod tests {
    use super::super::super::components::tracks::audio_format;
    use super::*;

    fn entry(path: &str) -> TrackEntry {
        TrackEntry {
            provider: crate::config::ProviderId::Local,
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
        let tracks = vec![
            entry("/m/a/1.mp3"),
            entry("/m/a/b/2.mp3"),
            entry("/m/3.mp3"),
            entry("/x/4.mp3"),
        ];
        let tree = build_tree_with(&["/m".into()], &tracks, &[], '/');
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
    fn rows_follow_expand_state() {
        let tracks = vec![
            entry("/m/a/1.mp3"),
            entry("/m/a/b/2.mp3"),
            entry("/m/3.mp3"),
        ];
        let tree = build_tree_with(&["/m".into()], &tracks, &[], '/');
        fn paths(rows: &[TreeRow]) -> Vec<&str> {
            rows.iter().map(|r| r.path.as_str()).collect()
        }
        // 全部折叠：只有根。
        assert_eq!(
            paths(&flatten_with(&tree, &BTreeSet::new(), None, '/')),
            vec!["/m"]
        );
        // 根展开：一层子文件夹。
        let open = BTreeSet::from(["/m".to_owned()]);
        assert_eq!(
            paths(&flatten_with(&tree, &open, None, '/')),
            vec!["/m", "/m/a"]
        );
        // 选中 /m/a/b：祖先自动展开（自己没进展开集不算开，但它是叶子）。
        let rows = flatten_with(&tree, &BTreeSet::new(), Some("/m/a/b"), '/');
        assert_eq!(paths(&rows), vec!["/m", "/m/a", "/m/a/b"]);
        assert!(rows[..2].iter().all(|r| r.open));
        assert!(!rows[2].open);
    }

    #[test]
    fn place_shows_folder_files_and_subtree() {
        let mut tracks = vec![
            entry("/m/a/1.mp3"),
            entry("/m/a/b/2.mp3"),
            entry("/m/3.mp3"),
        ];
        for (t, size) in tracks.iter_mut().zip([100, 200, 300]) {
            t.size_bytes = size;
            t.duration = 60.0;
        }
        let tree = build_tree_with(&["/m".into()], &tracks, &[], '/');
        // 没选或选中不存在时都回退到第一个根。
        let m = place_with(&tree, None, '/');
        assert_eq!(m.path.as_deref(), Some("/m"));
        assert_eq!(
            place_with(&tree, Some("/gone"), '/').path.as_deref(),
            Some("/m")
        );
        // 根：files 只是直接曲目；all 是整棵子树；trail 是完整根路径。
        assert_eq!(*m.files, vec![2]);
        assert_eq!(*m.all, vec![1, 0, 2]);
        assert_eq!(m.folders, 1);
        assert_eq!(m.tracks, 3);
        assert_eq!(m.bytes, 600);
        assert_eq!(m.trail, "/m");
        let a = place_with(&tree, Some("/m/a"), '/');
        assert_eq!(*a.files, vec![0]);
        assert_eq!(*a.all, vec![1, 0]);
        assert_eq!(a.trail, "m / a");
        // 多个根：只列根，没选时显示名字靠前的那个。
        let both = vec![entry("/m/1.mp3"), entry("/x/2.mp3")];
        let two = build_tree_with(&["/m".into(), "/x".into()], &both, &[], '/');
        assert_eq!(two.len(), 2);
        assert_eq!(place_with(&two, None, '/').path.as_deref(), Some("/m"));
        assert_eq!(format_bytes(1536 * 1024), "1.5 MB");
        assert_eq!(audio_format("/m/a.flac"), "FLAC");
        assert_eq!(audio_format("C:\\m\\b.Mp3"), "MP3");
    }

    #[test]
    fn drop_target_rejects_noops_and_cycles() {
        // 文件 / 文件夹落到别的文件夹：可以。
        assert!(drop_ok("/m/a/1.mp3", false, "/m/b", '/'));
        assert!(drop_ok("/m/a", true, "/x", '/'));
        // 落到自身或当前所在：不行（等于没动）。
        assert!(!drop_ok("/m/a", true, "/m/a", '/'));
        assert!(!drop_ok("/m/a", true, "/m", '/'));
        assert!(!drop_ok("/m/a/1.mp3", false, "/m/a", '/'));
        // 文件夹不能进自己的子树。
        assert!(!drop_ok("/m/a", true, "/m/a/b", '/'));
        // 子树判断按路径分段比：「/m/abc」不是「/m/ab」的后代。
        assert!(drop_ok("/m/ab", true, "/m/abc", '/'));
        // 尾随分隔符不影响判断。
        assert!(!drop_ok("/m/a/", true, "/m", '/'));
        // Windows 路径同规则。
        assert!(drop_ok("C:\\m\\a", true, "C:\\x", '\\'));
        assert!(!drop_ok("C:\\m\\a", true, "C:\\m\\a\\b", '\\'));
        assert!(!drop_ok("C:\\m\\a", true, "C:\\m", '\\'));
    }

    #[test]
    fn move_into_renames_and_reports_collisions() {
        let base = std::env::temp_dir().join(format!("zeo-move-test-{}", std::process::id()));
        let src = base.join("src");
        let dst = base.join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        let file = src.join("a.mp3");
        std::fs::write(&file, b"x").unwrap();
        let to = move_into(file.to_str().unwrap(), dst.to_str().unwrap(), '/').unwrap();
        assert!(std::path::Path::new(&to).exists());
        assert!(!file.exists());
        // 目标已有同名：报错且原文件还在。
        let file2 = src.join("a.mp3");
        std::fs::write(&file2, b"y").unwrap();
        assert!(move_into(file2.to_str().unwrap(), dst.to_str().unwrap(), '/').is_err());
        assert!(file2.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn windows_paths_build_the_same_tree() {
        let tracks = vec![
            entry("C:\\m\\a\\1.mp3"),
            entry("C:\\m\\2.mp3"),
            entry("D:\\3.mp3"),
        ];
        let tree = build_tree_with(&["C:\\m".into(), "D:\\".into()], &tracks, &[], '\\');
        // 根按名字排序：「D:」在「m」前 → 默认显示它。
        assert_eq!(
            tree.iter()
                .map(|r| (r.name.as_str(), r.total))
                .collect::<Vec<_>>(),
            vec![("D:", 1), ("m", 2)]
        );
        let top = place_with(&tree, None, '\\');
        assert_eq!(top.path.as_deref(), Some("D:\\"));
        let m = place_with(&tree, Some("C:\\m"), '\\');
        assert_eq!(*m.files, vec![1]);
        assert_eq!(m.trail, "C:\\m");
        let a = place_with(&tree, Some("C:\\m\\a"), '\\');
        assert_eq!(a.trail, "m / a");
        assert_eq!(*a.all, vec![0]);
    }

    #[test]
    fn names_are_validated() {
        // 首尾空白裁掉。
        assert_eq!(valid_name("  新建文件夹  ").unwrap(), "新建文件夹");
        // 空名、分隔符、相对路径都不行。
        for bad in ["", "   ", "a/b", "a\\b", ".", ".."] {
            assert!(valid_name(bad).is_err(), "{bad}");
        }
        assert!(valid_name("a:b").is_ok());
    }

    #[test]
    fn rename_dest_keeps_extension() {
        // 文件夹整名换掉。
        assert_eq!(rename_dest("/m/a/b", true, "c", '/').unwrap(), "/m/a/c");
        // 文件只换主干；写了扩展名也不会重复拼；大小写不同也认。
        assert_eq!(
            rename_dest("/m/a/s.mp3", false, "t", '/').unwrap(),
            "/m/a/t.mp3"
        );
        assert_eq!(
            rename_dest("/m/a/s.mp3", false, "t.mp3", '/').unwrap(),
            "/m/a/t.mp3"
        );
        assert_eq!(
            rename_dest("/m/a/s.MP3", false, "t.mp3", '/').unwrap(),
            "/m/a/t.mp3"
        );
        // 无扩展名与点文件：名字原样用。
        assert_eq!(
            rename_dest("/m/a/s", false, "t.flac", '/').unwrap(),
            "/m/a/t.flac"
        );
        assert_eq!(
            rename_dest("/m/a/.env", false, ".env2", '/').unwrap(),
            "/m/a/.env2"
        );
        // Windows 分隔符。
        assert_eq!(
            rename_dest("C:\\m\\a\\s.mp3", false, "t", '\\').unwrap(),
            "C:\\m\\a\\t.mp3"
        );
        // 不合法的名字原样把错误传出来。
        assert!(rename_dest("/m/a", true, "x/y", '/').is_err());
        // 父目录：`/m` 的父是 `/`，盘符下是 `C:\`。
        assert_eq!(parent_dir("/m", '/').as_deref(), Some("/"));
        assert_eq!(parent_dir("/m/a", '/').as_deref(), Some("/m"));
        assert_eq!(parent_dir("C:\\m\\a", '\\').as_deref(), Some("C:\\m"));
        assert_eq!(parent_dir("C:\\a", '\\').as_deref(), Some("C:\\"));
    }

    #[test]
    fn rename_default_is_stem_for_files() {
        assert_eq!(rename_default("/m/a/b", true), "b");
        assert_eq!(rename_default("/m/a/s.mp3", false), "s");
        assert_eq!(rename_default("/m/a/s", false), "s");
        assert_eq!(rename_default("C:\\m\\s.flac", false), "s");
    }

    #[test]
    fn extra_empty_dirs_join_the_tree() {
        let tracks = vec![entry("/m/a/1.mp3")];
        let extra = vec![
            "/m/b".to_string(),
            "/m/a/deep".to_string(),
            "/outside".to_string(),
        ];
        let tree = build_tree_with(&["/m".into()], &tracks, &extra, '/');
        let kids: Vec<&str> = tree[0].children.values().map(|c| c.name.as_str()).collect();
        // 曲目推不出的 b 挂上了；a 里的 deep 也逐级保证；根外的 /outside 不进来。
        assert_eq!(kids, vec!["a", "b"]);
        let a = &tree[0].children["a\u{0}a"];
        assert_eq!(
            a.children
                .values()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["deep"]
        );
        assert_eq!(tree.len(), 1);
    }
}
