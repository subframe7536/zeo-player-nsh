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
use nsh_core::element::desktop::set_title_bar;
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
    bench(open.clone());

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
        .child(view().h(move || px(top_inset())).window_drag(true))
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

thread_local! {
    /// 专辑网格（测量脚本滚动它）。
    pub(crate) static BENCH_GRID: std::cell::Cell<Option<nsh_core::element::ElementRef>> = const { std::cell::Cell::new(None) };
}

/// 内存测量用的脚本（`ZEO_BENCH=1` 启动时运行，平时不做任何事）：依次打开
/// 各视图、在专辑页停留、播放并打开全屏、连续切 20 首，每步之间留时间测量。
/// 进度写到标准错误：`ZEO_BENCH step <名称>`。
fn bench(open_full: Rc<dyn Fn()>) {
    if std::env::var_os("ZEO_BENCH").is_none() {
        return;
    }
    let nav = global::<Nav>();
    spawn(async move {
        let step = |name: &str| eprintln!("ZEO_BENCH step {name}");
        let wait = |s: u64| timer(std::time::Duration::from_secs(s));
        // 等扫描结束。
        while store::store().library.with_untracked(|l| l.scanning) || store::store().tracks.with_untracked(|t| t.is_empty()) {
            wait(1).await;
        }
        step("idle");
        wait(6).await;
        for (target, name) in [(Nav::Artists, "artists"), (Nav::Genres, "genres"), (Nav::Albums, "albums")] {
            nav.set(target);
            step(name);
            wait(6).await;
        }
        // 专辑网格从头滚到底：每张封面都解码一遍（最坏情况）。
        if let Some(grid) = BENCH_GRID.with(|g| g.get()) {
            for i in 0..120 {
                grid.scroll_to_offset(px(i as f32 * 400.0), false);
                timer(std::time::Duration::from_millis(150)).await;
            }
            step("scrolled");
            wait(6).await;
            nav.set(Nav::Folders);
            step("left");
            wait(10).await;
        }
        let tracks = store::store().tracks.get_untracked();
        store::play_list(&tracks, 0);
        open_full();
        step("full");
        wait(8).await;
        for i in 0..20 {
            store::player().next();
            wait(2).await;
            if i == 19 {
                step("skipped");
            }
        }
        wait(6).await;
        step("done");
    });
}
