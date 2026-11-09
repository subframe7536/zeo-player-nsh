//! 命令面板（Command Palette）：浮动快捷搜索弹窗。
//! 可搜索并快速导航页面、执行常用操作（播放/暂停、下一首、设置等）、以及搜索并直接播放曲库曲目。

/// 命令面板弹窗（nsh-base `dialog`，Esc / 点遮罩关闭）。
pub(crate) use nsh_base::{CommandElement, CommandEntry, CommandItem, command};

pub(crate) use nsh_core::prelude::*;

pub(crate) use zeo_ui_components::icons::{Icon, icon};

pub(crate) use zeo_ui_store::theme::{self, *};

pub(crate) use zeo_ui_store::{self as store, Nav};

mod items;
mod palette;

pub(crate) use items::*;
pub(crate) use palette::*;

pub fn dialog() -> AnyElement {
    let close = move || {
        let s = store::store();
        s.search_open.set(false);
        s.search.set(String::new());
    };

    let content = view()
        .w(px(580.0))
        .max_w(move || px((window().width.0 - 32.0).max(240.0)))
        .max_h(move || px((window().height.0 - 120.0).max(240.0)))
        .rounded(px(12.0))
        .border_1()
        .border_color(border())
        .bg(move || nsh_base::theme().card)
        .shadow_xl()
        .overflow_hidden()
        .on_press(|| {})
        .child(palette_content(close));

    nsh_base::dialog()
        .animation(ModalAnimation::Fade)
        .on_cancel(move || close())
        .child(
            view()
                .absolute()
                .inset(px(0.0))
                .bg(black_a(0.5))
                .items_center()
                .pt(px(80.0))
                .on_press(move || close())
                .child(content),
        )
        .into_element()
}
