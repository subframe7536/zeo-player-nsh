use super::*;

/// 「元数据」页。
#[cfg(test)]
mod tests {
    use super::*;

    /// 歌词预览：剥 LRC 时间戳与标记段，取第一条非空行。
    #[test]
    pub(crate) fn lyrics_preview_strips_lrc_marks() {
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
    pub(crate) fn contains_ignore_case_matches() {
        assert!(contains_ignore_case("Sine.flac", "sine"));
        assert!(contains_ignore_case("Sine.flac", "FLAC"));
        assert!(!contains_ignore_case("Sine.flac", "wav"));
        assert!(contains_ignore_case("测试歌曲.flac", "测试"));
        assert!(contains_ignore_case("Hello World", ""));
    }

    #[test]
    pub(crate) fn column_sets_have_expected_members() {
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
