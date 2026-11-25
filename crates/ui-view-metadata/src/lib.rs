//! 「元数据」视图：全库曲目的标签表格（nsh-base `DataTable` + 复选框
//! 多选），右侧单选 / 批量编辑面板；工具栏有乱码修复 / 批量重命名 /
//! 标签历史三个弹窗入口（前两者作用于当前所选）。
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
mod table;

use nsh_base::{DataTableSort, h_resizable, popover, resizable_panel};
use nsh_core::prelude::*;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use zeo_icons::{Icon, icon};
use zeo_metadata::write::TagField;
use zeo_types::config::TrackEntry;
use zeo_ui_components::button::{RADIUS_MD, Size, Variant, text_button};
use zeo_ui_components::checkbox::checkbox;
use zeo_ui_components::input::text_input;
use zeo_ui_components::page::page_header;
use zeo_ui_store::store;
use zeo_ui_store::theme::*;
pub(crate) use zeo_ui_views_base::no_library;

/// 右侧编辑面板的宽限：拖把手 240–560pt；窗口不够宽时按 `panel_max` 再收。
const PANEL_MIN_W: f32 = 240.0;
const PANEL_MAX_W: f32 = 560.0;
/// 表格最少占分栏宽的比例：面板的实际上限 = 分栏宽 − 它。
const TABLE_MIN_SHARE: f32 = 0.3;

