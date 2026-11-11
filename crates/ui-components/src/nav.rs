//! 侧栏导航行：图标 + 文字，导航 / 设置 / 歌单共用外观。

use nsh_core::prelude::*;
use zeo_ui_store::theme::*;

use crate::icons::{Icon, icon};

/// 侧栏里的一行：图标 + 文字。`label` 用 `SharedString`：导航是静态串，
/// 歌单是名字。不是按钮——只指示当前路由：无按下反馈，连点同项不跳变。
pub fn nav_row(
    glyph: Icon,
    label: SharedString,
    selected: impl Fn() -> bool + Copy + 'static,
    on_press: impl Fn() + 'static,
) -> View {
    view()
        .w_full()
        .h(px(34.0))
        .flex_row()
        .items_center()
        // 图标列中心固定在 30 pt（10 外边距 + 10 行内边距 + 半个
        // 20 pt 图标格），与「曲库」小标题的左缘对齐。
        .pl(px(10.0))
        .pr(px(10.0))
        .gap(px(10.0))
        .rounded(px(8.0))
        .overflow_hidden()
        .cursor(Cursor::Pointer)
        .role(AccessibilityRole::Link)
        .label(label.clone())
        .accessibility_state(move || AccessibilityState {
            selected: selected(),
            ..AccessibilityState::default()
        })
        .bg(move || {
            if selected() {
                primary().opacity(0.12)
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .hover(|st| st.bg(move || primary().opacity(0.08)))
        .on_press(move |_: &()| on_press())
        .child(
            view()
                .size(px(20.0))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .child(icon(glyph, 17.0, move || {
                    if selected() { primary() } else { sidebar_fg() }
                })),
        )
        .child(
            view().flex_1().min_w(px(0.0)).child(
                text(label)
                    .number_of_lines(1)
                    .text_size(px(14.0))
                    .line_height(px(14.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || if selected() { fg() } else { sidebar_fg() }),
            ),
        )
}
