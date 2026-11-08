//! 表格：nsh-base `DataTable` 的列配置（首列选择标记 + 固定文件名 + 22
//! 个标签列，对应 spec §一）、单元格文本 / 排序比较与行选择交互。

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use nsh_base::{
    DataTableAlign, DataTableColumn, DataTablePress, DataTableRows, DataTableSelection,
    DataTableSort, data_table,
};
use nsh_core::prelude::*;

use super::super::super::theme::*;
use super::{CoverState, MetaRow};
use crate::config::TrackEntry;
use crate::metadata::write::TagField;

/// 选择标记列的 key（固定第 0 列，不参与选择 / 移动 / 缩放 / 排序）。
const SELECT: &str = "select";

/// 标签列：key、标题、宽（pt）、对齐、可排序（spec §一）。
const COLUMNS: &[(&str, &str, f32, DataTableAlign, bool)] = &[
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
pub(super) fn cell_text(key: &str, track: &TrackEntry, row: Option<&MetaRow>) -> String {
    match row {
        Some(row) => loaded_cell(key, track, row),
        None => fallback_cell(key, track),
    }
}

fn field(row: &MetaRow, f: TagField) -> String {
    row.fields.get(&f).cloned().unwrap_or_default()
}

fn loaded_cell(key: &str, track: &TrackEntry, row: &MetaRow) -> String {
    let gain = |f: TagField| {
        let v = field(row, f);
        if v.is_empty() { v } else { format!("{v} dB") }
    };
    match key {
        "file_name" => track.file_name.clone(),
        "title" => field(row, TagField::Title),
        "artist" => field(row, TagField::Artist),
        "album" => field(row, TagField::Album),
        "album_artist" => field(row, TagField::AlbumArtist),
        "genre" => field(row, TagField::Genre),
        "year" => field(row, TagField::Year),
        "track_number" => field(row, TagField::Track),
        "track_total" => field(row, TagField::TrackTotal),
        "disc_number" => field(row, TagField::Disc),
        "discs_total" => field(row, TagField::DiscTotal),
        "composer" => field(row, TagField::Composer),
        "conductor" => field(row, TagField::Conductor),
        "lyricist" => field(row, TagField::Lyricist),
        "publisher" => field(row, TagField::Publisher),
        "comment" => field(row, TagField::Comment),
        "lyrics" => field(row, TagField::Lyrics),
        "copyright" => field(row, TagField::Copyright),
        "rating" => field(row, TagField::Rating)
            .parse::<usize>()
            .map(|n| "★".repeat(n))
            .unwrap_or_default(),
        "pictures" => match row.cover {
            CoverState::Embedded => "内嵌",
            CoverState::File => "文件",
            CoverState::None => "无",
        }
        .to_owned(),
        "track_replay_gain" => gain(TagField::TrackGain),
        "track_replay_peak" => field(row, TagField::TrackPeak),
        "album_replay_gain" => gain(TagField::AlbumGain),
        "album_replay_peak" => field(row, TagField::AlbumPeak),
        _ => String::new(),
    }
}

/// 行未加载时回落的索引值（`TrackEntry` / `MetaInfo` 里已有的字段）。
fn fallback_cell(key: &str, t: &TrackEntry) -> String {
    let num = |v: Option<u32>| v.map(|n| n.to_string()).unwrap_or_default();
    let gain = |v: Option<f32>| v.map(|v| format!("{v:.2} dB")).unwrap_or_default();
    let peak = |v: Option<f32>| v.map(|v| format!("{v:.6}")).unwrap_or_default();
    match key {
        "file_name" => t.file_name.clone(),
        "title" => t.title.clone(),
        "artist" => t.artist.clone(),
        "album" => t.album.clone(),
        "album_artist" => t.album_artist.clone(),
        "genre" => t.genres.join("; "),
        "year" => num(t.year),
        "track_number" => num(t.track),
        "track_total" => num(t.track_total),
        "disc_number" => num(t.disk),
        "discs_total" => num(t.disk_total),
        "composer" => t.composers.join("; "),
        "track_replay_gain" => gain(t.replaygain.track_gain),
        "track_replay_peak" => peak(t.replaygain.track_peak),
        "album_replay_gain" => gain(t.replaygain.album_gain),
        "album_replay_peak" => peak(t.replaygain.album_peak),
        _ => String::new(),
    }
}

/// 单元格比较（`displayed` 排序用，纯函数）：空值恒排最后（与方向
/// 无关）；数值列按数字（增益剥 ` dB`，非数字回落文本序）；文本列
/// 不分大小写。
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
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// `displayed` memo 的就地排序（decorate-sort：比较器里不做重复取值 /
/// 分配，每行算一次键再排）。
pub(super) fn sort_entries(
    list: &mut [TrackEntry],
    key: &str,
    ascending: bool,
    rows: &HashMap<String, Arc<MetaRow>>,
) {
    let mut keyed: Vec<(String, TrackEntry)> = list
        .iter()
        .map(|t| {
            (
                cell_text(key, t, rows.get(t.path.as_str()).map(|r| r.as_ref())),
                t.clone(),
            )
        })
        .collect();
    keyed.sort_by(|(a, _), (b, _)| compare_cells(key, a, b, ascending));
    let sorted: Vec<TrackEntry> = keyed.into_iter().map(|(_, t)| t).collect();
    list.clone_from_slice(&sorted);
}

/// 标签表格：工具栏的全选 / 半选与行内「✓」字形都由 `selected` 集合
/// 驱动（表格自身 `selection` 恒空，不画它竞争的单选高亮）；行高亮
/// 走 `row_classes`。排序由 app 侧做（`sort` 信号 + `displayed` 重排）。
pub(super) fn table(
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    anchor: Signal<Option<String>>,
    sort: Signal<Option<DataTableSort>>,
) -> impl IntoElement {
    let mut defs = Vec::with_capacity(COLUMNS.len() + 1);
    let mut select_col = DataTableColumn::default();
    select_col.key = SharedString::from(SELECT);
    select_col.title = Some(SharedString::default());
    select_col.width = Some(px(30.0));
    select_col.align = DataTableAlign::Center;
    select_col.fixed = true;
    select_col.resizable = Some(false);
    select_col.movable = Some(false);
    select_col.selectable = Some(false);
    // 选中行画一个主色「✓」文本字形（cell_class 走主题 token，与行高亮
    // 的 bg-primary/8 同色）；未选中的单元格留空——不画淡色空心框，免得
    // 悬停 / 选中行底从透明盒里透出来显得和别行不一样（就是用户问的
    // 「为什么这里颜色不一样」）。
    select_col.cell_class = Some("text-primary".into());
    defs.push(select_col);
    for (key, title, width, align, sortable) in COLUMNS {
        let mut c = DataTableColumn::default();
        c.key = SharedString::from(*key);
        c.title = Some(SharedString::from(*title));
        c.width = Some(px(*width));
        c.align = *align;
        c.sortable = *sortable;
        c.fixed = *key == "file_name";
        defs.push(c);
    }

    // 标签列数据与选择列拆两截：勾选切换只重写一列字形，不为它把 24 个
    // 标签列全表重算一遍。`base` 只订 `displayed` / `rows`（选择变化走
    // memo 缓存命中）；`Cell::Text` 是 `Arc<str>`，clone 全表只是按列
    // 增引用，便宜。
    let base = computed({
        let displayed = displayed.clone();
        move || -> DataTableRows {
            let list = displayed.get();
            let map = rows.get();
            let mut data = DataTableRows::new(list.len());
            for &(key, ..) in COLUMNS {
                data = data.texts(
                    key,
                    list.iter()
                        .map(|t| cell_text(key, t, map.get(t.path.as_str()).map(|r| r.as_ref()))),
                );
            }
            data
        }
    });
    let data = move || -> DataTableRows {
        let set = selected.get();
        displayed.with(|list| {
            base.get().texts(
                SELECT,
                list.iter()
                    .map(|t| if set.contains(&t.path) { "✓" } else { "" }),
            )
        })
    };

    let row_classes = move || -> Vec<SharedString> {
        let set = selected.get();
        displayed.with(|v| {
            v.iter()
                .map(|t| {
                    if set.contains(&t.path) {
                        // 与侧栏悬停同一个 8% 主色（`primary().opacity(0.08)`，
                        // sidebar.rs）——选中行跟着主题色走，不再用 accent 灰底。
                        SharedString::from("bg-primary/8")
                    } else {
                        SharedString::default()
                    }
                })
                .collect()
        })
    };

    // 不设 `render_cell`：它是全局开关，一设整表体就走逐格 dynamic +
    // Renderer 的自绘路径（nsh-base data_table/view.rs），列窗口每平移
    // 一次可见格全部重建——横向滚动卡。全文本格走原生 `text(cell_text)`
    // 绑定，复选框列也用数据驱动的「✓」字形，不再自绘。
    data_table()
        .flex_1()
        .accessibility_label(Some(SharedString::from("元数据表")))
        .columns(defs)
        .data(data)
        .row_height(px(32.0))
        .font_size(px(13.0))
        .stripe(false)
        .movable_columns(false)
        .select_on_modifier_press(false)
        .selection(move || Some(DataTableSelection::default()))
        .row_classes(row_classes)
        .sort(move || sort.get())
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
            // 复选框列：文本字形没有自己的按压应答，按列 0 的格同样切换
            // （命中由表格的 hit 层按位置报列号）。
            if p.column == Some(0) {
                selected.update(|set| {
                    if !set.remove(&path) {
                        set.insert(path.clone());
                    }
                });
                anchor.set(Some(path));
                return;
            }
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
        .on_sort_change(move |next: &Option<DataTableSort>| sort.set(next.clone()))
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
}
