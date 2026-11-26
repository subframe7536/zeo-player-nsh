//! 歌词区的渐进模糊与上下边缘淡出。
//!
//! - **macOS**：`shader_effect` 包住滚动区会每帧在主线程做 `renderInContext:`
//!   CPU 软光栅化与纹理上传，导致高刷滚动掉帧；NSH macOS 后端原生支持
//!   `mask_linear` 与带遮罩的 `backdrop_blur`（`NshPlainBlur`，纯硬件
//!   `CABackdropLayer` 高斯模糊、无材质染色），全程 CoreAnimation GPU 合成。
//! - **Windows**：不画 `maskImage` 且 `backdropFilter` 局部重绘会留亮块
//!   （见 `upstream.md`），但普通 view 原生支持 `filter: blur()`（Direct2D
//!   `decor::draw_layered`）。因此 Windows 同样绕开 `shader_effect`，在滚动
//!   时仅对视口内可见行按位置更新行级 `st.blur()` 与边缘 `.opacity()`，视口
//!   外行以 `invisible()` 跳过绘制。
//! - **其余平台（如 Linux）**：使用 `shader_effect` + WGSL（`blur.wgsl`）在
//!   GPU 逐像素计算渐进模糊与边缘淡出；没有 GPU 时原样画子视图。

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use std::sync::LazyLock;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use nsh_core::shader::Shader;

use super::*;

/// 一行歌词的高度：上下 14 内边距 + 原文行高（`lyrics_scroller` 的行）。
const LINE_H: f32 = 14.0 * 2.0 + LYRIC_SIZE * 1.3;
/// 清晰带：锚点以上约 1.25 行（前一句）到以下约 2.25 行（高亮行 + 后一句），
/// 给双语或折行留出余量，同时缩小模糊范围。
const CLEAR_ABOVE: f32 = LINE_H * 1.25;
const CLEAR_BELOW: f32 = LINE_H * 2.25;
/// 离开清晰带多远（点）模糊到最大：约 4 行。
const RAMP: f32 = 260.0;
/// 最大模糊（σ，点）。
const MAX_BLUR: f32 = 4.0;
/// 顶部淡出区（视口高度的比例），小于 [`ANCHOR`]，清晰带不受影响。
pub(crate) const EDGE_TOP: f32 = 0.18;
/// 底部淡出区。
pub(crate) const EDGE_BOTTOM: f32 = 0.28;

/// 编译一次，各次全屏共用（`Shader` 克隆只加引用计数）。编译失败（构建
/// 没开 nsh-core 的 `shader` 特性）记日志，歌词原样画。
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

/// 按视口内 y 坐标计算该位置的模糊半径（Windows 原生行级 `st.blur` 用）：
/// 清晰带内为 0，离开后在 `RAMP` 内线性增至 `MAX_BLUR`，量化到 0.5 pt 步进
/// 以减少滚动期间的重复离屏重绘。
#[cfg(any(target_os = "windows", test))]
pub(crate) fn blur_at(y: f32, viewport: f32) -> f32 {
    let (top, bottom) = clear_band(viewport);
    let d = (top - y).max(y - bottom).max(0.0);
    let raw = MAX_BLUR * (d / RAMP).clamp(0.0, 1.0);
    let q = (raw * 2.0).round() * 0.5;
    if q < 0.5 { 0.0 } else { q }
}

/// 按视口内 y 坐标计算上下边缘淡出不透明度（Windows 原生行级 `.opacity` 用）：
/// 进入顶部 / 底部淡出区后线性降到 0，量化到 0.02 步进减少微弱抖动写入。
#[cfg(any(target_os = "windows", test))]
pub(crate) fn edge_fade(y: f32, viewport: f32) -> f32 {
    if viewport <= 0.0 {
        return 1.0;
    }
    let top = y / (viewport * EDGE_TOP).max(1.0);
    let bottom = (viewport - y) / (viewport * EDGE_BOTTOM).max(1.0);
    let raw = top.min(bottom).clamp(0.0, 1.0);
    (raw * 50.0).round() / 50.0
}

