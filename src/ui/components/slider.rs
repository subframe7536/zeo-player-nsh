//! 滑块：nsh-base 的 `slider`（手势、键盘、无障碍、拖动中本地跟手）套上
//! shadcn 外观的三个预设——横向细轨 / 竖向 / EQ 双极推子。
//!
//! EQ 推子是双极填充（0 dB 居中、向正负两端各长一半）：上游的 `range`
//! 只从起点画到 thumb（`Range` 则是两个 thumb 之间），画不出中心填充，
//! 所以 FADER 自己画轨道 / 填充 / thumb，透明 `slider` 盖在上面接管手势、
//! 键盘与无障碍。波形条同理——视觉全由 canvas 画，`scrub` 盖一个全透明
//! `slider` 作拖动区（比裸 responder 多键盘步进与 Adjustable 语义）。

use std::rc::Rc;

use nsh_base::{SliderElement, SliderValue, slider as nsh_slider};
use nsh_core::geometry::Axis;
use nsh_core::prelude::*;

/// thumb 的显示时机。
#[derive(Clone, Copy, PartialEq)]
pub enum Thumb {
    /// 悬停 / 拖动 / 键盘焦点时才显示（细轨滑块）。
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
    /// 键盘 / 无障碍步进（0..1 的比例）。
    pub step: f64,
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
        step: 0.01,
    };
    /// 竖向滑块，填充从底部长起（音量弹层）。
    pub const VERTICAL: SliderStyle = SliderStyle {
        vertical: true,
        ..SliderStyle::HORIZONTAL
    };
    /// 双极竖推子（均衡器 ±12 dB，0.5 dB 一步）；两侧大小都交给调用方。
    pub const FADER: SliderStyle = SliderStyle {
        vertical: true,
        bipolar: true,
        thumb: Thumb::Always,
        hit: None,
        step: 0.5 / 24.0,
        ..SliderStyle::HORIZONTAL
    };
}

/// nsh-base `slider` 的单值回调换算（本模块的滑块都是 `Single`）。
fn on_single(
    f: impl Fn(f64) + 'static,
) -> impl Fn(&SliderValue) + 'static {
    move |v: &SliderValue| {
        if let SliderValue::Single(v) = v {
            f(*v)
        }
    }
}

/// 全透明的 `slider`：只接管手势、键盘与无障碍（`track` 命中区铺满自身、
/// `indicator` 同样铺满好让位置换算对上；thumb 留 24pt 的隐形命中区，
/// 零尺寸的视图不上屏，无障碍 / `label:` 匹配与 thumb 命中都没了）。
fn overlay_slider(
    vertical: bool,
    step: f64,
    a11y: &'static str,
    fraction: impl Fn() -> f64 + Copy + 'static,
    on_move: impl Fn(f64) + 'static,
    on_release: impl Fn(f64) + 'static,
) -> SliderElement {
    nsh_slider()
        .axis(if vertical {
            Axis::Vertical
        } else {
            Axis::Horizontal
        })
        .min(0.0)
        .max(1.0)
        .step(step)
        .value(move || Some(SliderValue::Single(fraction().clamp(0.0, 1.0))))
        .on_change(on_single(on_move))
        .on_change_end(on_single(on_release))
        .accessibility_label(a11y)
        .track_style(|st| st.w_full().h_full())
        .indicator_style(|st| st.w_full().h_full())
        .thumb_style(|st| st.size(px(24.0)))
}

/// 覆盖在自定义内容上的不可见滑块（波形）：`area` 负责全部视觉，
/// `fraction` 是内容的当前位置（0..1），`on_move` 按下 / 移动 /
/// 键盘步进调，`on_release` 手势结束调。
pub fn scrub(
    area: View,
    vertical: bool,
    a11y: &'static str,
    fraction: impl Fn() -> f64 + Copy + 'static,
    on_move: impl Fn(f64) + 'static,
    on_release: impl Fn(f64) + 'static,
) -> View {
    // 标签只标在 thumb 上（`accessibility_label`）：外层的 `label:` 选中后，
    // 测试拖动走的是命中视图的「自身 + 祖先」链，盖在上面的滑块是外层视图
    // 的子级、不在链里，收不到手势。
    view()
        .child(area)
        .child(
            overlay_slider(vertical, 0.01, a11y, fraction, on_move, on_release)
                .absolute()
                .inset_0(),
        )
}

