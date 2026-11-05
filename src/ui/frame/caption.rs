//! Windows 的窗口按钮（最小化、最大化 / 还原、关闭），和其它图标一样用
//! SVG 画。
//!
//! NSH 0.3.0 隐藏标题栏时自己画这三个按钮（1 设备像素的 Direct2D 线条，
//! 发虚），而且总画在应用内容之上。所以窗口子类化成功后（`window.rs`
//! 接管 `WM_NCCALCSIZE` / `WM_NCHITTEST`），把 NSH 的标题栏样式切到
//! Default：它不再画按钮，窗口外观不变，按钮由这里画。没有 Snap 布局
//! （鼠标停在最大化按钮上弹出的贴靠面板），Win + Z 仍可用。

use nsh_core::desktop::TitleBarStyle;
use nsh_core::element::desktop::set_title_bar;
use nsh_core::prelude::*;

use super::super::components::icons::{Icon, icon_dyn};
use super::super::theme::*;

/// 每个按钮的宽度（与系统按钮相同）。
pub const BUTTON_W: f32 = 46.0;
/// 字形图片的尺寸（字形本身 10 pt）。
const GLYPH: f32 = 16.0;

/// 应用已接管窗口按钮。
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Own(bool);

/// 等窗口子类化完成，再切换标题栏样式、显示应用的按钮。
pub fn install() {
    let own = global::<Own>();
    spawn(async move {
        // 窗口建好后约 200 ms 内子类化；之前仍是 NSH 自己画的按钮。
        for _ in 0..300 {
            if crate::window::caption_ready() {
                set_title_bar(TitleBarStyle::Default, None);
                own.set(Own(true));
                return;
            }
            timer(std::time::Duration::from_millis(50)).await;
        }
    });
}

/// 应用的按钮正在显示（已接管且不在全屏）。读取即订阅：窗口尺寸变化
/// （最大化、还原、全屏）时重新判断。
pub fn shown() -> bool {
    let _ = window();
    global::<Own>().get().0 && !crate::window::is_full_screen()
}

/// 按钮占去的宽度（未显示时为 0）。
pub fn width() -> f32 {
    if shown() { BUTTON_W * 3.0 } else { 0.0 }
}

/// 右上角的三个按钮。`on_dark` 为真时（全屏播放页）用白色。
pub fn buttons(on_dark: impl Fn() -> bool + Copy + 'static) -> impl IntoElement {
    when(shown, move || {
        let maximized = move || {
            let _ = window();
            crate::window::is_maximized()
        };
        view()
            .absolute()
            .top(px(0.0))
            .right(px(0.0))
            .h(move || px(top_inset()))
            .flex_row()
            .z_index(200)
            .window_drag(false)
            .child(caption_button(
                move || Icon::WinMinimize,
                || "最小化",
                false,
                on_dark,
                crate::window::minimize,
            ))
            .child(caption_button(
                move || {
                    if maximized() {
                        Icon::WinRestore
                    } else {
                        Icon::WinMaximize
                    }
                },
                move || if maximized() { "还原" } else { "最大化" },
                false,
                on_dark,
                crate::window::toggle_maximize,
            ))
            .child(caption_button(
                move || Icon::WinClose,
                || "关闭",
                true,
                on_dark,
                crate::window::close,
            ))
    })
}

fn caption_button(
    glyph: impl Fn() -> Icon + 'static,
    label: impl Fn() -> &'static str + 'static,
    close: bool,
    on_dark: impl Fn() -> bool + Copy + 'static,
    press: fn(),
) -> View {
    let hovered = signal(false);
    let background = move || {
        if !hovered.get() {
            AdaptiveColor::TRANSPARENT
        } else if close {
            AdaptiveColor::rgb(0xc42b1c, 0xc42b1c)
        } else if on_dark() {
            white_a(0.1)
        } else {
            AdaptiveColor::rgb(0x000000, 0xffffff).opacity(0.07)
        }
    };
    let tint = move || {
        if (close && hovered.get()) || on_dark() {
            white_a(if close && hovered.get() { 1.0 } else { 0.85 })
        } else {
            fg()
        }
    };
    // 悬停进出只能挂在裸 view 上（`Interactive` 不在组件元素上）：
    // 外层记悬停，内层 nsh-base `button` 管按压与无障碍。
    view()
        .w(px(BUTTON_W))
        .h_full()
        .on_mouse_enter(move |_: &MouseEvent| hovered.set(true))
        .on_mouse_leave(move |_: &MouseEvent| hovered.set(false))
        .child(
            nsh_base::button()
                .w_full()
                .h_full()
                .items_center()
                .justify_center()
                .accessibility_label(move || SharedString::from(label()))
                .bg(background)
                .active(|st| st.opacity(0.8))
                .on_press(move |_: &()| {
                    hovered.set(false);
                    press();
                })
                .child(icon_dyn(glyph, GLYPH, tint)),
        )
}
