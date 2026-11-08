//! 「元数据」视图：全库曲目的标签表格（nsh-base `DataTable` + 复选框
//! 多选），右侧单选 / 批量编辑面板与纯文件模板重命名弹窗。
//!
//! 行的标签值是视图自有状态（`rows` 信号）：后台线程分批用
//! `write::read_row` 读出，不进 `MetaCache` / SQLite（全局元数据缓存
//! 保持精简，见 AGENTS 约定）；未加载的行回落 `TrackEntry` 索引值。

mod panel;
mod rename;
mod table;

use nsh_base::{DataTableSort, popover};
use nsh_core::prelude::*;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use super::super::components::button::{RADIUS_MD, Size, Variant, text_button};
use super::super::components::checkbox::checkbox;
use super::super::components::form::input_field;
use super::super::components::icons::{Icon, icon};
use super::super::components::page::page_header;
use super::super::store::store;
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
#[allow(dead_code)]
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

/// 大小写无关的子串匹配（ASCII 快速路径，不分配 String）。
fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle.is_ascii() {
        let needle_bytes = needle.as_bytes();
        haystack.as_bytes().windows(needle_bytes.len()).any(|w| {
            w.iter()
                .zip(needle_bytes.iter())
                .all(|(h, n)| h.to_ascii_lowercase() == n.to_ascii_lowercase())
        })
    } else {
        haystack.to_lowercase().contains(&needle.to_lowercase())
    }
}

fn all_columns() -> BTreeSet<String> {
    table::COLUMNS.iter().map(|(k, ..)| k.to_string()).collect()
}

const COLUMN_GROUPS: &[(&str, &[&str])] = &[
    (
        "基本信息",
        &[
            "file_name",
            "title",
            "artist",
            "album",
            "album_artist",
            "genre",
            "year",
            "track_number",
            "track_total",
            "disc_number",
            "discs_total",
        ],
    ),
    (
        "人员与版权",
        &[
            "composer",
            "conductor",
            "lyricist",
            "publisher",
            "copyright",
        ],
    ),
    ("封面与歌词", &["rating", "pictures", "lyrics", "comment"]),
    (
        "ReplayGain",
        &[
            "track_replay_gain",
            "track_replay_peak",
            "album_replay_gain",
            "album_replay_peak",
        ],
    ),
];

/// 默认列：包含「基本信息」与「封面与歌词」集合中的全部列（共 15 列）。
fn default_columns() -> BTreeSet<String> {
    let mut cols = BTreeSet::new();
    for (group_name, keys) in COLUMN_GROUPS {
        if *group_name == "基本信息" || *group_name == "封面与歌词" {
            for key in *keys {
                cols.insert((*key).to_string());
            }
        }
    }
    cols
}

