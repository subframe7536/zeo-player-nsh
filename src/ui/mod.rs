//! Zeo 的界面（纯 Rust）：侧栏导航 + 内容区 + 浮动播放条；全屏播放为
//! 带淡入的覆盖层。

mod bar;
mod full;
mod icons;
mod parts;
pub mod store;
mod theme;
mod views;

use std::rc::Rc;

use nsh_core::desktop::TitleBarStyle;
use nsh_core::element::desktop::{set_title_bar, title_bar};
use nsh_core::prelude::*;

use icons::{icon, Icon};
use theme::*;

/// 侧栏的去处。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Nav {
    #[default]
    Folders,
    Artists,
    Albums,
    Genres,
    Settings,
}

const LIBRARY: [(Nav, &str, Icon); 4] = [
    (Nav::Folders, "文件夹", Icon::Folder),
    (Nav::Artists, "歌手", Icon::Mic),
    (Nav::Albums, "专辑", Icon::Disc),
    (Nav::Genres, "标签", Icon::Tag),
];

/// 应用根组件（`nsh.json` `"root"`）。
#[component]
pub fn App() -> impl IntoElement {
    let s = store::install();
    set_title_bar(TitleBarStyle::Hidden, Some(px(TITLE_BAR)));
    on_key("space", |_| store::player().toggle());

    let full_open = signal(false);
    let fade = animated_value(0.0);
    let open = {
        let fade = fade.clone();
        Rc::new(move || {
            if full_open.get_untracked() {
                return;
            }
            full_open.set(true);
            let _ = fade.timing(1.0, 240.0, Easing::EaseOut);
        }) as Rc<dyn Fn()>
    };
    let close = {
        let fade = fade.clone();
        Rc::new(move || {
            let end = fade.timing(0.0, 180.0, Easing::EaseIn);
            spawn(async move {
                end.await;
                full_open.set(false);
            });
        }) as Rc<dyn Fn()>
    };
    let _ = s;

    let nav = global::<Nav>();
    let content = dynamic(move || match nav.get() {
        Nav::Folders => views::folders().into_element(),
        Nav::Artists => views::artists().into_element(),
        Nav::Albums => views::albums().into_element(),
        Nav::Genres => views::genres().into_element(),
        Nav::Settings => views::settings().into_element(),
    });

    view()
        .flex_1()
        .flex_row()
        .bg(bg())
        .text_color(fg())
        // 文字样式由里面的文字继承：整个界面跟着设置里的字体走。
        .font_family(store::font_family)
        .child(sidebar())
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .child(view().flex_1().child(content))
                .child(bar::player_bar(open)),
        )
        .child(when(move || full_open.get(), move || full::full_player(fade.clone(), close.clone())))
}

fn nav_row(target: Nav, label: &'static str, glyph: Icon) -> View {
    let nav = global::<Nav>();
    let active = move || nav.get() == target;
    view()
        .h(px(34.0))
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .px(px(10.0))
        .rounded(px(8.0))
        .role(AccessibilityRole::Button)
        .label(label)
        .accessibility_state(move || AccessibilityState { selected: active(), ..AccessibilityState::default() })
        .style_when(active, |s| s.bg(sidebar_accent()))
        .hover(|s| s.bg(sidebar_accent()))
        .active(|s| s.opacity(0.7))
        .on_press(move || nav.set(target))
        .child(icon(glyph, 17.0, move || if active() { primary() } else { sidebar_fg() }))
        .child(
            text(label)
                .flex_1()
                .text_size(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if active() { theme::fg() } else { sidebar_fg() }),
        )
}

fn section_label(label: &'static str) -> Text {
    text(label)
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .letter_spacing(px(0.6))
        .text_color(faint())
        .px(px(10.0))
        .pt(px(16.0))
        .pb(px(6.0))
}

fn sidebar() -> View {
    let s = store::store();
    view()
        .w(px(SIDEBAR))
        .flex_shrink_0()
        .bg(theme::sidebar())
        .border_r_1()
        .border_r_color(sidebar_border())
        .child(view().h(move || px(title_bar().top.0.max(TITLE_BAR))).window_drag(true))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .px(px(18.0))
                .pb(px(6.0))
                .window_drag(true)
                .child(
                    view()
                        .size(px(30.0))
                        .rounded(px(8.0))
                        .bg(primary())
                        .items_center()
                        .justify_center()
                        .child(icon(Icon::Note, 17.0, primary_fg)),
                )
                .child(
                    text("Zeo")
                        .text_size(px(18.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::fg()),
                ),
        )
        .child(
            view()
                .px(px(10.0))
                .gap(px(2.0))
                .child(section_label("曲库"))
                .children(LIBRARY.iter().map(|&(target, label, glyph)| nav_row(target, label, glyph))),
        )
        .child(view().flex_1().window_drag(true))
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
                    .tabular_nums()
                    .text_color(faint())
                    .px(px(10.0))
                    .pt(px(8.0)),
                ),
        )
}
