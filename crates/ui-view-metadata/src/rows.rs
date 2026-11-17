use super::*;

/// 一行的标签值（视图自有）。歌词只留预览（第一条非空行、剥 LRC
/// 标记、截 40 字），注释截 120 字——完整值由单选面板按需现读；
/// `stamp` 是读取时的 `TrackEntry.stamp`，文件变了自动重读。
#[derive(PartialEq)]
pub(crate) struct MetaRow {
    pub fields: HashMap<TagField, String>,
    /// 有歌词：内嵌非空或同名 `.lrc` / `.LRC` 存在。
    pub has_lyrics: bool,
    /// 读取来源的标签类型显示名（文件无标签为 `None`）。
    pub tag_type: Option<&'static str>,
    pub stamp: u64,
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
