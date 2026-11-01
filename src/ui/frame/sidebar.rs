//! 侧栏：搜索框、曲库导航、歌单区、设置入口与底部状态。

use nsh_core::element::ElementRef;
use nsh_core::element::input;
use nsh_core::prelude::*;
use nsh_core::{TouchEvent, TouchPhase};

use super::super::components::button::icon_button;
use super::super::components::icons::{Icon, icon};
use super::super::theme::{self, *};
use super::super::{Nav, store};

const LIBRARY: [(Nav, &str, Icon); 4] = [
    (Nav::Folders, "文件夹", Icon::Folder),
    (Nav::Artists, "歌手", Icon::Mic),
    (Nav::Albums, "专辑", Icon::Disc),
    (Nav::Genres, "标签", Icon::Tag),
];

/// 搜索框的焦点：点在框外时让它失焦。
#[derive(Clone, Copy)]
pub(crate) struct SearchFocus {
    input: ElementRef,
    focused: Signal<bool>,
    /// 这次按下落在搜索框里（由搜索框自己的触摸处理记下）。
    inside: Signal<bool>,
}

impl SearchFocus {
    pub(crate) fn new() -> Self {
        SearchFocus {
            input: element_ref(),
            focused: signal(false),
            inside: signal(false),
        }
    }

    pub(crate) fn outside_touch(self, e: &TouchEvent) {
        if e.phase != TouchPhase::Start {
            return;
        }
        // 同一次按下，搜索框与根视图都收到触摸，先后不定：等这次事件派发
        // 完再看它是否落在框里。
        spawn(async move {
            let inside = self.inside.get_untracked();
            self.inside.set(false);
            if !inside && self.focused.get_untracked() {
                self.input.blur();
            }
        });
    }
}

/// 搜索框的高度。
const SEARCH_H: f32 = 28.0;

/// 侧栏顶部的搜索框。
fn search_box(focus: SearchFocus) -> View {
    let s = store::store();
    let query_empty = move || s.search.with(|q| q.is_empty());
    // 外框画边框与底色，里面一行：搜索图标、输入框（透明）、清除按钮。
    // 原生输入框画在应用内容之上，图标不能叠在它下面。
    view()
        .h(px(SEARCH_H))
        .flex_row()
        .items_center()
        .pl(px(10.0))
        .pr(px(1.0))
        .rounded(px(SEARCH_H / 2.0))
        .border_1()
        .border_color(border())
        .bg(bg())
        .on_touch(move |e: &TouchEvent| {
            if e.phase == TouchPhase::Start {
                focus.inside.set(true);
            }
        })
        // 点在图标上也聚焦输入框。
        .on_press(move || focus.input.focus())
        .child(icon(Icon::Search, 13.0, faint).flex_shrink_0())
        .child(
            input()
                .element_ref(&focus.input)
                .flex_1()
                .min_w(px(0.0))
                .h(px(SEARCH_H - 2.0))
                .px(px(6.0))
                .text_size(px(13.0))
                .text_color(fg())
                .value(move || s.search.get())
                .placeholder("搜索")
                .label("搜索")
                .on_focus(move || focus.focused.set(true))
                .on_blur(move || focus.focused.set(false))
                .on_change(move |t: &str| s.search.set(t.to_owned())),
        )
        .child(when(
            move || !query_empty(),
            move || {
                icon_button(
                    || Icon::Close,
                    10.0,
                    "清除搜索",
                    faint,
                    || store::store().search.set(String::new()),
                )
                .flex_shrink_0()
            },
        ))
}

fn nav_row(target: Nav, label: &'static str, glyph: Icon) -> nsh_base::ButtonElement {
    let nav = global::<Nav>();
    let active = move || nav.get() == target;
    nsh_base::button()
        .h(px(34.0))
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .px(px(10.0))
        .rounded(px(8.0))
        .accessibility_label(label)
        .selected(active)
        .selected_style(|s| s.bg(sidebar_accent()))
        .hover(|s| s.bg(sidebar_accent()))
        .active(|s| s.opacity(0.7))
        .on_press(move |_: &()| nav.set(target))
        .child(icon(glyph, 17.0, move || {
            if active() { primary() } else { sidebar_fg() }
        }))
        .child(
            text(label)
                .flex_1()
                .text_size(px(14.0))
                .line_height(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if active() { theme::fg() } else { sidebar_fg() }),
        )
}

pub(crate) fn section_label(label: &'static str) -> Text {
    text(label)
        .text_size(px(11.0))
        .line_height(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .letter_spacing(px(0.6))
        .text_color(faint())
        .px(px(10.0))
        .pt(px(16.0))
        .pb(px(6.0))
}

pub(crate) fn sidebar(search: SearchFocus) -> View {
    let s = store::store();
    view()
        .w(px(SIDEBAR))
        .flex_shrink_0()
        .bg(theme::sidebar())
        .border_r_1()
        .border_r_color(sidebar_border())
        // 顶部让给标题栏（侧栏底色一直铺到窗口顶边，交通灯落在上面）。
        .child(view().h(move || px(top_inset())).flex_shrink_0())
        .child(view().px(px(10.0)).child(search_box(search)))
        .child(
            view()
                .px(px(10.0))
                .gap(px(2.0))
                .child(section_label("曲库"))
                .children(
                    LIBRARY
                        .iter()
                        .map(|&(target, label, glyph)| nav_row(target, label, glyph)),
                ),
        )
        .child(super::super::views::playlists::sidebar_section())
        .child(
            view()
                .px(px(10.0))
                .pb(px(12.0))
                .gap(px(2.0))
                .child(nav_row(Nav::Settings, "设置", Icon::Settings))
                .child(
                    text(move || {
                        s.library.with(|l| {
                            if l.scanning {
                                "正在扫描曲库…".to_owned()
                            } else {
                                format!("{} 首曲目", s.tracks.with(|t| t.len()))
                            }
                        })
                    })
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .tabular_nums()
                    .text_color(faint())
                    .px(px(10.0))
                    .pt(px(8.0)),
                ),
        )
}