fn column_selector(visible_cols: Signal<BTreeSet<String>>) -> AnyElement {
    let open = signal(false);
    let gate = crate::ui::store::menu_control();
    effect(gate.evicted(open));

    let trigger = view()
        .h(px(32.0))
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .px(px(10.0))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(border())
        .cursor_pointer()
        .window_drag(false)
        .bg(move || {
            if gate.is_open() {
                hover()
            } else {
                nsh_base::theme().card
            }
        })
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.8))
        .role(AccessibilityRole::Button)
        .label("列显示设置")
        .child(
            text(move || {
                let count = visible_cols.with(|s| s.len());
                let total = table::COLUMNS.len();
                format!("显示列 ({count}/{total})")
            })
            .text_size(px(13.0))
            .line_height(px(13.0))
            .text_color(fg()),
        )
        .child(icon(Icon::ChevronDown, 12.0, muted_fg));

    let mut items = Vec::new();
    for (group_name, keys) in COLUMN_GROUPS {
        items.push(
            view()
                .px(px(8.0))
                .pt(px(8.0))
                .pb(px(4.0))
                .child(
                    text(*group_name)
                        .text_size(px(11.0))
                        .line_height(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(muted_fg()),
                )
                .into_element(),
        );
        for key in *keys {
            let key_s = (*key).to_string();
            let title_str = table::COLUMNS
                .iter()
                .find(|(k, ..)| *k == *key)
                .map(|(.., t, _, _, _)| *t)
                .unwrap_or(*key);
            let is_checked = {
                let key_s = key_s.clone();
                move || visible_cols.with(|s| s.contains(&key_s))
            };
            let toggle = {
                let key_s = key_s.clone();
                move || {
                    visible_cols.update(|set| {
                        if set.contains(&key_s) {
                            if set.len() > 1 {
                                set.remove(&key_s);
                            }
                        } else {
                            set.insert(key_s.clone());
                        }
                    });
                }
            };
            let on_row_click = toggle.clone();
            items.push(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(8.0))
                    .py(px(5.0))
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .hover(|st| st.bg(hover()))
                    .active(|st| st.opacity(0.85))
                    .on_press(move || on_row_click())
                    .child(checkbox(is_checked, || false, title_str, {
                        let toggle = toggle.clone();
                        move |_| toggle()
                    }))
                    .child(
                        text(title_str)
                            .flex_1()
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(fg()),
                    )
                    .into_element(),
            );
        }
    }

    let popup = view()
        .w(px(230.0))
        .h(px(390.0))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(border())
        .bg(nsh_base::theme().popover)
        .shadow_md()
        .overflow_hidden()
        .flex_col()
        .window_drag(false)
        .child(
            view()
                .h(px(36.0))
                .px(px(12.0))
                .border_b_1()
                .border_b_color(border())
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text("选择显示列")
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(muted_fg()),
                )
                .child(
                    view()
                        .flex_row()
                        .gap(px(4.0))
                        .child(text_button(
                            Variant::Ghost,
                            Size::Xs,
                            None,
                            "全选",
                            move || visible_cols.set(all_columns()),
                        ))
                        .child(text_button(
                            Variant::Ghost,
                            Size::Xs,
                            None,
                            "默认",
                            move || visible_cols.set(default_columns()),
                        )),
                ),
        )
        .child(
            scroll()
                .flex_1()
                .min_h(px(0.0))
                .child(view().p(px(6.0)).gap(px(2.0)).children(items)),
        );

    let close = {
        let open = open.clone();
        move || {
            open.set(false);
            gate.close();
        }
    };

    let selector = popover()
        .open(gate.open_with(open))
        .on_open_change(gate.popover_change(open))
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(6.0))
        .margin(px(8.0))
        .trigger(trigger)
        .child(popup);

    view()
        .child(when(
            move || gate.is_open(),
            move || {
                overlay()
                    .test_id("column_selector_overlay")
                    .absolute()
                    .inset_0()
                    .on_press({
                        let close = close.clone();
                        move || close()
                    })
                    .on_context_menu({
                        let close = close.clone();
                        move |_: &nsh_core::ContextMenuEvent| close()
                    })
            },
        ))
        .child(selector)
        .into_element()
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
                if crate::ui::components::tracks::content_width() < 860.0 {
                    16.0
                } else {
                    32.0
                }
            };
            let gap_w = move || {
                if crate::ui::components::tracks::content_width() < 860.0 {
                    10.0
                } else {
                    16.0
                }
            };
            let filter_w = move || {
                if crate::ui::components::tracks::content_width() < 860.0 {
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
                            input_field(true)
                                .w(move || px(filter_w()))
                                .label("筛选")
                                .placeholder("筛选文件名、标题、艺术家、专辑")
                                .default_value(untracked(|| filter.get()))
                                .window_drag(false)
                                .on_change(move |t: &str| filter.set(t.to_owned())),
                        )
                        .child(column_selector(visible_cols))
                        .child(view().flex_1().min_w(px(0.0)).window_drag(true))
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
                                            if crate::ui::components::tracks::content_width()
                                                < 860.0
                                            {
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
                                            if crate::ui::components::tracks::content_width()
                                                < 860.0
                                            {
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

    #[test]
    fn contains_ignore_case_matches() {
        assert!(contains_ignore_case("Sine.flac", "sine"));
        assert!(contains_ignore_case("Sine.flac", "FLAC"));
        assert!(!contains_ignore_case("Sine.flac", "wav"));
        assert!(contains_ignore_case("测试歌曲.flac", "测试"));
        assert!(contains_ignore_case("Hello World", ""));
    }

    #[test]
    fn column_sets_have_expected_members() {
        let all = all_columns();
        assert!(all.contains("file_name"));
        assert!(all.contains("title"));
        assert!(all.contains("artist"));
        assert!(all.contains("album"));
        assert_eq!(all.len(), table::COLUMNS.len());

        let default_cols = default_columns();
        assert!(default_cols.contains("file_name"));
        assert!(default_cols.contains("title"));
        assert!(default_cols.contains("artist"));
        assert!(default_cols.contains("album"));
        assert!(default_cols.contains("rating"));
        assert!(default_cols.contains("pictures"));
        assert!(default_cols.contains("lyrics"));
        assert!(default_cols.contains("comment"));
        assert!(!default_cols.contains("composer"));
        assert!(!default_cols.contains("track_replay_gain"));
        assert!(default_cols.len() < all.len());
        for col in &default_cols {
            assert!(all.contains(col));
        }
    }
}
