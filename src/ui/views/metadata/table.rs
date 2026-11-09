//! 表格：nsh-base `DataTable` 的列配置（首列选择标记 + 固定文件名 + 22
//! 个标签列，对应 spec §一）、单元格文本 / 排序比较与行选择交互。

use std::borrow::Cow;
use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use nsh_base::{
    DataTableAlign, DataTableClasses, DataTableColumn, DataTablePress, DataTableRows,
    DataTableSelection, DataTableSort, data_table,
};
use nsh_core::prelude::*;
use nsh_core::{Cursor, PointerEvents};

use super::super::super::theme::*;
use super::{CoverState, MetaRow};
use crate::config::TrackEntry;
use crate::metadata::write::TagField;

/// 标签列：key、标题、宽（pt）、对齐、可排序（spec §一）。
pub(super) const COLUMNS: &[(&str, &str, f32, DataTableAlign, bool)] = &[
    ("file_name", "文件名", 200.0, DataTableAlign::Start, true),
    ("title", "标题", 180.0, DataTableAlign::Start, true),
    ("artist", "艺术家", 150.0, DataTableAlign::Start, true),
    ("album", "专辑", 160.0, DataTableAlign::Start, true),
    (
        "album_artist",
        "专辑艺术家",
        140.0,
        DataTableAlign::Start,
        true,
    ),
    ("genre", "流派", 120.0, DataTableAlign::Start, true),
    ("year", "年份", 75.0, DataTableAlign::End, true),
    ("track_number", "音轨号", 70.0, DataTableAlign::End, true),
    ("track_total", "音轨总数", 70.0, DataTableAlign::End, true),
    ("disc_number", "碟号", 65.0, DataTableAlign::End, true),
    ("discs_total", "碟总数", 65.0, DataTableAlign::End, true),
    ("composer", "作曲", 130.0, DataTableAlign::Start, true),
    ("conductor", "指挥", 120.0, DataTableAlign::Start, true),
    ("lyricist", "作词", 120.0, DataTableAlign::Start, true),
    ("publisher", "发行商", 130.0, DataTableAlign::Start, true),
    ("comment", "注释", 150.0, DataTableAlign::Start, true),
    ("lyrics", "歌词", 160.0, DataTableAlign::Start, true),
    ("copyright", "版权", 140.0, DataTableAlign::Start, true),
    ("rating", "评分", 85.0, DataTableAlign::Center, true),
    ("pictures", "封面状态", 80.0, DataTableAlign::Center, true),
    (
        "track_replay_gain",
        "音轨增益",
        85.0,
        DataTableAlign::End,
        true,
    ),
    (
        "track_replay_peak",
        "音轨峰值",
        80.0,
        DataTableAlign::End,
        true,
    ),
    (
        "album_replay_gain",
        "专辑增益",
        85.0,
        DataTableAlign::End,
        true,
    ),
    (
        "album_replay_peak",
        "专辑峰值",
        80.0,
        DataTableAlign::End,
        true,
    ),
];

/// 数值比较的列（增益剥 ` dB` 尾解析，解析不出回落文本序；评分比星数）。
const NUMERIC: &[&str] = &[
    "year",
    "track_number",
    "track_total",
    "disc_number",
    "discs_total",
    "rating",
    "track_replay_gain",
    "track_replay_peak",
    "album_replay_gain",
    "album_replay_peak",
];

/// 一个单元格的显示文本：行已加载用标签值（增益补 ` dB`、评分画星、
/// 封面状态译名），未加载回落 `TrackEntry` 索引值（多余列为空）。
#[allow(dead_code)]
pub(super) fn cell_text(key: &str, track: &TrackEntry, row: Option<&MetaRow>) -> String {
    cell_value(key, track, row).into_owned()
}

fn field_str(row: &MetaRow, f: TagField) -> &str {
    row.fields.get(&f).map(|s| s.as_str()).unwrap_or("")
}

/// 单元格零分配提取：借用已加载行字段或 `TrackEntry` 现有字串，减少渲染期
/// 堆分配与中间临时 String 产生。
pub(super) fn cell_value<'a>(
    key: &str,
    track: &'a TrackEntry,
    row: Option<&'a MetaRow>,
) -> Cow<'a, str> {
    match row {
        Some(row) => loaded_cell_cow(key, track, row),
        None => entry_cell_cow(key, track),
    }
}

