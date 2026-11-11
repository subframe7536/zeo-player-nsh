use super::*;

/// 文件夹：面向文件，带大小列。
pub(crate) const FOLDER_COLS: Columns = Columns {
    number: false,
    cover: true,
    album: true,
    size: true,
    format: true,
};

/// 目录树行高（与侧栏 nav_row 统一为 34pt）与每层缩进。
pub(crate) const TREE_ROW: f32 = 34.0;
pub(crate) const INDENT: f32 = 14.0;

#[derive(Default)]
pub(crate) struct Folder {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) children: BTreeMap<String, Folder>,
    pub(crate) tracks: Vec<usize>,
    /// 子树曲目数、字节数、时长（秒）。
    pub(crate) total: usize,
    pub(crate) bytes: u64,
    pub(crate) seconds: f64,
}

impl Folder {
    pub(crate) fn finish(&mut self, all: &[TrackEntry]) -> usize {
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

    pub(crate) fn find(&self, path: &str) -> Option<&Folder> {
        if self.path == path {
            return Some(self);
        }
        self.children.values().find_map(|c| c.find(path))
    }

    /// 文件夹内（递归）的曲目，按显示顺序：子文件夹在前。
    pub(crate) fn subtree(&self, out: &mut Vec<usize>) {
        for child in self.children.values() {
            child.subtree(out);
        }
        out.extend(&self.tracks);
    }
}

pub(crate) fn build_tree(roots: &[String], tracks: &[TrackEntry], extra: &[String]) -> Vec<Folder> {
    build_tree_with(roots, tracks, extra, SEP)
}

pub(crate) fn build_tree_with(
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

pub(crate) fn compare_key(name: &str) -> String {
    format!("{}\u{0}{name}", name.to_lowercase())
}

/// 右侧显示的位置：`path` 为实际显示的文件夹（选中项失效或还没选时回
/// 退到第一个根文件夹；没有根目录为 `None`，由 `no_library` 接住）。
#[derive(Clone, PartialEq)]
pub(crate) struct Place {
    pub(crate) path: Option<String>,
    pub(crate) title: String,
    pub(crate) trail: String,
    pub(crate) folders: usize,
    pub(crate) tracks: usize,
    pub(crate) bytes: u64,
    /// 子树全部曲目（列表、「播放全部」与点播队列；子文件夹在前）。
    pub(crate) all: Rc<Vec<usize>>,
}

pub(crate) fn place(tree: &[Folder], at: Option<&str>) -> Place {
    place_with(tree, at, SEP)
}

pub(crate) fn place_with(tree: &[Folder], at: Option<&str>, sep: char) -> Place {
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
        all: Rc::new(all),
    }
}

/// 树里可见的一行（折叠状态已算进列表）。
#[derive(Clone, PartialEq)]
pub(crate) struct TreeRow {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) depth: usize,
    pub(crate) has_children: bool,
    pub(crate) open: bool,
    pub(crate) count: usize,
}

/// 展开 `path` 的所有父级祖先目录。
pub(crate) fn expand_ancestors(expanded: &mut BTreeSet<String>, path: &str, sep: char) {
    let mut current = path.trim_end_matches(sep);
    while let Some(idx) = current.rfind(sep) {
        current = &current[..idx];
        if current.is_empty() {
            break;
        }
        expanded.insert(current.to_owned());
    }
}

/// `expanded` 里的文件夹展开。
pub(crate) fn flatten_with(tree: &[Folder], expanded: &BTreeSet<String>) -> Vec<TreeRow> {
    pub(crate) fn walk(
        node: &Folder,
        depth: usize,
        expanded: &BTreeSet<String>,
        out: &mut Vec<TreeRow>,
    ) {
        let open = expanded.contains(&node.path);
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
                walk(child, depth + 1, expanded, out);
            }
        }
    }
    let mut out = Vec::new();
    for root in tree {
        walk(root, 0, expanded, &mut out);
    }
    out
}

pub(crate) fn play_indices(indices: &[usize], start: usize) {
    let all = store().tracks.get_untracked();
    let queue: Vec<TrackEntry> = indices
        .iter()
        .filter_map(|&i| all.get(i).cloned())
        .collect();
    play_list(&queue, start);
}