/// 通用滑块：`style` 定方向与外观（[`SliderStyle::HORIZONTAL`] /
/// [`SliderStyle::VERTICAL`] / [`SliderStyle::FADER`] 或自组），
/// `fraction` 是当前位置（0..1），`on_move` 按下 / 移动 / 键盘步进调，
/// `on_release` 手势结束调。长侧大小由调用方的 `w`/`h` 定（`style.hit`
/// 只管细侧）。
pub fn slider(
    style: SliderStyle,
    a11y: &'static str,
    fraction: impl Fn() -> f64 + Copy + 'static,
    track: AdaptiveColor,
    fill: AdaptiveColor,
    thumb: AdaptiveColor,
    on_move: impl Fn(f64) + 'static,
    on_release: impl Fn(f64) + 'static,
) -> View {
    if style.bipolar {
        return fader(style, a11y, fraction, track, fill, thumb, on_move, on_release);
    }
    let thumb_class = match style.thumb {
        // 轨道是 `group`：悬停 / 按下 / 内部有焦点时才显出 thumb。
        Thumb::OnHover => Some(
            "opacity-0 group-hover:opacity-100 group-active:opacity-100 \
             group-focus-within:opacity-100 transition-opacity duration-100"
                .into(),
        ),
        Thumb::Always => None,
    };
    let el = nsh_slider()
        .axis(if style.vertical {
            Axis::Vertical
        } else {
            Axis::Horizontal
        })
        .min(0.0)
        .max(1.0)
        .step(style.step)
        .value(move || Some(SliderValue::Single(fraction().clamp(0.0, 1.0))))
        .on_change(on_single(on_move))
        .on_change_end(on_single(on_release))
        .accessibility_label(a11y)
        .track_style(move |st| {
            let st = st.w_full().h_full();
            if style.vertical {
                st.items_center()
            } else {
                st.justify_center()
            }
        })
        .indicator_style(move |st| {
            let st = st.rounded_full().bg(track);
            if style.vertical {
                st.w(px(style.track)).h_full()
            } else {
                st.h(px(style.track)).w_full()
            }
        })
        .range_style(move |st| st.rounded_full().bg(fill))
        .thumb_class(thumb_class)
        .thumb_style(move |st| {
            st.size(px(style.thumb_d))
                .rounded_full()
                .bg(thumb)
                .shadow_sm()
        })
        .w_full()
        .h_full();
    let area = view().child(el);
    match (style.vertical, style.hit) {
        (false, Some(h)) => area.h(px(h)),
        (true, Some(w)) => area.w(px(w)),
        _ => area,
    }
}

/// EQ 双极推子：轨道 / 中心向填充 / 常显 thumb 自己画，透明 `slider`
/// 盖在上面接管手势、键盘（0.5 dB 一步）与无障碍。
fn fader(
    style: SliderStyle,
    a11y: &'static str,
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
    let rail = view()
        .w(px(style.track))
        .h_full()
        .rounded_full()
        .bg(track)
        .overflow_hidden()
        .pointer_events(PointerEvents::None)
        .child(
            view()
                .absolute()
                .w_full()
                .rounded_full()
                .bg(fill)
                .bottom(move || relative(pos().min(0.5)))
                .h(move || relative((pos() - 0.5).abs())),
        );
    let knob = view()
        .absolute()
        .size(px(style.thumb_d))
        .rounded_full()
        .bg(thumb)
        .shadow_sm()
        .pointer_events(PointerEvents::None)
        .left(relative(0.5))
        .ml(px(-style.thumb_d / 2.0))
        .bottom(move || relative(pos()))
        .mb(px(-style.thumb_d / 2.0));
    let on_move = Rc::new(on_move);
    let m = on_move.clone();
    view()
        .items_center()
        .child(rail)
        .child(knob)
        .child(
            overlay_slider(
                true,
                style.step,
                a11y,
                fraction,
                move |f| {
                    drag.set(Some(f));
                    m(f);
                },
                move |f| {
                    on_release(f);
                    drag.set(None);
                },
            )
            .absolute()
            .inset_0(),
        )
}
