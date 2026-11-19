use super::*;

use std::collections::HashSet;
use std::path::Path;

/// 效果预览取前几首（按当前显示序）：每次按键都重算，只算样本，
/// 全量校验留给「应用」。
pub(crate) const PREVIEW_N: usize = 3;

/// 模板占位符能取到的标签字段（`rename::Template` 认的那些）。
const TEMPLATE_FIELDS: [TagField; 14] = [
    TagField::Title,
    TagField::Artist,
    TagField::Album,
    TagField::AlbumArtist,
    TagField::Genre,
    TagField::Year,
    TagField::Track,
    TagField::TrackTotal,
    TagField::Disc,
    TagField::DiscTotal,
    TagField::Composer,
    TagField::Conductor,
    TagField::Lyricist,
    TagField::Publisher,
];

/// 预览里的一行（`bad` = 红字，用于模板报错 / 冲突）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreviewLine {
    pub(crate) text: String,
    pub(crate) bad: bool,
}

impl PreviewLine {
    pub(crate) fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bad: false,
        }
    }

    pub(crate) fn bad(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bad: true,
        }
    }
}

/// 一首曲目某字段的当前值：视图行已加载用行值，否则回落索引条目。
pub(crate) fn current_value(
    t: &TrackEntry,
    rows: &HashMap<String, Arc<MetaRow>>,
    field: TagField,
) -> String {
    match rows.get(t.path.as_str()) {
        Some(row) => row.fields.get(&field).cloned().unwrap_or_default(),
        None => entry_field_of(t, field),
    }
}

/// 文件名模板能看到的字段：视图行 / 索引条目的当前值（模板里的
/// `{artist}` 取的是此刻的歌手）。
pub(crate) fn effective_fields(
    t: &TrackEntry,
    rows: &HashMap<String, Arc<MetaRow>>,
) -> HashMap<TagField, String> {
    TEMPLATE_FIELDS
        .iter()
        .map(|f| (*f, current_value(t, rows, *f)))
        .collect()
}

/// 重命名模板的报错（空模板 = 不改名，不算错）。
pub(crate) fn template_error(tpl: &str) -> Option<String> {
    if tpl.trim().is_empty() {
        return None;
    }
    rename::Template::parse(tpl).err()
}

/// 按顺序给 `tracks` 出改名计划：模板按当前标签值渲染。预览传样本、
/// 应用传全部。
pub(crate) fn plan_with<'a>(
    template: &rename::Template,
    tracks: impl Iterator<Item = &'a TrackEntry>,
    rows: &HashMap<String, Arc<MetaRow>>,
) -> Vec<rename::Planned> {
    let inputs: Vec<(String, HashMap<TagField, String>)> = tracks
        .map(|t| (t.path.clone(), effective_fields(t, rows)))
        .collect();
    rename::plan(&inputs, template)
}

/// 全部所选的改名计划（弹窗「重命名」用）：任何一条不可用就整批拒绝
/// （Err 带条数与第一条原因），先于任何写盘；通过的只留真要改名的。
pub(crate) fn plan_renames(
    tpl: &str,
    order: &[String],
    entries: &HashMap<String, TrackEntry>,
    rows: &HashMap<String, Arc<MetaRow>>,
) -> Result<Vec<rename::Planned>, String> {
    if tpl.trim().is_empty() {
        return Ok(Vec::new());
    }
    let template = rename::Template::parse(tpl)?;
    let planned = plan_with(&template, order.iter().filter_map(|p| entries.get(p)), rows);
    let bad: Vec<(&rename::Planned, &String)> = planned
        .iter()
        .filter_map(|p| match &p.status {
            Status::Conflict(m) | Status::Invalid(m) => Some((p, m)),
            _ => None,
        })
        .collect();
    if let Some((first, why)) = bad.first() {
        return Err(format!(
            "{} 个文件名无法使用，未应用：{}（{why}）",
            bad.len(),
            file_name(&first.src)
        ));
    }
    Ok(planned
        .into_iter()
        .filter(|p| p.status == Status::Ok)
        .collect())
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 乱码修复预览要样本的完整歌词 / 注释（视图行里只有截断预览，拿预览去
/// 修会把原文截掉——所以从文件读）。`路径 → 字段 → 全文`。
pub(crate) type SampleTexts = HashMap<String, HashMap<TagField, String>>;

/// 给预览行数追加「… 共 N 首」（只看了样本时的说明）。
pub(crate) fn more_note(
    mut lines: Vec<PreviewLine>,
    total: usize,
    seen: usize,
) -> Vec<PreviewLine> {
    if !lines.is_empty() && total > seen {
        lines.push(PreviewLine::ok(format!("… 共 {total} 首")));
    }
    lines
}

/// 每首取 `fields` 里第一个会变的字段，`字段：旧 → 新`；都不变就写「无变化」。
fn first_change_lines(
    sample: &[TrackEntry],
    fields: &[TagField],
    change: impl Fn(&TrackEntry, TagField) -> Option<(String, String)>,
) -> Vec<PreviewLine> {
    sample
        .iter()
        .map(|t| {
            let line = fields.iter().find_map(|f| {
                change(t, *f).map(|(old, new)| format!("{}：{old} → {new}", f.label()))
            });
            PreviewLine::ok(line.unwrap_or_else(|| format!("{}（无变化）", t.file_name)))
        })
        .collect()
}

/// 预览里的一行文字：多行文本（歌词 / 注释）只取第一条非空行、截到 30 字。
fn one_line(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty());
    let line = line.unwrap_or("");
    let short: String = line.chars().take(30).collect();
    if short.len() < line.len() {
        format!("{short}…")
    } else {
        short
    }
}

