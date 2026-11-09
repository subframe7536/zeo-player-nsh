use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// 排序：数值列按数字（增益剥 ` dB`），文本列不分大小写，空值恒
    /// 排最后（与方向无关）。
    #[test]
    pub(crate) fn compare_cells_orders_values() {
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
    pub(crate) fn column_width_defaults_and_clamping() {
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