fn loaded_cell_cow<'a>(key: &str, track: &'a TrackEntry, row: &'a MetaRow) -> Cow<'a, str> {
    let gain = |f: TagField| {
        let v = field_str(row, f);
        if v.is_empty() {
            Cow::Borrowed("")
        } else {
            Cow::Owned(format!("{v} dB"))
        }
    };
    match key {
        "file_name" => Cow::Borrowed(track.file_name.as_str()),
        "title" => Cow::Borrowed(field_str(row, TagField::Title)),
        "artist" => Cow::Borrowed(field_str(row, TagField::Artist)),
        "album" => Cow::Borrowed(field_str(row, TagField::Album)),
        "album_artist" => Cow::Borrowed(field_str(row, TagField::AlbumArtist)),
        "genre" => Cow::Borrowed(field_str(row, TagField::Genre)),
        "year" => Cow::Borrowed(field_str(row, TagField::Year)),
        "track_number" => Cow::Borrowed(field_str(row, TagField::Track)),
        "track_total" => Cow::Borrowed(field_str(row, TagField::TrackTotal)),
        "disc_number" => Cow::Borrowed(field_str(row, TagField::Disc)),
        "discs_total" => Cow::Borrowed(field_str(row, TagField::DiscTotal)),
        "composer" => Cow::Borrowed(field_str(row, TagField::Composer)),
        "conductor" => Cow::Borrowed(field_str(row, TagField::Conductor)),
        "lyricist" => Cow::Borrowed(field_str(row, TagField::Lyricist)),
        "publisher" => Cow::Borrowed(field_str(row, TagField::Publisher)),
        "comment" => Cow::Borrowed(field_str(row, TagField::Comment)),
        "lyrics" => Cow::Borrowed(field_str(row, TagField::Lyrics)),
        "copyright" => Cow::Borrowed(field_str(row, TagField::Copyright)),
        "rating" => match field_str(row, TagField::Rating) {
            "1" => Cow::Borrowed("★"),
            "2" => Cow::Borrowed("★★"),
            "3" => Cow::Borrowed("★★★"),
            "4" => Cow::Borrowed("★★★★"),
            "5" => Cow::Borrowed("★★★★★"),
            s => match s.parse::<usize>() {
                Ok(n) if n > 0 => Cow::Owned("★".repeat(n)),
                _ => Cow::Borrowed(""),
            },
        },
        "pictures" => match row.cover {
            CoverState::Embedded => Cow::Borrowed("内嵌"),
            CoverState::File => Cow::Borrowed("文件"),
            CoverState::None => Cow::Borrowed("无"),
        },
        "track_replay_gain" => gain(TagField::TrackGain),
        "track_replay_peak" => Cow::Borrowed(field_str(row, TagField::TrackPeak)),
        "album_replay_gain" => gain(TagField::AlbumGain),
        "album_replay_peak" => Cow::Borrowed(field_str(row, TagField::AlbumPeak)),
        _ => Cow::Borrowed(""),
    }
}

