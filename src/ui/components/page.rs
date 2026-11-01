//! 页面脚手架：页头（可选返回、大标题、副标题、右侧操作）与空状态。

use std::rc::Rc;

use nsh_core::prelude::*;

use super::button::{Size, Variant};
use super::icons::{Icon, icon};
use super::super::theme::*;

/// 页头：可选返回、大标题、副标题、右侧操作；整条可拖动窗口。
pub fn page_header(
    title: impl Fn() -> String + 'static,
    subtitle: impl Fn() -> String + 'static,
    back: Option<Rc<dyn Fn()>>,
    actions: Option<AnyElement>,
) -> View {
    // shadcn ghost / sm，左侧留给图标的内边距收窄，文字与标题左对齐。
    let back_button = back.map(|back| {
        view().self_start().ml(px(-8.0)).window_drag(false).child(
            super::button::shell(Variant::Ghost, Size::Sm)
                .pl(px(6.0))
                .gap(px(2.0))
                .accessibility_label("返回")
                .on_press(move |_: &()| back())
                .child(icon(Icon::ChevronLeft, 16.0, muted_fg))
                .child(
                    text("返回")
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(muted_fg()),
                ),
        )
    });
    view()
        .flex_row()
        .items_end()
        .gap(px(16.0))
        .px(px(32.0))
        .pt(px(10.0))
        .pb(px(16.0))
        .window_drag(true)
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(4.0))
                .child(back_button)
                .child(
                    text(title)
                        .number_of_lines(1)
                        .text_size(px(28.0))
                        .line_height(px(32.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(fg()),
                )
                .child(
                    text(subtitle)
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(actions.map(|a| view().flex_row().gap(px(8.0)).window_drag(false).child(a)))
}

/// 空状态（居中图标 + 标题 + 说明）。
pub fn empty_state(name: Icon, title: &'static str, body: &'static str) -> View {
    view()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .p(px(32.0))
        .pb(px(BAR_SPACE))
        .child(
            view()
                .size(px(64.0))
                .rounded(px(18.0))
                .bg(muted())
                .items_center()
                .justify_center()
                .mb(px(8.0))
                .child(icon(name, 28.0, muted_fg)),
        )
        .child(
            text(title)
                .text_size(px(16.0))
                .line_height(px(16.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg()),
        )
        .child(
            text(body)
                .text_size(px(13.0))
                .text_color(muted_fg())
                .text_center()
                .max_w(px(360.0)),
        )
}
