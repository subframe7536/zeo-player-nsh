//! 「文件夹」页：曲库根目录的目录树；文件夹行可下钻，整个目录或其中的
//! 单曲都可播放。列配置为 [`FOLDER_COLS`]。

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use nsh_core::prelude::*;

use super::super::components::button::{icon_button, pill_button};
use super::super::components::icons::{Icon, icon};
use super::super::components::page::page_header;
use super::super::components::tracks::{
    Columns, ROW, W_ALBUM, W_COVER, W_FORMAT, W_NUMBER, W_SIZE, W_TIME, fits_album,
    fits_number, fits_size, format_bytes, track_columns, track_row, value_cell,
};
use super::super::store::{Same, play_list, scan, store};
use super::super::theme::*;
use super::{SEP, by_disc_track, compare, last_segment, no_library};
use crate::config::TrackEntry;

/// 文件夹：面向文件，带大小列。
const FOLDER_COLS: Columns = Columns {
    cover: true,
    album: true,
    size: true,
    format: true,
};

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
    Track {
        index: usize,
        number: usize,
    },
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
        node.path == r.path
            || (node.path.starts_with(root) && node.path[root.len()..].starts_with(sep))
    };
    let root = tree.iter().find(|r| under(r));
    let is_root = root.is_some_and(|r| r.path == node.path);
    let parent = if is_root {
        Some(None)
    } else {
        node.path.rsplit_once(sep).map(|(p, _)| Some(p.to_owned()))
    };
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
    let mut rows: Vec<FolderRow> = node
        .children
        .values()
        .map(|c| folder_row(c, false))
        .collect();
    rows.extend(
        node.tracks
            .iter()
            .enumerate()
            .map(|(n, &index)| FolderRow::Track {
                index,
                number: n + 1,
            }),
    );
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
    let queue: Vec<TrackEntry> = indices
        .iter()
        .filter_map(|&i| all.get(i).cloned())
        .collect();
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
        let back: Option<Rc<dyn Fn()>> = p
            .parent
            .clone()
            .map(|parent| Rc::new(move || cwd.set(parent.clone())) as Rc<dyn Fn()>);
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
        let title = move || here.with(|p| p.title.clone());
        let header = view()
            .child(page_header(
                title,
                subtitle,
                back,
                Some(actions.into_element()),
            ))
            .child(track_columns(FOLDER_COLS));

        let rows = computed(move || here.with(|p| p.rows.clone()));
        let count = computed(move || rows.with(|r| r.len()));
        let list = virtual_list(count, move |at| {
            let row = computed(move || rows.with(|r| r.get(at.index()).cloned()));
            let is_folder =
                computed(move || row.with(|r| matches!(r, Some(FolderRow::Folder { .. }))));
            when(is_folder, move || folder_cell(row, cwd))
                .otherwise(move || {
                    let track = computed(move || match row.get() {
                        Some(FolderRow::Track { index, .. }) => {
                            s.tracks.with(|t| t.get(index).cloned())
                        }
                        _ => None,
                    });
                    let number = computed(move || match row.get() {
                        Some(FolderRow::Track { number, .. }) => number.to_string(),
                        _ => String::new(),
                    });
                    let play = Rc::new(move || {
                        let Some(FolderRow::Track { number, .. }) = row.get_untracked() else {
                            return;
                        };
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

    view().flex_1().child(
        when(move || s.library.with(|l| l.roots.is_empty()), no_library).otherwise(move || {
            // 位置变了才重建页头与列表；同一位置内的数据变化由绑定更新。
            let at = computed(move || here.with(|p| p.path.clone()));
            dynamic(move || {
                at.get();
                untracked(body)
            })
        }),
    )
}

fn rescan_button() -> nsh_base::ButtonElement {
    let s = store();
    icon_button(|| Icon::Refresh, 16.0, "重新扫描", muted_fg, scan)
        .disabled(move || s.library.with(|l| l.scanning))
}

fn folder_cell(row: Memo<Option<FolderRow>>, cwd: Signal<Option<String>>) -> nsh_base::ButtonElement {
    let pick = move |f: fn(&FolderRow) -> String| {
        move || row.with(|r| r.as_ref().map(f).unwrap_or_default())
    };
    let is_root = move || row.with(|r| matches!(r, Some(FolderRow::Folder { root: true, .. })));
    let tile = view()
        .size(px(W_COVER))
        .rounded(px(8.0))
        .items_center()
        .justify_center()
        .bg(move || {
            if is_root() {
                primary().opacity(0.12)
            } else {
                muted()
            }
        })
        .child(icon(Icon::Folder, 20.0, move || {
            if is_root() { primary() } else { muted_fg() }
        }));
    let label = pick(|r| match r {
        FolderRow::Folder { name, .. } => name.clone(),
        FolderRow::Track { .. } => String::new(),
    });
    nsh_base::button()
        .h(px(ROW))
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .px(px(12.0))
        .rounded(px(10.0))
        .accessibility_label(move || SharedString::from(label()))
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| {
            if let Some(FolderRow::Folder { path, .. }) = row.get_untracked() {
                cwd.set(Some(path));
            }
        })
        .child(when(fits_number, || {
            view()
                .w(px(W_NUMBER))
                .items_center()
                .child(icon(Icon::ChevronRight, 14.0, faint))
        }))
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
                    .line_height(px(14.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg()),
                )
                .child(
                    text(pick(|r| match r {
                        FolderRow::Folder {
                            root: true, path, ..
                        } => path.clone(),
                        FolderRow::Folder { folders, count, .. } if *folders > 0 => {
                            format!("{folders} 个文件夹 · {count} 首")
                        }
                        FolderRow::Folder { count, .. } => format!("{count} 首"),
                        FolderRow::Track { .. } => String::new(),
                    }))
                    .number_of_lines(1)
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(muted_fg()),
                ),
        )
        .child(when(fits_album, move || {
            view().w(px(W_ALBUM)).child(
                text(pick(|r| match r {
                    FolderRow::Folder {
                        root: true,
                        folders,
                        count,
                        ..
                    } if *folders > 0 => {
                        format!("{folders} 个文件夹 · {count} 首")
                    }
                    FolderRow::Folder {
                        root: true, count, ..
                    } => format!("{count} 首"),
                    _ => String::new(),
                }))
                .number_of_lines(1)
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
            )
        }))
        // 文件夹行的格式列留空，保持与曲目行对齐。
        .child(when(fits_size, || view().w(px(W_FORMAT))))
        .child(when(fits_size, move || {
            value_cell(
                W_SIZE,
                pick(|r| match r {
                    FolderRow::Folder { bytes, .. } => format_bytes(*bytes),
                    FolderRow::Track { .. } => String::new(),
                }),
            )
        }))
        .child(value_cell(
            W_TIME,
            pick(|r| match r {
                FolderRow::Folder { seconds, .. } => format_total_short(*seconds),
                FolderRow::Track { .. } => String::new(),
            }),
        ))
}

/// 文件夹总时长（列内的短格式）：3:25:10 / 42:05。
fn format_total_short(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
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
        let tracks = vec![
            entry("C:\\m\\a\\1.mp3"),
            entry("C:\\m\\2.mp3"),
            entry("D:\\3.mp3"),
        ];
        let tree = build_tree_with(&["C:\\m".into(), "D:\\".into()], &tracks, '\\');
        // 根按名字排序：「D:」在「m」前。
        assert_eq!(
            tree.iter()
                .map(|r| (r.name.as_str(), r.total))
                .collect::<Vec<_>>(),
            vec![("D:", 1), ("m", 2)]
        );
        let m = place_with(&tree, Some("C:\\m"), '\\');
        assert!(
            matches!(&m.rows[0], FolderRow::Folder { name, path, .. } if name == "a" && path == "C:\\m\\a")
        );
        let a = place_with(&tree, Some("C:\\m\\a"), '\\');
        assert_eq!(a.parent, Some(Some("C:\\m".into())));
        assert_eq!(a.trail, "m / a");
    }

    #[test]
    fn roots_are_listed_and_navigable() {
        let mut tracks = vec![
            entry("/m/a/1.mp3"),
            entry("/m/a/b/2.mp3"),
            entry("/m/3.mp3"),
        ];
        for (t, size) in tracks.iter_mut().zip([100, 200, 300]) {
            t.size_bytes = size;
            t.duration = 60.0;
        }
        let tree = build_tree_with(&["/m".into()], &tracks, '/');
        // 顶层：一个根也列出来。
        let top = place_with(&tree, None, '/');
        assert_eq!(top.path, None);
        assert_eq!(top.parent, None);
        assert!(matches!(
            &top.rows[..],
            [FolderRow::Folder {
                root: true,
                count: 3,
                bytes: 600,
                ..
            }]
        ));
        // 根：子文件夹在前，再是本层曲目；返回顶层。
        let m = place_with(&tree, Some("/m"), '/');
        assert_eq!(m.parent, Some(None));
        assert_eq!(m.trail, "/m");
        assert!(
            matches!(&m.rows[0], FolderRow::Folder { name, root: false, count: 2, bytes: 300, .. } if name == "a")
        );
        assert!(matches!(
            m.rows[1],
            FolderRow::Track {
                index: 2,
                number: 1
            }
        ));
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