/// 边缘淡出的线性渐变遮罩（用于 macOS 原生 `CALayer.mask` 及其他支持遮罩的平台）。
#[cfg(not(target_os = "windows"))]
pub(crate) fn edge_fade_view(content: impl IntoElement) -> View {
    view()
        .flex_1()
        .w_full()
        .mask_linear(
            deg(180.0),
            [
                (transparent_black(), percentage(0.0)),
                (black(), percentage(EDGE_TOP)),
                (black(), percentage(1.0 - EDGE_BOTTOM)),
                (transparent_black(), percentage(1.0)),
            ],
        )
        .child(content)
}

/// 计算单层 `backdrop_blur` 覆盖层的竖向遮罩色标：清晰带内完全透明，
/// 离开清晰带 `[start_offset, end_offset]` 区间内从透明过渡到不透明。
#[cfg(any(target_os = "macos", test))]
pub(crate) fn blur_mask_stops(h: f32, start_offset: f32, end_offset: f32) -> Vec<ColorStop> {
    if h <= 1.0 {
        // 保持非空色标，确保 macOS 始终走 `PlainBlur` 而非系统着色材质。
        return vec![
            (transparent_black(), percentage(0.0)).into(),
            (transparent_black(), percentage(1.0)).into(),
        ];
    }
    let (clear_top, clear_bottom) = clear_band(h);
    let top_full = (clear_top - end_offset).clamp(0.0, h) / h;
    let top_zero = (clear_top - start_offset).clamp(0.0, h) / h;
    let bot_zero = (clear_bottom + start_offset).clamp(0.0, h) / h;
    let bot_full = (clear_bottom + end_offset).clamp(0.0, h) / h;
    vec![
        (black(), percentage(0.0)).into(),
        (black(), percentage(top_full)).into(),
        (transparent_black(), percentage(top_zero)).into(),
        (transparent_black(), percentage(bot_zero)).into(),
        (black(), percentage(bot_full)).into(),
        (black(), percentage(1.0)).into(),
    ]
}

/// macOS 原生 GPU 渐进模糊：底层是带边缘淡出的原生滚动区，上层叠两层
/// 带渐进遮罩的 `backdrop_blur`（`NshPlainBlur`），由 `amount` 驱动整体
/// 不透明度实现悬停去模糊过渡。
#[cfg(target_os = "macos")]
pub(crate) fn native_blur_view(
    content: impl IntoElement,
    viewport: Signal<f32>,
    amount: &AnimatedValue,
) -> View {
    let layer = |radius: f32, start: f32, end: f32| {
        view()
            .absolute()
            .inset_0()
            .pointer_events(PointerEvents::None)
            .backdrop_blur(px(radius))
            .mask_linear(deg(180.0), move || {
                blur_mask_stops(viewport.get(), start, end)
            })
    };
    view()
        .flex_1()
        .w_full()
        .child(edge_fade_view(content))
        .child(
            view()
                .absolute()
                .inset_0()
                .pointer_events(PointerEvents::None)
                .bind_opacity(amount)
                .child(layer(MAX_BLUR * 0.5, 0.0, RAMP * 0.55))
                .child(layer(MAX_BLUR, RAMP * 0.3, RAMP)),
        )
}

/// 把 `content`（歌词滚动区）包进模糊 effect（macOS / Windows 以外的平台使用）。
/// `amount` 为平台驱动的动画值（1 按位置模糊，0 悬停全部清晰）。
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn blur_effect(content: impl IntoElement, amount: &AnimatedValue) -> ShaderView {
    let effect = shader_effect().flex_1().w_full();
    let Some(shader) = SHADER.clone() else {
        return effect.child(content);
    };
    effect
        .source(shader)
        .uniform("anchor", ANCHOR)
        .uniform("clear_above", CLEAR_ABOVE)
        .uniform("clear_below", CLEAR_BELOW)
        .uniform("ramp", RAMP)
        .uniform("max_radius", MAX_BLUR)
        .uniform("amount", amount)
        .uniform("fade_top", EDGE_TOP)
        .uniform("fade_bottom", EDGE_BOTTOM)
        .child(content)
}
