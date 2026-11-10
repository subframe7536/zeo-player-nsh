//! 「元数据」视图：全库曲目的标签表格（nsh-base `DataTable` + 复选框
//! 多选），右侧单选 / 批量编辑面板与纯文件模板重命名弹窗。
//!
//! 行的标签值是视图自有状态（`rows` 信号）：后台线程分批用
//! `write::read_row` 读出，不进 `MetaCache` / SQLite（全局元数据缓存
//! 保持精简，见 AGENTS 约定）；未加载的行回落 `TrackEntry` 索引值。

mod columns;
mod rows;
#[cfg(test)]
mod tests;

pub(crate) use columns::*;
pub(crate) use rows::*;

mod panel;
mod rename;
mod table;

use nsh_base::{DataTableSort, popover};
use nsh_core::prelude::*;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use zeo_metadata::write::TagField;
use zeo_types::config::TrackEntry;
use zeo_ui_components::button::{RADIUS_MD, Size, Variant, text_button};
use zeo_ui_components::checkbox::checkbox;
use zeo_ui_components::form::input_field;
use zeo_ui_components::icons::{Icon, icon};
use zeo_ui_components::page::page_header;
use zeo_ui_store::store;
use zeo_ui_store::theme::*;
pub(crate) use zeo_ui_views_base::no_library;

pub fn page() -> View {
    let s = store();
    let rows = signal(HashMap::<String, Arc<MetaRow>>::new());
    let selected = signal(BTreeSet::<String>::new());
    let anchor = signal(Option::<String>::None);
    let filter = signal(String::new());
    let sort = signal(Option::<DataTableSort>::None);
    let rename_open = signal(false);
    let visible_cols = signal(default_columns());
    let col_widths = signal(HashMap::<String, f32>::new());

    // 同步修剪：清掉离开曲库的路径与选中集。曲目元数据直接由 SQLite / 索引条目供给，
    // 无需后台再逐曲扫描磁盘标签。
    effect(move || {
        let tracks = s.tracks.get();
        let live: HashSet<&str> = tracks.iter().map(|t| t.path.as_str()).collect();
        rows.update(|r| r.retain(|p, _| live.contains(p.as_str())));
        selected.update(|set| set.retain(|p| live.contains(p.as_str())));
    });

    // 当前显示序：筛选（文件名 / 标题 / 歌手 / 专辑，不分大小写）+ app
    // 侧排序（无排序 = 曲库序）。未排序时不读 `rows`，避免后台分批写入
    // 频繁导致全表重算与重建。
    let displayed = computed(move || {
        let tracks = s.tracks.get();
        let srt = sort.get();
        let q = filter.with(|f| f.trim().to_lowercase());
        let mut list: Vec<TrackEntry> = tracks
            .iter()
            .filter(|t| {
                q.is_empty()
                    || contains_ignore_case(&t.file_name, &q)
                    || contains_ignore_case(&t.title, &q)
                    || contains_ignore_case(&t.artist, &q)
                    || contains_ignore_case(&t.album, &q)
                    || contains_ignore_case(&t.album_artist, &q)
            })
            .cloned()
            .collect();
        if let Some(srt) = srt {
            let map = rows.get();
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
    let has_selection = computed(move || selected.with(|s| !s.is_empty()));
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
            let pad_x = move || {
                if zeo_ui_components::tracks::content_break() < 860.0 {
                    16.0
                } else {
                    32.0
                }
            };
            let gap_w = move || {
                if zeo_ui_components::tracks::content_break() < 860.0 {
                    10.0
                } else {
                    16.0
                }
            };
            let filter_w = move || {
                if zeo_ui_components::tracks::content_break() < 860.0 {
                    180.0
                } else {
                    260.0
                }
            };
            view()
                .flex_1()
                .min_w(px(0.0))
                .child(
                    page_header(
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
                    )
                    .px(move || px(pad_x())),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .px(move || px(pad_x()))
                        .pb(px(12.0))
                        .child(
                            input_field(true, |i| {
                                i.label("筛选")
                                    .placeholder("筛选文件名、标题、艺术家、专辑")
                                    .default_value(untracked(|| filter.get()))
                                    .window_drag(false)
                                    .on_change(move |t: &str| filter.set(t.to_owned()))
                            })
                            .w(move || px(filter_w())),
                        )
                        .child(column_selector(visible_cols))
                        .child(view().flex_1().min_w(px(0.0)).window_drag(true))
                        .child({
                            let displayed = displayed.clone();
                            let selected = selected.clone();
                            let anchor = anchor.clone();
                            // 「全选」不放表头格：上游自定义表头对滚动区列
                            // 不落渲染内容（见 upstream.md），工具栏的 nsh_base
                            // 复选框在真机 / headless 都稳。
                            let mark = checkbox(
                                {
                                    let displayed = displayed.clone();
                                    let selected = selected.clone();
                                    move || {
                                        selected.with(|s| {
                                            if s.is_empty() {
                                                return false;
                                            }
                                            displayed.with(|v| {
                                                !v.is_empty()
                                                    && s.len() >= v.len()
                                                    && v.iter().all(|t| s.contains(&t.path))
                                            })
                                        })
                                    }
                                },
                                {
                                    let displayed = displayed.clone();
                                    let selected = selected.clone();
                                    move || {
                                        selected.with(|s| {
                                            if s.is_empty() {
                                                return false;
                                            }
                                            displayed.with(|v| {
                                                if v.is_empty() {
                                                    return false;
                                                }
                                                let any = v.iter().any(|t| s.contains(&t.path));
                                                let all = s.len() >= v.len()
                                                    && v.iter().all(|t| s.contains(&t.path));
                                                any && !all
                                            })
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
                                    .child(
                                        text_button(
                                            Variant::Outline,
                                            Size::Xs,
                                            None,
                                            if zeo_ui_components::tracks::content_break() < 860.0 {
                                                "重命名…"
                                            } else {
                                                "重命名文件…"
                                            },
                                            move || rename_open.set(true),
                                        )
                                        .accessibility_label("重命名文件…"),
                                    )
                                    .child(
                                        text_button(
                                            Variant::Ghost,
                                            Size::Xs,
                                            None,
                                            if zeo_ui_components::tracks::content_break() < 860.0 {
                                                "取消"
                                            } else {
                                                "取消选择"
                                            },
                                            move || {
                                                selected.set(BTreeSet::new());
                                                anchor.set(None);
                                            },
                                        )
                                        .accessibility_label("取消选择"),
                                    )
                            },
                        )),
                )
                .child(
                    view()
                        .flex_1()
                        .min_h(px(0.0))
                        .flex_row()
                        .px(move || px(pad_x()))
                        .pb(px(BAR_SPACE))
                        .gap(move || px(gap_w()))
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .rounded(px(12.0))
                                .border_1()
                                .border_color(nsh_base::theme().input)
                                .bg(nsh_base::theme().card)
                                .shadow_xs()
                                .overflow_hidden()
                                .child(table::table(
                                    displayed,
                                    rows,
                                    selected,
                                    anchor,
                                    sort,
                                    visible_cols,
                                    col_widths,
                                )),
                        )
                        .child(when(
                            move || has_selection.get(),
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
