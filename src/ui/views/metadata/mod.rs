//! 「元数据」视图：全库曲目的标签表格（nsh-base `DataTable` + 复选框
//! 多选），右侧单选 / 批量编辑面板与纯文件模板重命名弹窗。
//!
//! 行的标签值是视图自有状态（`rows` 信号）：后台线程分批用
//! `write::read_row` 读出，不进 `MetaCache` / SQLite（全局元数据缓存
//! 保持精简，见 AGENTS 约定）；未加载的行回落 `TrackEntry` 索引值。

mod panel;
mod rename;
mod table;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use nsh_base::DataTableSort;
use nsh_core::prelude::*;

use super::super::components::button::{Size, Variant, text_button};
use super::super::components::checkbox::checkbox;
use super::super::components::form::input_field;
use super::super::components::page::page_header;
use super::super::store::{Post, poster, store};
use super::super::theme::*;
use super::no_library;
use crate::config::TrackEntry;
use crate::metadata::write::TagField;

/// 一行的标签值（视图自有）。歌词只留预览（第一条非空行、剥 LRC
/// 标记、截 40 字），注释截 120 字——完整值由单选面板按需现读；
/// `stamp` 是读取时的 `TrackEntry.stamp`，文件变了自动重读。
#[derive(PartialEq)]
pub(super) struct MetaRow {
    pub fields: HashMap<TagField, String>,
    /// 有歌词：内嵌非空或同名 `.lrc` / `.LRC` 存在。
    pub has_lyrics: bool,
    pub cover: CoverState,
    /// 读取来源的标签类型显示名（文件无标签为 `None`）。
    pub tag_type: Option<&'static str>,
    pub stamp: u64,
}

/// 表格「封面状态」列的三种来源。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum CoverState {
    Embedded,
    File,
    None,
}

/// 后台线程读一行（读不出返回 `None`，单元格回落 `TrackEntry` 值）。
fn build_row(path: &Path, stamp: u64, has_art: bool) -> Option<MetaRow> {
    let (mut fields, tag_type) = crate::metadata::write::read_row(path).ok()?;
    let embedded = fields
        .get(&TagField::Lyrics)
        .is_some_and(|s| !s.trim().is_empty());
    // 与 engine::Meta::external_lrc 同一组候选（纯文件检查，线程安全）。
    let has_lyrics = embedded
        || crate::engine::Meta::external_lrc(path.to_string_lossy().into_owned()).is_some();
    let preview = fields
        .get(&TagField::Lyrics)
        .map(|s| lyrics_preview(s))
        .unwrap_or_default();
    fields.insert(TagField::Lyrics, preview);
    if let Some(comment) = fields.get_mut(&TagField::Comment) {
        *comment = comment.chars().take(120).collect();
    }
    let cover = if crate::metadata::covers::find_sidecar(path).is_some() {
        CoverState::File
    } else if has_art {
        CoverState::Embedded
    } else {
        CoverState::None
    };
    Some(MetaRow {
        fields,
        has_lyrics,
        cover,
        tag_type,
        stamp,
    })
}

/// 歌词预览：第一条非空行，剥掉行首 `[..]`（LRC 时间戳与标记段），截 40 字。
fn lyrics_preview(raw: &str) -> String {
    for line in raw.lines() {
        let mut s = line.trim();
        while let Some((_, rest)) = s.strip_prefix('[').and_then(|t| t.split_once(']')) {
            s = rest.trim_start();
        }
        if !s.is_empty() {
            return s.chars().take(40).collect();
        }
    }
    String::new()
}

