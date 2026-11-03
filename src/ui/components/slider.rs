//! 拖动输入：`scrub` 裸拖动区；`slider` 细轨道 + 填充 + 悬停 thumb 的滑块
//!（nsh-base 0.3.0 没有 slider 组件，这里自己组合，见 README「待上游」）。

use std::rc::Rc;

use nsh_core::TouchEvent;
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
        // 进触摸路径的门票：claim 只协商已在路径上的节点，不挂 on_touch
        // 的话 responder 永远不被问到（事件掩码里没有 Touch）。
        .on_touch(|_: &TouchEvent| {})
        .claim_touches(TouchClaim::on_start())
        .keep_touches(true)
        .on_responder_grant(move |e: &ResponderEvent| grant(fraction(e.location.x.0)))
        .on_responder_move(move |e: &ResponderEvent| on_move(fraction(e.location.x.0)))
        .on_responder_release(move |e: &ResponderEvent| on_release(fraction(e.location.x.0)))
}

/// 纵向拖动取比例（0 在底部）：同 [`scrub`]，沿高度量。
fn vscrub(area: View, on_move: impl Fn(f64) + 'static, on_release: impl Fn(f64) + 'static) -> View {
    let height = signal(1.0_f32);
    let fraction = move |y: f32| {
        let h = height.get_untracked();
        if h > 0.0 {
            (1.0 - y / h).clamp(0.0, 1.0) as f64
        } else {
            0.0
        }
    };
    let on_move = Rc::new(on_move);
    let grant = on_move.clone();
    area.on_layout(move |frame: &Bounds<Points>| height.set(frame.size.height.0))
        // 同 [`scrub`]：先挂 on_touch 进路径，claim 才生效。
        .on_touch(|_: &TouchEvent| {})
        .claim_touches(TouchClaim::on_start())
        .keep_touches(true)
        .on_responder_grant(move |e: &ResponderEvent| grant(fraction(e.location.y.0)))
        .on_responder_move(move |e: &ResponderEvent| on_move(fraction(e.location.y.0)))
        .on_responder_release(move |e: &ResponderEvent| on_release(fraction(e.location.y.0)))
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

/// 双极竖推子（均衡器）：细轨道、thumb 常显、填充从中心（0.5）向 thumb
/// 长——正增益向上、负增益向下，0 位不填色。宽 / 高由调用方给，
/// 整块都是拖动区；fraction 0..1，0.5 是中点。
pub fn fader(
    fraction: impl Fn() -> f64 + Copy + 'static,
    track: AdaptiveColor,
    fill: AdaptiveColor,
    thumb: AdaptiveColor,
    on_move: impl Fn(f64) + 'static,
    on_release: impl Fn(f64) + 'static,
) -> View {
    let at = move || relative(fraction().clamp(0.0, 1.0) as f32);
    let span = move || {
        let f = fraction().clamp(0.0, 1.0) as f32;
        (relative(f.min(0.5)), relative((f - 0.5).abs()))
    };
    let rail = view()
        .w(px(TRACK_H))
        .h_full()
        .rounded_full()
        .bg(track)
        .overflow_hidden()
        .pointer_events(PointerEvents::None)
        .child(
            view()
                .absolute()
                .w_full()
                .bottom(move || span().0)
                .h(move || span().1)
                .bg(fill),
        );
    let knob = view()
        .absolute()
        .left(relative(0.5))
        .ml(px(-THUMB / 2.0))
        .bottom(at)
        .mb(px(-THUMB / 2.0))
        .size(px(THUMB))
        .rounded_full()
        .bg(thumb)
        .shadow_sm()
        .pointer_events(PointerEvents::None);
    let area = view()
        .w_full()
        .h_full()
        .items_center()
        .child(rail)
        .child(knob);
    vscrub(area, on_move, on_release)
}

/// 竖向滑块（窄播放条的音量弹层用）：同 [`slider`] 竖过来，填充从底部
/// 长起。宽固定为 [`SLIDER_H`]（可点击厚度），高固定 [`VSLIDER_H`]。
pub fn vslider(
    fraction: impl Fn() -> f64 + Copy + 'static,
    track: AdaptiveColor,
    fill: AdaptiveColor,
    thumb: AdaptiveColor,
    on_move: impl Fn(f64) + 'static,
    on_release: impl Fn(f64) + 'static,
) -> View {
    const VSLIDER_H: f32 = 112.0;
    let dragging = signal(false);
    let at = move || relative(fraction().clamp(0.0, 1.0) as f32);
    let pill = view()
        .w(px(TRACK_H))
        .h_full()
        .rounded_full()
        .bg(track)
        .overflow_hidden()
        .pointer_events(PointerEvents::None)
        // 填充从底部长起。
        .justify_end()
        .child(view().w_full().rounded_full().bg(fill).h(at));
    let knob = view()
        .absolute()
        .left(px((SLIDER_H - THUMB) / 2.0))
        .bottom(at)
        .mb(px(-THUMB / 2.0))
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
        .w(px(SLIDER_H))
        .h(px(VSLIDER_H))
        // 列布局里横向居中用 items_center（justify_center 是垂直方向）。
        .items_center()
        .group()
        .child(pill)
        .child(knob);
    vscrub(
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
