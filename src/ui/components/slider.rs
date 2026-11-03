//! 拖动输入：`scrub` 裸拖动区；`slider` 统一的可定制滑块（横向细轨 /
//! 竖向 / 双极推子——nsh-base 0.3.0 没有 slider 组件，自己组合，见
//! AGENTS「待上游」）。
//!
//! 手势走 `claim_touches` + responder，挡住外层滚动视图抢拖。拖动中的
//! 位置先写本地 `drag` 信号直接画，不等 `fraction` 跟存储写回环——
//! EQ 推子每动一下都写一次 `s.library`，回环比指针慢一拍会卡顿；抬起
//! 调完 `on_release`（同步落值）再清 `drag`，位置无缝交回 `fraction`。

use std::rc::Rc;

use nsh_core::prelude::*;

/// thumb 的显示时机。
#[derive(Clone, Copy, PartialEq)]
pub enum Thumb {
    /// 悬停或拖动时才显示（细轨滑块）。
    OnHover,
    /// 常显（推子）。
    Always,
}

/// [`slider`] 的外观件：方向、填充形态与各部位尺寸。
#[derive(Clone, Copy)]
pub struct SliderStyle {
    /// 竖向（0 在底部）；默认横向（0 在左）。
    pub vertical: bool,
    /// 填充从中心（0.5）向 thumb 长（双极：正值朝上、负值朝下、0 位
    /// 不填色）；默认从起点长。
    pub bipolar: bool,
    /// thumb 显示时机。
    pub thumb: Thumb,
    /// 轨道粗细（横向是高、竖向是宽）。
    pub track: f32,
    /// thumb 直径。
    pub thumb_d: f32,
    /// 细侧的可点厚度（横向是高、竖向是宽）；`None` 表示两个方向都由
    /// 调用方的 `w`/`h` 定（EQ 推子要整块作拖动区）。
    pub hit: Option<f32>,
}

impl SliderStyle {
    /// 细轨 + 悬停 thumb 的横向滑块（进度条、音量条、偏移条）。
    pub const HORIZONTAL: SliderStyle = SliderStyle {
        vertical: false,
        bipolar: false,
        thumb: Thumb::OnHover,
        track: 4.0,
        thumb_d: 12.0,
        hit: Some(18.0),
    };
    /// 竖向滑块，填充从底部长起（音量弹层）。
    pub const VERTICAL: SliderStyle = SliderStyle {
        vertical: true,
        ..SliderStyle::HORIZONTAL
    };
    /// 双极竖推子（均衡器）；两侧大小都交给调用方。
    pub const FADER: SliderStyle = SliderStyle {
        vertical: true,
        bipolar: true,
        thumb: Thumb::Always,
        hit: None,
        ..SliderStyle::HORIZONTAL
    };
}

/// 拖动取比例（0..1）：按下/移动调 `on_move`，抬起调 `on_release`；
/// `vertical` 沿高度量且 0 在底部。
pub fn scrub(
    area: View,
    vertical: bool,
    on_move: impl Fn(f64) + 'static,
    on_release: impl Fn(f64) + 'static,
) -> View {
    let span = signal(1.0_f32);
    let fraction = move |p: f32| {
        let s = span.get_untracked();
        let f = if s > 0.0 { (p / s).clamp(0.0, 1.0) } else { 0.0 };
        (if vertical { 1.0 - f } else { f }) as f64
    };
    let at = move |e: &ResponderEvent| {
        fraction(if vertical {
            e.location.y.0
        } else {
            e.location.x.0
        })
    };
    let on_move = Rc::new(on_move);
    let grant = on_move.clone();
    let on_release = Rc::new(on_release);
    let term = on_release.clone();
    area.on_layout(move |frame: &Bounds<Points>| {
        span.set(if vertical {
            frame.size.height.0
        } else {
            frame.size.width.0
        })
    })
    .claim_touches(TouchClaim::on_start())
    .keep_touches(true)
    .on_responder_grant(move |e: &ResponderEvent| grant(at(e)))
    .on_responder_move(move |e: &ResponderEvent| on_move(at(e)))
    .on_responder_release(move |e: &ResponderEvent| on_release(at(e)))
    // 手势被外层滚动视图抢走或取消时按抬起收尾：提交末位并清本地
    // 位置，否则 drag 留在 Some 里 thumb 会停在当时的位置。
    .on_responder_terminate(move |e: &ResponderEvent| term(at(e)))
}

