use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub(crate) fn blur_shader_compiles() {
        let source = include_str!("blur.wgsl");
        if let Err(e) = nsh_core::shader::Shader::compile(source) {
            panic!("歌词模糊 shader 编译失败: {e}");
        }
    }

    #[test]
    pub(crate) fn clear_band_holds_the_anchor_line_and_its_neighbours() {
        let (top, bottom) = clear_band(1000.0);
        let anchor = 1000.0 * ANCHOR;
        // 跟随时高亮行的顶在锚点：前一句、高亮行、后一句都在带内。
        assert!(
            top <= anchor - 60.0 && bottom >= anchor + 120.0,
            "{top}..{bottom}"
        );
        // 与视口高度成比例移动，宽度不变。
        let (top2, bottom2) = clear_band(500.0);
        assert!(((bottom - top) - (bottom2 - top2)).abs() < 1e-3);
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
    pub(crate) fn blur_mask_stops_are_non_empty_and_monotonic() {
        // 视口尚未布局时保持非空透明色标，避免 macOS 退回系统着色材质。
        let zero = blur_mask_stops(0.0, 0.0, 140.0);
        assert_eq!(zero.len(), 2);
        let stops = blur_mask_stops(800.0, 0.0, 140.0);
        assert_eq!(stops.len(), 6);
        let pcts: Vec<f32> = stops
            .iter()
            .map(|s| match s.position {
                Some(DefiniteLength::Fraction(p)) => p.0,
                other => panic!("expected fraction stop, got {other:?}"),
            })
            .collect();
        for w in pcts.windows(2) {
            assert!(w[0] <= w[1] + 1e-6, "stops must be monotonic: {pcts:?}");
        }
    }

    #[test]
    pub(crate) fn windows_blur_and_edge_fade_match_clear_band() {
        let vp = 800.0;
        let (top, bottom) = clear_band(vp);
        // 清晰带内模糊为 0、边缘不透明度为 1。
        assert_eq!(blur_at((top + bottom) * 0.5, vp), 0.0);
        assert_eq!(edge_fade((top + bottom) * 0.5, vp), 1.0);
        // 远离清晰带后模糊单调增大，边缘淡出趋近 0。
        assert!(blur_at(bottom + 300.0, vp) > 0.0);
        assert_eq!(edge_fade(0.0, vp), 0.0);
        assert_eq!(edge_fade(vp, vp), 0.0);
    }
}
