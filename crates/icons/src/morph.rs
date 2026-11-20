//! 播放 / 暂停图标的共享字形、静态 SVG 与形变动画：界面按钮（[`play_pause`]
//! 画布动画）和托盘位图（`zeo-shell`，经 `nsh_core::svg` 光栅化）用同一组
//! 顶点 + 同款描边，图形完全一致。

use nsh_core::prelude::*;

/// 形变时长与缓动（easeOutQuart 的近似）。
const MORPH_MS: f32 = 320.0;
const MORPH_EASE: Easing = Easing::Bezier(0.25, 1.0, 0.5, 1.0);

/// 播放 / 暂停图标之间的形变：两块四边形，播放时拼成一个三角形（左块
/// 是三角形的左半，右块是右半，接缝两侧同色），暂停时各自变成一根竖条。
/// 填充加 [`MORPH_STROKE`] 宽的圆角描边，圆角半径是描边宽的一半。顶点
/// 坐标在 24 × 24 格里，是参考图形（播放三角 (7, 4.3)–(18.7, 12)–(7, 19.7)、
/// 暂停竖条 6–9 / 15–18 × 6–18，描边 2）向内收了半个多出的描边宽：
/// 三角形绕内心缩小，竖条四边各收 0.75，加粗描边后外轮廓与参考相同。
pub const PLAY_SHAPES: [[(f32, f32); 4]; 2] = [
    [(7.75, 5.69), (12.91, 9.09), (12.91, 14.91), (7.75, 18.31)],
    [(12.91, 9.09), (17.34, 12.0), (17.34, 12.0), (12.91, 14.91)],
];
pub const PAUSE_SHAPES: [[(f32, f32); 4]; 2] = [
    [(6.75, 6.75), (8.25, 6.75), (8.25, 17.25), (6.75, 17.25)],
    [(15.75, 6.75), (17.25, 6.75), (17.25, 17.25), (15.75, 17.25)],
];
/// 描边宽（24 格里）：圆角半径为它的一半。
pub const MORPH_STROKE: f32 = 3.5;

/// `play_pause` 的静态 SVG 版：要位图的场景（托盘图标）拿去
/// `nsh_core::svg::rasterize` 光栅化，图形与按钮完全一致——同一组顶点、
/// 同样的填充 + `MORPH_STROKE` 圆角描边。`tile` 为真时在字形下垫一层和
/// 应用图标同款的浅绿渐变圆角方块（22% 圆角 ≈ 其 squircle；Windows 托盘
/// 「外浅内深」需要底，底砖不随 `scale` 缩）。`dy` 把字形在 24 格里整体
/// 竖移（正向下）：macOS 菜单栏的文字光学中心比格子几何中心高约 1 单位，
/// 图标要跟着抬。`scale` 把字形绕格子中心 (12,12) 缩放（macOS 菜单栏图标
/// 用 0.8 让字形比满格小一号，Windows 用 2/3）。
pub fn play_pause_svg(pause: bool, tile: bool, dy: f32, scale: f32) -> String {
    let mut d = String::new();
    for [a, b, c, e] in if pause { PAUSE_SHAPES } else { PLAY_SHAPES } {
        d += &format!(
            "M{} {}L{} {}L{} {}L{} {}Z",
            a.0, a.1, b.0, b.1, c.0, c.1, e.0, e.1
        );
    }
    let tile = tile
        .then_some(
            "<defs><linearGradient id=\"g\" x1=\"0\" y1=\"0\" x2=\"24\" y2=\"24\" \
             gradientUnits=\"userSpaceOnUse\"><stop stop-color=\"#f0ffd1\"/>\
             <stop offset=\".5\" stop-color=\"#deffe5\"/><stop offset=\"1\" stop-color=\"#dafff6\"/>\
             </linearGradient></defs><rect width=\"24\" height=\"24\" rx=\"5.3\" fill=\"url(#g)\"/>",
        )
        .unwrap_or_default();
    // 字形包一层 transform：translate 把缩放中心 (12,12) 对回原点再附加
    // `dy` 竖移；p' = t + scale·p，中心落在 (12, 12+dy)。
    let tx = 12.0 * (1.0 - scale);
    let ty = tx + dy;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" viewBox=\"0 0 24 24\">\
         {tile}<g transform=\"translate({tx},{ty}) scale({scale})\">\
         <path d=\"{d}\" fill=\"#246449\" stroke=\"#246449\" \
         stroke-width=\"{MORPH_STROKE}\" stroke-linejoin=\"round\"/></g></svg>"
    )
}

/// 播放 / 暂停键的图标：`playing` 为真时显示暂停，切换时顶点逐个移动
/// 过去。顶点绑定在动画值上，由平台在绘制时读取，形变不重跑界面代码。
/// 系统开了「减弱动态效果」时直接切换。
pub fn play_pause(
    size: f32,
    playing: impl Fn() -> bool + 'static,
    tint: impl Fn() -> AdaptiveColor + 'static,
) -> CanvasView {
    let target = |on: bool| if on { 1.0 } else { 0.0 };
    let morph = animated_value(target(untracked(&playing)));
    effect(move || {
        let to = target(playing());
        if untracked(reduce_motion) {
            morph.set(to);
        } else {
            let _ = morph.timing(to, MORPH_MS, MORPH_EASE);
        }
    });
    let lerp = move |a: f32, b: f32| morph.interpolate(&[0.0, 1.0], &[a, b]);
    let points: [[(AnimatedNode, AnimatedNode); 4]; 2] = std::array::from_fn(|s| {
        std::array::from_fn(|i| {
            let (play, pause) = (PLAY_SHAPES[s][i], PAUSE_SHAPES[s][i]);
            (lerp(play.0, pause.0), lerp(play.1, pause.1))
        })
    });
    let scale = size / 24.0;
    canvas(move || {
        let c = color(tint());
        let mut p = Painter::new()
            .scale(scale, scale)
            .fill_color(c)
            .stroke_color(c)
            .line_width(MORPH_STROKE)
            .line_join(nsh_core::canvas::LineJoin::Round)
            .begin_path();
        for [a, b, c, d] in &points {
            p = p
                .move_to(&a.0, &a.1)
                .line_to(&b.0, &b.1)
                .line_to(&c.0, &c.1)
                .line_to(&d.0, &d.1)
                .close_path();
        }
        p.fill().stroke()
    })
    .size(px(size))
    .pointer_events(PointerEvents::None)
}
