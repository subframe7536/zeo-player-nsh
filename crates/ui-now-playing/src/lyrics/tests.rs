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
}
