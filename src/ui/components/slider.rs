//! 拖动输入：`scrub` 裸拖动区；`slider` 细轨道 + 填充 + 悬停 thumb 的滑块
//!（nsh-base 0.3.0 没有 slider 组件，这里自己组合，见 README「待上游」）。

use std::rc::Rc;

use nsh_core::prelude::*;

/// 横向拖动取比例（0..1）：按下/移动调 `on_move`，抬起调 `on_release`。
pub fn scrub(
    area: View,
    on_move: impl Fn(f64) + 'static,
    on_release: impl Fn(f64) + 'static,
) -> View {
    let width = signal(1.0_f32);
    let fraction = move |x: f32| {
        let w = width.get_untracked();
        if w > 0.0 {
            (x / w).clamp(0.0, 1.0) as f64
        } else {
            0.0
        }
    };
    let on_move = Rc::new(on_move);
    let grant = on_move.clone();
    area.on_layout(move |frame: &Bounds<Points>| width.set(frame.size.width.0))
        .claim_touches(TouchClaim::on_start())
        .keep_touches(true)
        .on_responder_grant(move |e: &ResponderEvent| grant(fraction(e.location.x.0)))
        .on_responder_move(move |e: &ResponderEvent| on_move(fraction(e.location.x.0)))
        .on_responder_release(move |e: &ResponderEvent| on_release(fraction(e.location.x.0)))
}

/// 滑块的高度（可点击区域）、轨道粗细与 thumb 直径。
const SLIDER_H: f32 = 18.0;
const TRACK_H: f32 = 4.0;
const THUMB: f32 = 12.0;

/// 滑块：细轨道 + 填充，鼠标移入（或拖动中）时在当前位置显示圆形 thumb，
/// 轨道本身不缩放。横向拖动取比例（0..1）：按下/移动调 `on_move`，抬起调
/// `on_release`。
pub fn slider(
    fraction: impl Fn() -> f64 + Copy + 'static,
    track: AdaptiveColor,
    fill: AdaptiveColor,
    thumb: AdaptiveColor,
    on_move: impl Fn(f64) + 'static,
    on_release: impl Fn(f64) + 'static,
) -> View {
    let dragging = signal(false);
    let at = move || relative(fraction().clamp(0.0, 1.0) as f32);
    let pill = view()
        .h(px(TRACK_H))
        .w_full()
        .rounded_full()
        .bg(track)
        .overflow_hidden()
        .pointer_events(PointerEvents::None)
        .child(view().h_full().rounded_full().bg(fill).w(at));
    let knob = view()
        .absolute()
        .top(px((SLIDER_H - THUMB) / 2.0))
        .left(at)
        .ml(px(-THUMB / 2.0))
        .size(px(THUMB))
        .rounded_full()
        .bg(thumb)
        .shadow_sm()
        .pointer_events(PointerEvents::None)
        .opacity(0.0)
        .group_hover(|st| st.opacity(1.0))
        .style_when(move || dragging.get(), |st| st.opacity(1.0))
        .transition_opacity()
        .duration(120.0);
    let area = view()
        .h(px(SLIDER_H))
        .justify_center()
        .group()
        .child(pill)
        .child(knob);
    scrub(
        area,
        move |f| {
            dragging.set(true);
            on_move(f);
        },
        move |f| {
            dragging.set(false);
            on_release(f);
        },
    )
}
