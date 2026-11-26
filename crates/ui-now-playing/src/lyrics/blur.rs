//! 歌词区的渐进模糊与上下边缘淡出：`shader_effect` 包住歌词滚动区，WGSL
//! （`blur.wgsl`）逐像素读子视图画出的内容，按到清晰带的距离给模糊半径。
//!
//! 不用每行 `filter: blur()`：macOS 后端不画 view 的 blur（只有图片），
//! 而且逐行只能按行算、做不出行内渐变；也不用 `backdropFilter` +
//! `maskImage`：Windows 不画遮罩。shader 两端都走 GPU。没有 GPU 的机器上
//! effect 原样画子视图（不模糊、不淡出）。

use std::sync::LazyLock;

use nsh_core::shader::Shader;

use super::*;

/// 一行歌词的高度：上下 14 内边距 + 原文行高（`lyrics_scroller` 的行）。
const LINE_H: f32 = 14.0 * 2.0 + LYRIC_SIZE * 1.3;
/// 清晰带：锚点以上一行（前一句）到以下两行（高亮行 + 后一句）。
const CLEAR_ABOVE: f32 = LINE_H;
const CLEAR_BELOW: f32 = LINE_H * 2.0;
/// 离开清晰带多远（点）模糊到最大：约 5 行。
const RAMP: f32 = 320.0;
/// 最大模糊（σ，点）。
const MAX_BLUR: f32 = 8.0;
/// 顶部淡出区（视口高度的比例），小于 [`ANCHOR`]，清晰带不受影响。
const EDGE_TOP: f32 = 0.18;
/// 底部淡出区。
const EDGE_BOTTOM: f32 = 0.28;

/// 编译一次，各次全屏共用（`Shader` 克隆只加引用计数）。编译失败（构建
/// 没开 nsh-core 的 `shader` 特性）记日志，歌词原样画。
static SHADER: LazyLock<Option<Shader>> = LazyLock::new(|| {
    Shader::compile(include_str!("blur.wgsl"))
        .inspect_err(|e| eprintln!("zeo-player: 歌词模糊 shader 编译失败: {e}"))
        .ok()
});

/// 视口高 `viewport` 时清晰带的上下沿（视口内的 y）：固定在锚点附近，
/// 与高亮行、滚动位置无关。
pub(crate) fn clear_band(viewport: f32) -> (f32, f32) {
    let anchor = viewport * ANCHOR;
    (anchor - CLEAR_ABOVE, anchor + CLEAR_BELOW)
}

/// 把 `content`（歌词滚动区）包进模糊 effect。`viewport`：滚动区高度；
/// `clear`：悬停去模糊的程度（0 按位置模糊，1 全部清晰）；无时间轴的
/// 歌词（`timed` 为假）不模糊，只淡出边缘。
pub(crate) fn blur_effect(
    content: impl IntoElement,
    timed: bool,
    viewport: Signal<f32>,
    clear: Signal<f32>,
) -> ShaderView {
    let effect = shader_effect().flex_1().w_full();
    let Some(shader) = SHADER.clone() else {
        return effect.child(content);
    };
    effect
        .source(shader)
        .uniform("clear_top", move || clear_band(viewport.get()).0)
        .uniform("clear_bottom", move || clear_band(viewport.get()).1)
        .uniform("ramp", RAMP)
        .uniform("max_radius", MAX_BLUR)
        .uniform(
            "amount",
            move || {
                if timed { 1.0 - clear.get() } else { 0.0 }
            },
        )
        .uniform("fade_top", EDGE_TOP)
        .uniform("fade_bottom", EDGE_BOTTOM)
        .child(content)
}