pub fn page() -> View {
    let s = store();
    let rows = signal(HashMap::<String, Arc<MetaRow>>::new());
    let selected = signal(BTreeSet::<String>::new());
    let anchor = signal(Option::<String>::None);
    let filter = signal(String::new());
    let sort = signal(Option::<DataTableSort>::None);
    let visible_cols = signal(untracked(|| {
        s.library
            .with(|l| l.config.metadata_columns.clone())
            .map(|saved| {
                let known = all_columns();
                saved.into_iter().filter(|k| known.contains(k)).collect()
            })
            .unwrap_or_else(default_columns)
    }));
    let col_widths = signal(HashMap::<String, f32>::new());
    // 工具栏批量工具（乱码修复 / 批量重命名，选中至少一首可用）与标签历史
    // （只选中恰好一首时可用）的弹窗开关；`reload` 是历史还原写盘后让右侧
    // 单选面板重建的计数。
    let encoding_open = signal(false);
    let rename_open = signal(false);
    let history_open = signal(false);
    let history_reload = signal(0u32);
    // 右侧面板宽：None 走断点默认（`panel_default_w`），拖过把手后记住
    // 用户值，窗口过断点不再重置（`size` 新值会顶掉拖拽结果）。
    let panel_w = signal(Option::<f32>::None);
    // 列选择变化即落盘（首轮是读出来的值本身，跳过）。
    let first = std::cell::Cell::new(true);
    effect(move || {
        let cols = visible_cols.get();
        if first.replace(false) {
            return;
        }
        zeo_ui_store::set_metadata_columns(cols);
    });

    // 同步修剪：清掉离开曲库的路径与选中集。曲目元数据直接由 SQLite / 索引条目供给，
    // 无需后台再逐曲扫描磁盘标签。
    effect(move || {
        let tracks = s.tracks.get();
        let live: HashSet<&str> = tracks.iter().map(|t| t.path.as_str()).collect();
        rows.update(|r| r.retain(|p, _| live.contains(p.as_str())));
        selected.update(|set| set.retain(|p| live.contains(p.as_str())));
    });

    // 当前显示序：筛选（文件名 / 标题 / 歌手 / 专辑 / 标签，不分大小写）+ app
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
                    || t.genres.iter().any(|g| contains_ignore_case(g, &q))
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
    let single_path = computed(move || {
        selected.with(|s| (s.len() == 1).then(|| s.iter().next().unwrap().clone()))
    });
    let can_history = computed(move || single_path.with(|p| p.is_some()));
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
            let gap_w = move || {
                if zeo_ui_components::tracks::content_break() < 860.0 {
                    10.0
                } else {
                    16.0
                }
            };
            let filter_w = move || {
                if zeo_ui_components::tracks::content_break() < 860.0 {
                    120.0
                } else {
                    260.0
                }
            };
            let panel_default_w = move || {
                let cw = zeo_ui_components::tracks::content_break();
                if cw < 760.0 {
                    280.0
                } else if cw < 960.0 {
                    320.0
                } else {
                    380.0
                }
            };
            // 两块面板平分的宽度：内容宽减两侧页边距与分栏 gap。
            let panels_room =
                move || zeo_ui_components::tracks::content_break() - PAGE_X * 2.0 - gap_w();
            // 把手调的是把手前的表格，面板拿到剩下的。
            let table_min = move || panels_room() * TABLE_MIN_SHARE;
            // 面板的实际上限：min 之和超过可分宽度时上游 `min_of` 按比例缩各
            // min，这里同口径反推。`max_size` 挂它：窗口变窄面板随之收、变宽
            // 再回 `panel_w` 记住的宽。（下限取缩放后的面板 min，防止浮点误差
            // 让 max 跌破 range。）
            let panel_max = move || {
                let room = panels_room();
                let total_min = table_min() + PANEL_MIN_W;
                let scale = if total_min > room {
                    (room / total_min).max(0.0)
                } else {
                    1.0
                };
                (room - table_min() * scale)
                    .max(PANEL_MIN_W * scale)
                    .min(PANEL_MAX_W)
            };
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
                        .px(px(PAGE_X))
                        .pb(px(16.0))
                        .child(
                            // 筛选框：原生 Input 没有前置图标位，左内边距让出
                            // 图标宽、把搜索图标绝对定位叠上去（不吃点击）。
                            view()
                                .w(move || px(filter_w()))
                                .h(px(32.0))
                                .child(
                                    text_input(true)
                                        .size_full()
                                        .label("筛选")
                                        .placeholder("筛选文件名、标题、艺术家、专辑、标签")
                                        .default_value(untracked(|| filter.get()))
                                        .window_drag(false)
                                        .pl(px(26.0))
                                        .on_change(move |t: &str| filter.set(t.to_owned())),
                                )
                                .child(
                                    view()
                                        .absolute()
                                        .inset_0()
                                        .flex_row()
                                        .items_center()
                                        .pl(px(9.0))
                                        .pointer_events(PointerEvents::None)
                                        .child(icon(Icon::Search, 14.0, faint)),
                                ),
                        )
                        .child(column_selector(visible_cols))
                        .child(panel::encoding_button(encoding_open, has_selection))
                        .child(panel::rename_button(rename_open, has_selection))
                        .child(panel::history_button(history_open, can_history))
                        .child(view().flex_1().min_w(px(0.0)).window_drag(true))
                        .child({
                            // 「全选」不放表头格：工具栏的 nsh_base 复选框
                            // 在真机 / headless 都稳。
                            let mark = checkbox(
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
                                },
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
                                // 窄窗口省掉「全选」文字只留复选框：工具栏加了两颗
                                // 批量工具钮，文字再占 34pt 会把行尾挤出可视区。
                                .child(when(
                                    move || zeo_ui_components::tracks::content_break() >= 860.0,
                                    || {
                                        text("全选")
                                            .text_size(px(13.0))
                                            .line_height(px(13.0))
                                            .text_color(muted_fg())
                                    },
                                ))
                                .into_element()
                        })
                        .child(when(
                            move || selected.with(|set| !set.is_empty()),
                            move || {
                                text_button(
                                    Variant::Ghost,
                                    Size::Xs,
                                    Some(Icon::Close),
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
                                .accessibility_label("取消选择")
                            },
                        )),
                )
                .child(
                    view()
                        .flex_1()
                        .min_h(px(0.0))
                        .px(px(PAGE_X))
                        .pb(px(BAR_SPACE))
                        .child(
                            // 表格 | 面板分栏：把手落在两者的 gap 正中（分栏
                            // 模型认得 flex gap），拖动在 core 里逐帧走；
                            // 面板隐藏时不占宽、把手也随之消失。
                            h_resizable()
                                .gap(move || px(gap_w()))
                                .handle_class(
                                    "w-[2px] bg-transparent group-hover:bg-primary \
                                     group-active:bg-primary",
                                )
                                .handle_accessibility_label("调整面板宽度")
                                .child(
                                    resizable_panel().min_size(move || px(table_min())).child(
                                        view()
                                            .size_full()
                                            .min_w(px(0.0))
                                            .rounded(px(12.0))
                                            .border_1()
                                            .border_color(border())
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
                                    ),
                                )
                                .child(
                                    resizable_panel()
                                        .size(move || {
                                            Some(px(panel_w.get().unwrap_or_else(panel_default_w)))
                                        })
                                        .min_size(px(PANEL_MIN_W))
                                        .max_size(move || Some(px(panel_max())))
                                        .fixed(true)
                                        .visible(move || has_selection.get())
                                        .on_resize(move |size: &Points| panel_w.set(Some(size.0)))
                                        .child(panel::panel(
                                            displayed,
                                            rows,
                                            selected,
                                            // 面板 ✕：清空选中集与范围锚点。
                                            Rc::new(move || {
                                                selected.set(BTreeSet::new());
                                                anchor.set(None);
                                            }),
                                            history_reload,
                                        )),
                                ),
                        ),
                )
                .into_element()
        }))
        // 批量工具与标签历史弹窗（页面级：入口是工具栏图标钮；作用目标按
        // 打开时的所选快照算，清空选中会连带收掉弹窗）。
        .child(when(
            move || encoding_open.get() && has_selection.get(),
            move || {
                let (order, entries, sample) = panel::tool_context(displayed, selected);
                panel::encoding_dialog(order, entries, sample, rows, encoding_open)
            },
        ))
        .child(when(
            move || rename_open.get() && has_selection.get(),
            move || {
                let (order, entries, sample) = panel::tool_context(displayed, selected);
                panel::rename_dialog(order, entries, sample, rows, selected, rename_open)
            },
        ))
        .child(when(
            move || history_open.get() && single_path.with(|p| p.is_some()),
            move || {
                let path = single_path.get_untracked().unwrap_or_default();
                panel::history_dialog(path, rows, history_open, history_reload)
            },
        ))
}