/// 直接读取曲目索引条目（`TrackEntry` / `MetaInfo`）对应的标签值。
/// 数据库字段无回退，真实标签缺失时保持空串。
fn entry_cell_cow<'a>(key: &str, t: &'a TrackEntry) -> Cow<'a, str> {
    let num = |v: Option<u32>| match v {
        Some(n) => Cow::Owned(n.to_string()),
        None => Cow::Borrowed(""),
    };
    let gain = |v: Option<f32>| match v {
        Some(v) => Cow::Owned(format!("{v:.2} dB")),
        None => Cow::Borrowed(""),
    };
    let peak = |v: Option<f32>| match v {
        Some(v) => Cow::Owned(format!("{v:.6}")),
        None => Cow::Borrowed(""),
    };
    match key {
        "file_name" => Cow::Borrowed(t.file_name.as_str()),
        "title" => Cow::Borrowed(t.title.as_str()),
        "artist" => Cow::Borrowed(t.artist.as_str()),
        "album" => Cow::Borrowed(t.album.as_str()),
        "album_artist" => Cow::Borrowed(t.meta.album_artist.as_str()),
        "genre" => {
            if t.genres.is_empty() {
                Cow::Borrowed("")
            } else if t.genres.len() == 1 {
                Cow::Borrowed(t.genres[0].as_str())
            } else {
                Cow::Owned(t.genres.join("; "))
            }
        }
        "year" => num(t.year),
        "track_number" => num(t.track),
        "track_total" => num(t.track_total),
        "disc_number" => num(t.disk),
        "discs_total" => num(t.disk_total),
        "composer" => {
            if t.composers.is_empty() {
                Cow::Borrowed("")
            } else if t.composers.len() == 1 {
                Cow::Borrowed(t.composers[0].as_str())
            } else {
                Cow::Owned(t.composers.join("; "))
            }
        }
        "conductor" => Cow::Borrowed(t.conductor.as_deref().unwrap_or("")),
        "lyricist" => Cow::Borrowed(t.lyricist.as_deref().unwrap_or("")),
        "publisher" => Cow::Borrowed(t.publisher.as_deref().unwrap_or("")),
        "comment" => Cow::Borrowed(t.comment.as_deref().unwrap_or("")),
        "lyrics" => match t.lyrics.as_deref() {
            Some(lrc) => Cow::Owned(super::lyrics_preview(lrc)),
            None => Cow::Borrowed(""),
        },
        "copyright" => Cow::Borrowed(t.copyright.as_deref().unwrap_or("")),
        "rating" => match t.rating {
            Some(1) => Cow::Borrowed("★"),
            Some(2) => Cow::Borrowed("★★"),
            Some(3) => Cow::Borrowed("★★★"),
            Some(4) => Cow::Borrowed("★★★★"),
            Some(5) => Cow::Borrowed("★★★★★"),
            Some(n) if n > 0 => Cow::Owned("★".repeat(n as usize)),
            _ => Cow::Borrowed(""),
        },
        "pictures" => {
            if crate::metadata::covers::find_sidecar(std::path::Path::new(&t.path)).is_some() {
                Cow::Borrowed("文件")
            } else if t.has_art {
                Cow::Borrowed("内嵌")
            } else {
                Cow::Borrowed("无")
            }
        }
        "track_replay_gain" => gain(t.replaygain.track_gain),
        "track_replay_peak" => peak(t.replaygain.track_peak),
        "album_replay_gain" => gain(t.replaygain.album_gain),
        "album_replay_peak" => peak(t.replaygain.album_peak),
        _ => Cow::Borrowed(""),
    }
}

/// 单元格比较（`displayed` 排序用，纯函数）：空值恒排最后（与方向
/// 无关）；数值列按数字（增益剥 ` dB`，非数字回落文本序）；文本列
/// 不分大小写。
#[allow(dead_code)]
pub(super) fn compare_cells(key: &str, a: &str, b: &str, ascending: bool) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    let ord = if NUMERIC.contains(&key) {
        let parse = |s: &str| s.trim_end_matches(" dB").parse::<f64>().ok();
        match (parse(a), parse(b)) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => compare_text(a, b),
        }
    } else {
        compare_text(a, b)
    };
    if ascending { ord } else { ord.reverse() }
}

fn compare_text(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        for (ca, cb) in a.bytes().zip(b.bytes()) {
            let ord = ca.to_ascii_lowercase().cmp(&cb.to_ascii_lowercase());
            if ord != Ordering::Equal {
                return ord;
            }
        }
        a.len().cmp(&b.len()).then_with(|| a.cmp(b))
    } else {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    }
}

#[derive(PartialEq)]
enum SortKey {
    Empty,
    Number(f64),
    Text { lower: String, orig: String },
}

