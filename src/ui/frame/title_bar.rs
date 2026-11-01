//! 标题栏：透明，浮在侧栏与内容区顶部，图标与名称相对整个窗口居中；
//! 两侧留出同样宽的边距（取 macOS 交通灯与 Windows 窗口按钮里较宽的一侧），
//! 窄窗口时也不会压到它们。其余区域拖动窗口。

use nsh_core::prelude::*;

use super::super::components::icons;
use super::super::theme::*;

pub(crate) fn title_bar() -> View {
    let inset = move || {
        let bar = nsh_core::element::desktop::title_bar();
        px(bar.left.0.max(controls_right()) + 12.0)
    };
    view()
        .absolute()
        .top(px(0.0))
        .left(px(0.0))
        .right(px(0.0))
        .h(move || px(top_inset()))
        .flex_row()
        .items_center()
        .justify_center()
        .window_drag(true)
        .px(inset)
        .child(
            view()
                .flex_row()
                .flex_shrink_0()
                .items_center()
                .gap(px(8.0))
                .child(icons::logo(18.0))
                .child(
                    text("Zeo Player")
                        .flex_shrink_0()
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg()),
                ),
        )
}
