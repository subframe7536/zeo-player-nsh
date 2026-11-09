use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub(crate) fn blur_spares_the_neighbours_and_grows_after() {
        assert_eq!(lyric_blur(true, 0.0), 0.0);
        assert_eq!(lyric_blur(true, 1.0), 0.0, "前后一句不模糊");
        let steps: Vec<f32> = (2..=10).map(|d| lyric_blur(true, d as f32)).collect();
        assert!(steps[0] > 0.0);
        assert!(
            steps.windows(2).all(|w| w[0] < w[1] || w[1] == 8.0),
            "{steps:?}"
        );
        assert_eq!(lyric_blur(true, 20.0), 8.0);
        assert_eq!(lyric_blur(false, 5.0), 0.0);
    }

    #[test]
    pub(crate) fn translation_sits_between_idle_and_active_originals() {
        assert!(
            original_opacity(true, false) < TRANSLATION_ALPHA,
            "未高亮：原文比译文淡"
        );
        assert!(
            original_opacity(true, true) > TRANSLATION_ALPHA,
            "高亮：原文比译文亮"
        );
    }

    #[test]
    pub(crate) fn edges_fade_out() {
        assert_eq!(edge_fade(0.0, 1000.0), 0.0);
        assert_eq!(edge_fade(1000.0, 1000.0), 0.0);
        assert_eq!(edge_fade(ANCHOR * 1000.0, 1000.0), 1.0, "高亮行不淡出");
        assert!((edge_fade(90.0, 1000.0) - 0.5).abs() < 1e-4);
        assert!((edge_fade(860.0, 1000.0) - 0.5).abs() < 1e-4);
    }
}