/// 乱码修复预览：覆盖字段（含歌词 / 注释）里第一个能修复的，`乱码 → 修复后`。
/// 歌词 / 注释的完整文本来自 `texts`（文件读出来的）。
pub(crate) fn encoding_preview(
    sample: &[TrackEntry],
    rows: &HashMap<String, Arc<MetaRow>>,
    texts: &SampleTexts,
) -> Vec<PreviewLine> {
    let fields: Vec<TagField> = ENCODING_FIELDS
        .iter()
        .chain(ENCODING_FILE_FIELDS.iter())
        .copied()
        .collect();
    first_change_lines(sample, &fields, |t, f| {
        let old = if ENCODING_FILE_FIELDS.contains(&f) {
            texts.get(&t.path)?.get(&f)?.clone()
        } else {
            current_value(t, rows, f)
        };
        fix_mojibake(&old).map(|new| (one_line(&old), one_line(&new)))
    })
}

/// 重命名预览：`旧文件名 → 新文件名`，冲突 / 不可用标红并带原因。
pub(crate) fn rename_preview(
    tpl: &str,
    sample: &[TrackEntry],
    rows: &HashMap<String, Arc<MetaRow>>,
) -> Vec<PreviewLine> {
    if tpl.trim().is_empty() {
        return Vec::new();
    }
    let template = match rename::Template::parse(tpl) {
        Ok(t) => t,
        Err(message) => return vec![PreviewLine::bad(message)],
    };
    plan_with(&template, sample.iter(), rows)
        .iter()
        .map(|p| {
            let (old, new) = (file_name(&p.src), file_name(&p.dest));
            match &p.status {
                Status::Ok => PreviewLine::ok(format!("{old} → {new}")),
                Status::Unchanged => PreviewLine::ok(format!("{old}（无变化）")),
                Status::Conflict(why) | Status::Invalid(why) => {
                    PreviewLine::bad(format!("{old} → {new}（{why}）"))
                }
            }
        })
        .collect()
}

/// 预览行的纵向列表（红 / 淡）；空预览不占位（下边距只在有行时才加）。
pub(crate) fn preview_list(lines: impl Fn() -> Vec<PreviewLine> + 'static) -> AnyElement {
    dynamic(move || {
        let lines = lines();
        view()
            .gap(px(3.0))
            .pb(px(if lines.is_empty() { 0.0 } else { 4.0 }))
            .children(lines.into_iter().map(|line| {
                let color = if line.bad {
                    nsh_base::theme().destructive
                } else {
                    muted_fg()
                };
                text(line.text)
                    .number_of_lines(1)
                    .text_size(px(11.0))
                    .line_height(px(14.0))
                    .text_color(color)
                    .into_element()
            }))
            .into_element()
    })
}

/// 所选里取前 [`PREVIEW_N`] 首（显示序）。
pub(crate) fn sample_of(displayed: &[TrackEntry], selected: &HashSet<String>) -> Vec<TrackEntry> {
    displayed
        .iter()
        .filter(|t| selected.contains(&t.path))
        .take(PREVIEW_N)
        .cloned()
        .collect()
}
