use super::*;

/// 一行的标签值（视图自有）。歌词只留预览（第一条非空行、剥 LRC
/// 标记、截 40 字），注释截 120 字——完整值由单选面板按需现读；
/// `stamp` 是读取时的 `TrackEntry.stamp`，文件变了自动重读。
#[derive(PartialEq)]
pub(crate) struct MetaRow {
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
pub(crate) enum CoverState {
    Embedded,
    File,
    None,
}

/// 后台线程读一行（读不出返回 `None`，单元格回落 `TrackEntry` 值）。
#[allow(dead_code)]
pub(crate) fn build_row(path: &Path, stamp: u64, has_art: bool) -> Option<MetaRow> {
    let (mut fields, tag_type) = zeo_metadata::write::read_row(path).ok()?;
    let embedded = fields
        .get(&TagField::Lyrics)
        .is_some_and(|s| !s.trim().is_empty());
    // 与 engine::Meta::external_lrc 同一组候选（纯文件检查，线程安全）。
    let has_lyrics =
        embedded || zeo_engine::Meta::external_lrc(path.to_string_lossy().into_owned()).is_some();
    let preview = fields
        .get(&TagField::Lyrics)
        .map(|s| lyrics_preview(s))
        .unwrap_or_default();
    fields.insert(TagField::Lyrics, preview);
    if let Some(comment) = fields.get_mut(&TagField::Comment) {
        *comment = comment.chars().take(120).collect();
    }
    let cover = if zeo_metadata::covers::find_sidecar(path).is_some() {
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
pub(crate) fn lyrics_preview(raw: &str) -> String {
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
pub(crate) fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
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