/// 通用滑块：`style` 定方向与外观（[`SliderStyle::HORIZONTAL`] /
/// [`SliderStyle::VERTICAL`] / [`SliderStyle::FADER`] 或自组），
/// `fraction` 是当前位置（0..1），`on_move` 按下 / 移动调，`on_release`
/// 抬起调。长侧大小由调用方的 `w`/`h` 定（`style.hit` 只管细侧）。
pub fn slider(
    style: SliderStyle,
    fraction: impl Fn() -> f64 + Copy + 'static,
    track: AdaptiveColor,
    fill: AdaptiveColor,
    thumb: AdaptiveColor,
    on_move: impl Fn(f64) + 'static,
    on_release: impl Fn(f64) + 'static,
) -> View {
    // 拖动中的实时位置：本地信号即时画；抬起写完回调才交回 fraction。
    let drag = signal(None::<f64>);
    let pos = move || drag.get().unwrap_or_else(|| fraction().clamp(0.0, 1.0)) as f32;
    let at = move || relative(pos());
    // 填充的（偏移, 长度）：单极从起点长，双极从中心向 thumb 长。
    let span = move || {
        let f = pos();
        if style.bipolar {
            (relative(f.min(0.5)), relative((f - 0.5).abs()))
        } else {
            (relative(0.0), relative(f))
        }
    };
    let rail = view()
        .rounded_full()
        .bg(track)
        .overflow_hidden()
        .pointer_events(PointerEvents::None);
    let rail = if style.vertical {
        rail.w(px(style.track)).h_full().child(
            view()
                .absolute()
                .w_full()
                .rounded_full()
                .bg(fill)
                .bottom(move || span().0)
                .h(move || span().1),
        )
    } else {
        rail.h(px(style.track)).w_full().child(
            view()
                .absolute()
                .h_full()
                .rounded_full()
                .bg(fill)
                .left(move || span().0)
                .w(move || span().1),
        )
    };
    let knob = view()
        .absolute()
        .size(px(style.thumb_d))
        .rounded_full()
        .bg(thumb)
        .shadow_sm()
        .pointer_events(PointerEvents::None);
    // thumb 位置：长侧随 pos，细侧居中（relative(0.5) + 负边距，不依赖 hit）。
    let knob = if style.vertical {
        knob.left(relative(0.5))
            .ml(px(-style.thumb_d / 2.0))
            .bottom(at)
            .mb(px(-style.thumb_d / 2.0))
    } else {
        knob.top(relative(0.5))
            .mt(px(-style.thumb_d / 2.0))
            .left(at)
            .ml(px(-style.thumb_d / 2.0))
    };
    let knob = match style.thumb {
        Thumb::Always => knob,
        Thumb::OnHover => knob
            .opacity(0.0)
            .group_hover(|st| st.opacity(1.0))
            .style_when(move || drag.get().is_some(), |st| st.opacity(1.0))
            .transition_opacity()
            .duration(120.0),
    };
    let area = view().group().child(rail).child(knob);
    let area = match (style.vertical, style.hit) {
        (false, Some(h)) => area.h(px(h)).justify_center(),
        (true, Some(w)) => area.w(px(w)).items_center(),
        (false, None) => area.justify_center(),
        (true, None) => area.items_center(),
    };
    let on_move = Rc::new(on_move);
    let on_release = Rc::new(on_release);
    let m = on_move.clone();
    let r = on_release.clone();
    scrub(
        area,
        style.vertical,
        move |f| {
            drag.set(Some(f));
            m(f);
        },
        move |f| {
            r(f);
            drag.set(None);
        },
    )
}
