//! 复选框：nsh-base 的 `checkbox`（checked / unchecked / indeterminate
//! 三态、键盘激活与无障碍）加上 shadcn Checkbox 的外观：16 pt 方框
//! （`rounded-[4px] border shadow-xs`），选中 / 半选时主色底 + 勾 / 减号
//! 图标（10 pt，主色前景），指示器外 6 pt 点击余量。

use std::rc::Rc;

use nsh_base::{CheckboxElement, checkbox as base_checkbox};
use nsh_core::prelude::*;

use super::icons::{Icon, icon};
use zeo_ui_store::theme::*;

/// 三态复选框：`checked` 与 `indeterminate` 都读自调用方状态（半选优先
/// 显示），`label` 是无障碍标签，`on_change` 收到按下请求的目标值
/// （半选 / 未选 → `true`，已选 → `false`）。
pub fn checkbox(
    checked: impl Fn() -> bool + 'static,
    indeterminate: impl Fn() -> bool + 'static,
    label: impl Into<SharedString>,
    on_change: impl Fn(bool) + 'static,
) -> CheckboxElement {
    let checked = Rc::new(checked);
    let indeterminate = Rc::new(indeterminate);
    // 半选 / 已选都画主色底。
    let on = {
        let (checked, indeterminate) = (checked.clone(), indeterminate.clone());
        move || checked() || indeterminate()
    };
    let on_b = on.clone();
    let mark = {
        let (checked, indeterminate) = (checked.clone(), indeterminate.clone());
        view()
            .items_center()
            .justify_center()
            .child(dynamic(move || {
                if indeterminate() {
                    icon(Icon::Minus, 10.0, primary_fg).into_element()
                } else if checked() {
                    icon(Icon::Check, 10.0, primary_fg).into_element()
                } else {
                    view().into_element()
                }
            }))
    };
    base_checkbox()
        .checked(move || checked())
        .indeterminate(move || indeterminate())
        .accessibility_label(label.into())
        .hit_slop(EdgeInsets::all(px(6.0)))
        .items_center()
        .justify_center()
        .indicator(
            view()
                .size(px(16.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(move || if on() { primary() } else { border() })
                .bg(move || {
                    if on_b() {
                        primary()
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .shadow_xs()
                .items_center()
                .justify_center()
                .transition_colors()
                .duration(150.0)
                .child(mark),
        )
        .on_change(move |next: &bool| on_change(*next))
}