fn compare_sort_keys(a: &SortKey, b: &SortKey, ascending: bool) -> Ordering {
    match (a, b) {
        (SortKey::Empty, SortKey::Empty) => return Ordering::Equal,
        (SortKey::Empty, _) => return Ordering::Greater,
        (_, SortKey::Empty) => return Ordering::Less,
        _ => {}
    }
    let ord = match (a, b) {
        (SortKey::Number(x), SortKey::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (
            SortKey::Text {
                lower: lx,
                orig: ox,
            },
            SortKey::Text {
                lower: ly,
                orig: oy,
            },
        ) => lx.cmp(ly).then_with(|| ox.cmp(oy)),
        _ => Ordering::Equal,
    };
    if ascending { ord } else { ord.reverse() }
}

/// `displayed` memo 的就地排序（decorate-sort：预解析键与索引，
/// 比较循环内 0 堆分配，重排移动原值避免全量克隆 TrackEntry）。
pub(super) fn sort_entries(
    list: &mut Vec<TrackEntry>,
    key: &str,
    ascending: bool,
    rows: &HashMap<String, Arc<MetaRow>>,
) {
    let mut keyed: Vec<(SortKey, usize)> = list
        .iter()
        .enumerate()
        .map(|(idx, t)| {
            let row = rows.get(t.path.as_str()).map(|r| r.as_ref());
            let val = cell_value(key, t, row);
            let sort_key = if val.is_empty() {
                SortKey::Empty
            } else if NUMERIC.contains(&key) {
                match val.trim_end_matches(" dB").parse::<f64>().ok() {
                    Some(n) => SortKey::Number(n),
                    None => SortKey::Text {
                        lower: val.to_lowercase(),
                        orig: val.into_owned(),
                    },
                }
            } else {
                SortKey::Text {
                    lower: val.to_lowercase(),
                    orig: val.into_owned(),
                }
            };
            (sort_key, idx)
        })
        .collect();

    keyed.sort_by(|(a, _), (b, _)| compare_sort_keys(a, b, ascending));

    let old = std::mem::take(list);
    let mut old_opts: Vec<Option<TrackEntry>> = old.into_iter().map(Some).collect();
    list.reserve(old_opts.len());
    for (_, idx) in keyed {
        if let Some(entry) = old_opts[idx].take() {
            list.push(entry);
        }
    }
}

/// 标签表格：选中态由 `selected` 集合驱动行高亮（`row_classes` 的
/// `bg-primary/8`）；表格自身 `selection` 恒空，不画它竞争的单选高亮。
/// 排序由 app 侧做（`sort` 信号 + `displayed` 重排）。
pub(super) fn table(
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    anchor: Signal<Option<String>>,
    sort: Signal<Option<DataTableSort>>,
    visible_cols: Signal<BTreeSet<String>>,
    col_widths: Signal<HashMap<String, f32>>,
) -> impl IntoElement {
    let defs = {
        let visible_cols = visible_cols.clone();
        let col_widths = col_widths.clone();
        move || {
            let active = visible_cols.get();
            let widths = col_widths.get();
            let mut defs = Vec::with_capacity(COLUMNS.len());
            for (key, title, width, align, _) in COLUMNS {
                if !active.contains(*key) {
                    continue;
                }
                let mut c = DataTableColumn::default();
                c.key = SharedString::from(*key);
                c.title = Some(SharedString::from(*title));
                let cur_w = widths.get(*key).copied().unwrap_or(*width);
                c.width = Some(px(cur_w));
                c.align = *align;
                c.sortable = false;
                c.resizable = Some(false);
                c.fixed = *key == "file_name";
                if *key == "file_name" {
                    // sticky 固定文件名列：右边框颜色加深（border-foreground/25，
                    // 显著深于普通网格线的 border-border/50），保证与滚动列分界清晰。
                    // 普通省略已由 classes.cell 与 classes.header_cell 统一处理。
                    c.header_class = Some(SharedString::from(
                        "bg-card border-r border-foreground/25 font-medium",
                    ));
                    c.cell_class = Some(SharedString::from("border-r border-foreground/25"));
                }
                // 其余列保持 None，不设多余列级覆盖，消除拖拽改列宽时逐格的字串拼接与堆分配开销。
                defs.push(c);
            }
            defs
        }
    };

    let visible_window = signal((0usize, 120usize));

    let data = computed({
        let displayed = displayed.clone();
        move || -> DataTableRows {
            let list = displayed.get();
            let total = list.len();
            let (win_start, win_end) = visible_window.get();
            let start = win_start.min(total);
            let end = win_end.min(total).max(start);
            let slice = &list[start..end];
            let map = rows.get();
            let active = visible_cols.get();
            let mut data = DataTableRows::new(slice.len());
            // 仅对当前可见窗口切片查表与生成文本，避免全库数万行全量物化
            let row_ptrs: Vec<Option<&MetaRow>> = slice
                .iter()
                .map(|t| map.get(t.path.as_str()).map(|r| r.as_ref()))
                .collect();
            for &(key, ..) in COLUMNS {
                if !active.contains(key) {
                    continue;
                }
                data = data.texts(
                    key,
                    slice
                        .iter()
                        .zip(row_ptrs.iter())
                        .map(|(t, row)| cell_value(key, t, *row)),
                );
            }
            data
        }
    });

    let row_classes = move || -> Vec<SharedString> {
        let set = selected.get();
        let total = displayed.with(|v| v.len());
        let (win_start, win_end) = visible_window.get();
        let start = win_start.min(total);
        let end = win_end.min(total).max(start);
        displayed.with(|v| {
            let slice = &v[start..end];
            if set.is_empty() {
                return vec![SharedString::new_static("bg-card"); slice.len()];
            }
            let active = SharedString::new_static("bg-primary/8");
            let card = SharedString::new_static("bg-card");
            slice
                .iter()
                .map(|t| {
                    if set.contains(&t.path) {
                        // 与侧栏悬停同一个 8% 主色（`primary().opacity(0.08)`，
                        // sidebar.rs）——选中行跟着主题色走，不再用 accent 灰底。
                        active.clone()
                    } else {
                        // 未选中行设不透明背景，横向滚动时固定列不会透光重叠文字。
                        card.clone()
                    }
                })
                .collect()
        })
    };

    let mut classes = DataTableClasses::default();
    // sticky 表头：底边框颜色（border-b border-foreground/25）
    classes.header = Some(SharedString::from("bg-card border-b border-foreground/25"));
    classes.header_content = Some(SharedString::from("h-full self-stretch"));
    // 表头单元格：竖向分隔网格线（border-r border-border/50）
    classes.header_cell = Some(SharedString::from(
        "text-muted-foreground font-medium border-r border-border/50",
    ));
    // 普通单元格：竖向分隔网格线（border-r border-border/50）
    classes.cell = Some(SharedString::from("px-3 border-r border-border/50"));
    // 不设 fixed_edge：上游在固定列后缘叠画两条 1pt 分隔（表体一条 +
    // 固定区逐行各一条，view.rs `edge()`），与固定列单元格自身的
    // border-r border-foreground/25 连叠后比表头底线明显深。固定列分隔
    // 只留列级 cell_class / header_class 那条 border-r border-foreground/25，
    // 与表头底线（border-foreground/25）同深。
    // 行体：横向分隔网格线（border-b border-border/50）
    classes.row = Some(SharedString::from("border-b border-border/50"));
    classes.row_hover = Some(SharedString::from("bg-accent/40"));

    let guide_line_x = signal(Option::<f32>::None);

    let render_head = {
        let sort = sort.clone();
        let col_widths = col_widths.clone();
        let guide_line_x = guide_line_x.clone();
        move |cell: &nsh_base::DataTableHeaderCell| -> AnyElement {
            let key = cell.key.to_string();
            let title = cell.title.clone();
            let align = COLUMNS
                .iter()
                .find(|(k, ..)| *k == key)
                .map(|(.., a, _)| *a)
                .unwrap_or(DataTableAlign::Start);

            let is_hovered = signal(false);
            let is_dragging = signal(false);
            let drag_origin = Rc::new(Cell::new(None::<(f32, f32, f32)>));
            let last_tap = Rc::new(Cell::new(None::<Instant>));

            let sort_key = key.clone();
            let sort_toggle = {
                let sort = sort.clone();
                move || {
                    let next = match sort.get() {
                        Some(s) if s.key.as_str() == sort_key.as_str() => match s.direction {
                            nsh_base::DataTableSortDirection::Ascending => {
                                let mut s = DataTableSort::default();
                                s.key = sort_key.clone().into();
                                s.direction = nsh_base::DataTableSortDirection::Descending;
                                Some(s)
                            }
                            nsh_base::DataTableSortDirection::Descending => None,
                        },
                        _ => {
                            let mut s = DataTableSort::default();
                            s.key = sort_key.clone().into();
                            s.direction = nsh_base::DataTableSortDirection::Ascending;
                            Some(s)
                        }
                    };
                    sort.set(next);
                }
            };

            let drag_key = key.clone();
            let drag_widths = col_widths.clone();
            let drag_guide = guide_line_x.clone();
            let drag_hovered = is_hovered.clone();
            let drag_dragging = is_dragging.clone();

            let handle_start = {
                let drag_origin = drag_origin.clone();
                let col_widths = drag_widths.clone();
                let guide_line_x = drag_guide.clone();
                let is_dragging = drag_dragging.clone();
                let key = drag_key.clone();
                move |e: &ResponderEvent| {
                    is_dragging.set(true);
                    let default_w = COLUMNS
                        .iter()
                        .find(|(k, ..)| *k == key)
                        .map(|(.., w, _, _)| *w)
                        .unwrap_or(120.0);
                    let cur_w = col_widths
                        .with(|m| m.get(&key).copied())
                        .unwrap_or(default_w);
                    let start_mouse_x = e.page.x.0;
                    let pad_x = if crate::ui::components::tracks::content_break() < 860.0 {
                        16.0
                    } else {
                        32.0
                    };
                    // 把手 14pt 宽、贴表头单元格右缘（即列线所在）；`location`
                    // 是落点在把手内的坐标，随抓握点变。列线页面 x = 落点 +
                    // 落点到把手右缘的距离，与抓在把手哪里无关。
                    let to_right = (14.0 - e.location.x.0).clamp(0.0, 14.0);
                    // 表格容器在页面里的左缘：侧栏 + 页边距 + 卡片 1pt 边框。
                    let table_left = crate::ui::theme::SIDEBAR + pad_x + 1.0;
                    let start_guide_x = e.page.x.0 + to_right - table_left - 2.0;
                    drag_origin.set(Some((start_mouse_x, cur_w, start_guide_x)));
                    guide_line_x.set(Some(start_guide_x));
                }
            };

            let handle_move = {
                let drag_origin = drag_origin.clone();
                let guide_line_x = drag_guide.clone();
                move |e: &ResponderEvent| {
                    if let Some((start_mouse_x, cur_w, start_guide_x)) = drag_origin.get() {
                        let delta = e.page.x.0 - start_mouse_x;
                        let min_delta = 50.0 - cur_w;
                        let clamped_delta = delta.max(min_delta);
                        guide_line_x.set(Some(start_guide_x + clamped_delta));
                    }
                }
            };

            let handle_end = {
                let drag_origin = drag_origin.clone();
                let col_widths = drag_widths.clone();
                let guide_line_x = drag_guide.clone();
                let is_dragging = drag_dragging.clone();
                let last_tap = last_tap.clone();
                let key = drag_key.clone();
                move |e: &ResponderEvent| {
                    is_dragging.set(false);
                    if let Some((start_mouse_x, cur_w, _)) = drag_origin.take() {
                        guide_line_x.set(None);
                        let delta = e.page.x.0 - start_mouse_x;
                        if delta.abs() < 2.0 {
                            let now = Instant::now();
                            if let Some(prev) = last_tap.get() {
                                if now.duration_since(prev).as_millis() < 350 {
                                    let def_w = COLUMNS
                                        .iter()
                                        .find(|(k, ..)| *k == key)
                                        .map(|(.., w, _, _)| *w)
                                        .unwrap_or(120.0);
                                    col_widths.update(|m| {
                                        m.insert(key.clone(), def_w);
                                    });
                                    last_tap.set(None);
                                    return;
                                }
                            }
                            last_tap.set(Some(now));
                        }
                        let final_w = (cur_w + delta).max(50.0);
                        col_widths.update(|m| {
                            m.insert(key.clone(), final_w);
                        });
                    }
                }
            };

            let sort_indicator = {
                let sort = sort.clone();
                let key = key.clone();
                dynamic(move || match sort.get() {
                    Some(s) if s.key.as_str() == key.as_str() => {
                        let arrow = match s.direction {
                            nsh_base::DataTableSortDirection::Ascending => "▲",
                            nsh_base::DataTableSortDirection::Descending => "▼",
                        };
                        text(arrow)
                            .text_size(px(9.0))
                            .line_height(px(9.0))
                            .text_color(primary())
                            .ml(px(4.0))
                            .into_element()
                    }
                    _ => view().w(px(0.0)).into_element(),
                })
            };

            let handle_view = view()
                .test_id(&format!("resize:{key}"))
                .absolute()
                .right_0()
                .top_0()
                .bottom_0()
                .w(px(14.0))
                .cursor(Cursor::ColResize)
                .on_mouse_enter(move |_: &nsh_core::MouseEvent| drag_hovered.set(true))
                .on_mouse_leave(move |_: &nsh_core::MouseEvent| drag_hovered.set(false))
                .claim_touches(TouchClaim::on_start().ask(move |request: &ClaimRequest| {
                    request.claim();
                }))
                .keep_touches(true)
                .on_responder_grant(handle_start)
                .on_responder_move(handle_move)
                .on_responder_release(handle_end.clone())
                .on_responder_terminate(handle_end)
                .child(
                    view()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(px(2.0))
                        .bg(primary())
                        // 不吃命中：触摸落点的 target 恒为把手自身，
                        // `e.location` 才是把手内坐标，列线位置才好换算。
                        .pointer_events(PointerEvents::None)
                        .opacity(move || {
                            if is_hovered.get() || is_dragging.get() {
                                1.0_f32
                            } else {
                                0.0_f32
                            }
                        }),
                );

            view()
                .w_full()
                .h(px(36.0))
                .flex_row()
                .items_center()
                .relative()
                .map(move |v| match align {
                    DataTableAlign::Start => v.justify_start(),
                    DataTableAlign::Center => v.justify_center(),
                    DataTableAlign::End => v.justify_end(),
                })
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .px(px(12.0))
                        .cursor_pointer()
                        .on_press(sort_toggle)
                        .child(
                            text(title)
                                .text_size(px(13.0))
                                .line_height(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(muted_fg()),
                        )
                        .child(sort_indicator),
                )
                .child(handle_view)
                .into_element()
        }
    };

    // 不设 `render_cell`：它是全局开关，一设整表体就走逐格 dynamic +
    // Renderer 的自绘路径（nsh-base data_table/view.rs），列窗口每平移
    // 一次可见格全部重建——横向滚动卡。全文本格走原生 `text(cell_text)` 绑定。
    let dt = data_table()
        .flex_1()
        .accessibility_label(Some(SharedString::from("元数据表")))
        .classes(classes)
        .columns(defs)
        .row_count(move || Some(displayed.with(|v| v.len() as u32)))
        .data_start(move || {
            let total = displayed.with(|v| v.len());
            visible_window.with(|(s, _)| (*s).min(total) as u32)
        })
        .data(move || data.get())
        .row_height(px(32.0))
        .header_height(Some(px(36.0)))
        .font_size(px(13.0))
        .stripe(false)
        .resizable_columns(false)
        .render_header_cell(render_head)
        .movable_columns(false)
        .select_on_modifier_press(false)
        .selection(move || Some(DataTableSelection::default()))
        .row_classes(row_classes)
        .sort(move || sort.get())
        .on_visible_rows_change(move |range: &nsh_base::DataTableRange| {
            let r_start = range.start as usize;
            let r_end = range.end as usize;
            let current = visible_window.get_untracked();
            if r_start < current.0 + 15 || r_end + 15 > current.1 {
                let start = r_start.saturating_sub(60);
                let end = r_end + 60;
                visible_window.set((start, end));
            }
        })
        .empty(
            view().flex_1().items_center().justify_center().child(
                text("没有匹配的曲目")
                    .text_size(px(13.0))
                    .line_height(px(13.0))
                    .text_color(muted_fg()),
            ),
        )
        .on_row_press(move |p: &DataTablePress| {
            let list = displayed.get_untracked();
            let Some(path) = list.get(p.row as usize).map(|t| t.path.clone()) else {
                return;
            };
            if p.shift {
                // Shift：锚点到本行的范围（当前显示序，替换整个集合）。
                let from = anchor
                    .get_untracked()
                    .and_then(|a| list.iter().position(|t| t.path == a))
                    .unwrap_or(p.row as usize);
                let (lo, hi) = (from.min(p.row as usize), from.max(p.row as usize));
                selected.set(list[lo..=hi].iter().map(|t| t.path.clone()).collect());
            } else if p.platform || p.control {
                // 平台修饰键（macOS ⌘ / Windows Ctrl）：切换单个。
                selected.update(|set| {
                    if !set.remove(&path) {
                        set.insert(path.clone());
                    }
                });
                anchor.set(Some(path));
            } else if selected.with(|set| set.contains(&path)) {
                // 无修饰按一个已选中的行：只取消它（多选里点掉单个），
                // 是最后一条选中时单选面板随 `when` 一起收掉。
                selected.update(|set| {
                    set.remove(&path);
                });
                if anchor.with(|a| a.as_deref() == Some(path.as_str())) {
                    // 锚点跟着已选集走，剩下的任意一条续当范围起点。
                    anchor.set(selected.with(|set| set.last().cloned()));
                }
            } else {
                selected.set(BTreeSet::from([path.clone()]));
                anchor.set(Some(path));
            }
        })
        .on_sort_change(move |next: &Option<DataTableSort>| sort.set(next.clone()));

    view()
        .relative()
        .flex_1()
        .min_w(px(0.0))
        .overflow_hidden()
        .child(dt)
        .child(when(
            move || guide_line_x.get().is_some(),
            move || {
                view()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(move || px(guide_line_x.get().unwrap_or(0.0)))
                    .w(px(2.0))
                    .bg(primary())
                    .z_index(50)
                    .pointer_events(PointerEvents::None)
            },
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 排序：数值列按数字（增益剥 ` dB`），文本列不分大小写，空值恒
    /// 排最后（与方向无关）。
    #[test]
    fn compare_cells_orders_values() {
        // 数值列：数字序（不是字典序——"9" < "12"）。
        assert_eq!(
            compare_cells("track_number", "9", "12", true),
            Ordering::Less
        );
        assert_eq!(
            compare_cells("track_number", "9", "12", false),
            Ordering::Greater
        );
        // 增益剥 " dB"：-6.5 < -3。
        assert_eq!(
            compare_cells("track_replay_gain", "-6.50 dB", "-3.00 dB", true),
            Ordering::Less
        );
        // 非数字的数值回落文本序。
        assert_eq!(compare_cells("year", "abc", "abd", true), Ordering::Less);
        // 文本列不分大小写。
        assert_eq!(
            compare_cells("title", "Beta", "alpha", true),
            Ordering::Greater
        );
        assert_eq!(
            compare_cells("title", "Beta", "alpha", false),
            Ordering::Less
        );
        // 空值恒排最后（两个方向都是）。
        assert_eq!(compare_cells("title", "", "a", true), Ordering::Greater);
        assert_eq!(compare_cells("title", "", "a", false), Ordering::Greater);
        assert_eq!(compare_cells("title", "a", "", false), Ordering::Less);
        assert_eq!(compare_cells("title", "", "", true), Ordering::Equal);
        // 评分星数：5 > 4 > 空。
        assert_eq!(
            compare_cells("rating", "★★★★★", "★★★★", true),
            Ordering::Greater
        );
    }

    #[test]
    fn column_width_defaults_and_clamping() {
        let def_w = |k: &str| {
            COLUMNS
                .iter()
                .find(|(col, ..)| *col == k)
                .map(|(.., w, _, _)| *w)
                .unwrap_or(120.0)
        };
        assert_eq!(def_w("file_name"), 200.0);
        assert_eq!(def_w("title"), 180.0);
        assert_eq!(def_w("artist"), 150.0);

        // 列宽拖拽计算：最小宽度为 50pt
        let cur_w = def_w("title");
        let min_w = 50.0f32;
        let delta = -200.0f32;
        let final_w = (cur_w + delta).max(min_w);
        assert_eq!(final_w, 50.0);

        let delta_expand = 50.0f32;
        let final_expand = (cur_w + delta_expand).max(min_w);
        assert_eq!(final_expand, 230.0);
    }
}