/// 「元数据」页。
pub fn page() -> View {
    let s = store();
    let rows = signal(HashMap::<String, Arc<MetaRow>>::new());
    let selected = signal(BTreeSet::<String>::new());
    let anchor = signal(Option::<String>::None);
    let filter = signal(String::new());
    let sort = signal(Option::<DataTableSort>::None);
    let rename_open = signal(false);

    // 行加载器：补读缺失 / 指纹过期的行，清掉离开曲库的（选中集同步
    // 修剪）；约 100 条一批回排写信号。代次计数作废上一趟，视图拆掉
    // 后投递不再写。
    let generation = Arc::new(AtomicU64::new(0));
    let alive = Arc::new(AtomicBool::new(true));
    {
        let alive_cleanup = alive.clone();
        cleanup(move || alive_cleanup.store(false, Ordering::Relaxed));
        effect(move || {
            let tracks = s.tracks.get();
            let live: HashSet<&str> = tracks.iter().map(|t| t.path.as_str()).collect();
            rows.update(|r| r.retain(|p, _| live.contains(p.as_str())));
            selected.update(|set| set.retain(|p| live.contains(p.as_str())));
            let need: Vec<(String, u64, bool)> = tracks
                .iter()
                .filter(|t| {
                    rows.with_untracked(|r| r.get(&t.path).map_or(true, |row| row.stamp != t.stamp))
                })
                .map(|t| (t.path.clone(), t.stamp, t.has_art))
                .collect();
            if need.is_empty() {
                return;
            }
            let run_gen = generation.fetch_add(1, Ordering::Relaxed) + 1;
            let generation = generation.clone();
            let alive = alive.clone();
            let post = poster();
            std::thread::spawn(move || {
                // 投递合并：每批到来都重建 `displayed` + 全表 DataTableRows，
                // 大曲库逐 100 条投递就是 ~100 次全表重算——每 ~250ms 至多
                // 投一次，末尾兜底收尾。
                let mut last = std::time::Instant::now() - std::time::Duration::from_millis(250);
                let mut batch: Vec<(String, Arc<MetaRow>)> = Vec::new();
                let flush = |post: &Post, batch: &mut Vec<(String, Arc<MetaRow>)>| {
                    if batch.is_empty() {
                        return;
                    }
                    let chunk = std::mem::take(batch);
                    let alive = alive.clone();
                    post.post(move || {
                        // 视图已拆：信号可能已随 owner 处置，别写。
                        if !alive.load(Ordering::Relaxed) {
                            return;
                        }
                        rows.update(|r| {
                            for (p, row) in chunk {
                                r.insert(p, row);
                            }
                        })
                    });
                };
                for (path, stamp, has_art) in need {
                    if generation.load(Ordering::Relaxed) != run_gen
                        || !alive.load(Ordering::Relaxed)
                    {
                        return;
                    }
                    if let Some(row) = build_row(Path::new(&path), stamp, has_art) {
                        batch.push((path, Arc::new(row)));
                    }
                    if last.elapsed() >= std::time::Duration::from_millis(250) {
                        flush(&post, &mut batch);
                        last = std::time::Instant::now();
                    }
                }
                flush(&post, &mut batch);
            });
        });
    }

    // 当前显示序：筛选（文件名 / 标题 / 歌手 / 专辑，不分大小写）+ app
    // 侧排序（无排序 = 曲库序）。
    let displayed = computed(move || {
        let tracks = s.tracks.get();
        let map = rows.get(); // 排序读已加载字段，加载批到了要重排
        let q = filter.with(|f| f.trim().to_lowercase());
        let mut list: Vec<TrackEntry> = tracks
            .iter()
            .filter(|t| {
                q.is_empty()
                    || [&t.file_name, &t.title, &t.artist, &t.album, &t.album_artist]
                        .iter()
                        .any(|f| f.to_lowercase().contains(&q))
            })
            .cloned()
            .collect();
        if let Some(srt) = sort.get() {
            table::sort_entries(
                &mut list,
                &srt.key,
                srt.direction == nsh_base::DataTableSortDirection::Ascending,
                &map,
            );
        }
        Rc::new(list)
    });

    let count = computed(move || s.tracks.with(|t| t.len()));
    // 空曲库切换单独成 memo：若直接读 `s.tracks`，每次索引刷新 /
    // `move_path` 都整页重建（表格滚动位、面板草稿、筛选框全丢）。
    let empty = computed(move || s.tracks.with(|t| t.is_empty()));
    view()
        .flex_1()
        .min_w(px(0.0))
        .child(dynamic(move || {
            if empty.get() {
                return no_library().into_element();
            }
            view()
                .flex_1()
                .min_w(px(0.0))
                .child(page_header(
                    || "元数据".into(),
                    move || {
                        format!(
                            "{} 首曲目 · 已选 {} 首",
                            count.get(),
                            selected.with(|set| set.len())
                        )
                    },
                    None,
                    None,
                ))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(32.0))
                        .pb(px(12.0))
                        .window_drag(true)
                        .child(
                            input_field(true)
                                .w(px(280.0))
                                .label("筛选")
                                .placeholder("筛选文件名、标题、歌手、专辑")
                                .default_value(untracked(|| filter.get()))
                                .window_drag(false)
                                .on_change(move |t: &str| filter.set(t.to_owned())),
                        )
                        .child({
                            let displayed = displayed.clone();
                            let selected = selected.clone();
                            let anchor = anchor.clone();
                            // 「全选」不放表头格：上游自定义表头对滚动区列
                            // 不落渲染内容（待上游），工具栏的 nsh_base
                            // 复选框在真机 / headless 都稳。
                            let mark = checkbox(
                                {
                                    let displayed = displayed.clone();
                                    let selected = selected.clone();
                                    move || {
                                        displayed.with(|v| {
                                            !v.is_empty()
                                                && v.iter()
                                                    .all(|t| selected.with(|s| s.contains(&t.path)))
                                        })
                                    }
                                },
                                {
                                    let displayed = displayed.clone();
                                    let selected = selected.clone();
                                    move || {
                                        displayed.with(|v| {
                                            v.iter().any(|t| selected.with(|s| s.contains(&t.path)))
                                                && !v
                                                    .iter()
                                                    .all(|t| selected.with(|s| s.contains(&t.path)))
                                        })
                                    }
                                },
                                "全选",
                                move |next| {
                                    let paths: Vec<String> = displayed
                                        .with(|v| v.iter().map(|t| t.path.clone()).collect());
                                    selected.update(|set| {
                                        if next {
                                            for p in &paths {
                                                set.insert(p.clone());
                                            }
                                        } else {
                                            for p in &paths {
                                                set.remove(p);
                                            }
                                        }
                                    });
                                    if next {
                                        anchor.set(paths.first().cloned());
                                    }
                                },
                            );
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .child(mark)
                                .child(
                                    text("全选")
                                        .text_size(px(13.0))
                                        .line_height(px(13.0))
                                        .text_color(muted_fg()),
                                )
                                .into_element()
                        })
                        .child(when(
                            move || selected.with(|set| !set.is_empty()),
                            move || {
                                view()
                                    .flex_row()
                                    .gap(px(8.0))
                                    .child(text_button(
                                        Variant::Outline,
                                        Size::Sm,
                                        None,
                                        "重命名文件…",
                                        move || rename_open.set(true),
                                    ))
                                    .child(text_button(
                                        Variant::Ghost,
                                        Size::Sm,
                                        None,
                                        "取消选择",
                                        move || {
                                            selected.set(BTreeSet::new());
                                            anchor.set(None);
                                        },
                                    ))
                            },
                        )),
                )
                .child(
                    view()
                        .flex_1()
                        .min_h(px(0.0))
                        .flex_row()
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .pb(px(BAR_SPACE))
                                .child(table::table(displayed, rows, selected, anchor, sort)),
                        )
                        .child(when(
                            move || selected.with(|set| !set.is_empty()),
                            move || {
                                panel::panel(
                                    displayed,
                                    rows,
                                    selected,
                                    // 面板 ✕：清空选中集与范围锚点。
                                    Rc::new(move || {
                                        selected.set(BTreeSet::new());
                                        anchor.set(None);
                                    }),
                                    move || rename_open.set(true),
                                )
                                .into_element()
                            },
                        )),
                )
                .into_element()
        }))
        .child(dynamic(move || {
            if rename_open.get() {
                rename::dialog(rename_open, displayed, rows, selected).into_element()
            } else {
                view().into_element()
            }
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 歌词预览：剥 LRC 时间戳与标记段，取第一条非空行。
    #[test]
    fn lyrics_preview_strips_lrc_marks() {
        assert_eq!(
            lyrics_preview("[00:01.00]第一行\n[00:02.00]第二行"),
            "第一行"
        );
        assert_eq!(
            lyrics_preview("[ti:标题]\n[ar:歌手]\n[00:01.00]正文"),
            "正文"
        );
        assert_eq!(lyrics_preview("\n  \n纯文本行"), "纯文本行");
        assert_eq!(lyrics_preview(""), "");
        // 长行截 40 字（按字符不按字节）。
        assert_eq!(lyrics_preview(&"长".repeat(50)).chars().count(), 40);
    }
}
