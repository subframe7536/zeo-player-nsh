//! Zeo Player 的界面（纯 Rust）：侧栏导航 + 内容区 + 浮动播放条；全屏播放为
//! 带淡入的覆盖层。

mod bar;
mod full;
mod icons;
mod parts;
mod playlists;
mod queue;
pub mod store;
mod theme;
mod views;

use std::rc::Rc;

use nsh_core::desktop::TitleBarStyle;
use nsh_core::element::desktop::set_title_bar;
use nsh_core::element::input;
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
    /// 一个歌单（id）。
    Playlist(u64),
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

    let queue_open = signal(false);
    // 窗口隐藏到托盘时整个界面拆掉（视图、图片、列表都释放），只留播放与
    // 状态轮询；再次显示时重建，导航位置在全局信号里，不丢。
    let shell = move || {
        let content = page(nav);
        let (open, close, fade) = (open.clone(), close.clone(), fade.clone());
        view()
            .flex_1()
            .child(title_bar())
            .child(
                view()
                    .flex_1()
                    .flex_row()
                    .child(sidebar())
                    .child(
                        view()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(view().flex_1().child(content))
                            .child(when(move || queue_open.get(), move || queue::queue_panel(queue_open)))
                            .child(bar::player_bar(open, queue_open)),
                    )
                    .child(when(move || full_open.get(), move || full::full_player(fade.clone(), close.clone()))),
            )
    };

    view()
        .flex_1()
        .bg(bg())
        .text_color(fg())
        // 文字样式由里面的文字继承：整个界面跟着设置里的字体走。
        .font_family(store::font_family)
        .child(when(move || !s.hidden.get(), shell))
        .child(when(move || s.close_prompt.get(), close_prompt))
        .child(when(move || s.editor.with(Option::is_some), playlists::editor_dialog))
}

/// 标题栏行：中间是应用图标与名称，右侧搜索框；两端让出系统按钮
/// （macOS 交通灯在左，Windows 标题栏按钮在右），其余区域拖动窗口。
fn title_bar() -> View {
    let s = store::store();
    let query_empty = move || s.search.with(|q| q.is_empty());
    view()
        .h(move || px(top_inset()))
        .flex_row()
        .items_center()
        .window_drag(true)
        .pl(move || px(nsh_core::element::desktop::title_bar().left.0 + 8.0))
        .pr(move || px(controls_right() + 12.0))
        .child(view().flex_1())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(icons::logo(20.0))
                .child(text("Zeo Player").text_size(px(15.0)).font_weight(FontWeight::SEMIBOLD).text_color(fg())),
        )
        .child(
            view().flex_1().flex_row().items_center().justify_end().window_drag(false).child(
                view()
                    .w(px(200.0))
                    .h(px(30.0))
                    .child(icon(Icon::Search, 13.0, faint).absolute().left(px(10.0)).top(px(8.5)))
                    .child(
                        input()
                            .w_full()
                            .h(px(30.0))
                            .pl(px(30.0))
                            .pr(px(26.0))
                            .rounded(px(15.0))
                            .border_1()
                            .border_color(border())
                            .bg(bg())
                            .text_size(px(13.0))
                            .text_color(fg())
                            .value(move || s.search.get())
                            .placeholder("搜索")
                            .on_change(move |t: &str| s.search.set(t.to_owned())),
                    )
                    .child(when(move || !query_empty(), move || {
                        parts::icon_button(|| Icon::Close, 10.0, "清除搜索", faint, || store::store().search.set(String::new()))
                            .absolute()
                            .right(px(2.0))
                            .top(px(2.0))
                    })),
            ),
        )
}

fn page(nav: Signal<Nav>) -> AnyElement {
    let s = store::store();
    dynamic(move || {
        if !s.search.with(|q| q.trim().is_empty()) {
            return views::search_results().into_element();
        }
        match nav.get() {
            Nav::Folders => views::folders().into_element(),
            Nav::Artists => views::artists().into_element(),
            Nav::Albums => views::albums().into_element(),
            Nav::Genres => views::genres().into_element(),
            Nav::Settings => views::settings().into_element(),
            Nav::Playlist(id) => playlists::page(id).into_element(),
        }
    })
}

/// 第一次点关闭：最小化到托盘还是退出。选择会记住（设置里可改）。
fn close_prompt() -> View {
    use crate::library::CloseAction;
    let s = store::store();
    on_key("escape", move |_| s.close_prompt.set(false));
    view()
        .absolute()
        .inset(px(0.0))
        .z_index(100)
        .bg(black_a(0.4))
        .items_center()
        .justify_center()
        .on_press(move || s.close_prompt.set(false))
        .child(
            view()
                .w(px(400.0))
                .p(px(24.0))
                .gap(px(8.0))
                .rounded(px(16.0))
                .bg(nsh_base::theme().popover)
                .border_1()
                .border_color(border())
                .shadow_2xl()
                // 点卡片本身不关闭。
                .on_press(|| {})
                .child(text("关闭窗口").text_size(px(17.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme::fg()))
                .child(
                    text("最小化到托盘后音乐继续播放，可以从托盘图标打开窗口、切歌或退出。这个选择会被记住，可以在「设置」里更改。")
                        .text_size(px(13.0))
                        .line_height(px(20.0))
                        .text_color(muted_fg()),
                )
                .child(
                    view()
                        .flex_row()
                        .justify_end()
                        .gap(px(8.0))
                        .mt(px(12.0))
                        .child(parts::outline_button(Icon::Close, "退出 Zeo Player", || store::answer_close(CloseAction::Quit)))
                        .child(parts::pill_button(Icon::ChevronDown, "最小化到托盘", || store::answer_close(CloseAction::Tray))),
                ),
        )
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

pub(crate) fn section_label(label: &'static str) -> Text {
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
        .pt(px(4.0))
        .child(
            view()
                .px(px(10.0))
                .gap(px(2.0))
                .child(section_label("曲库"))
                .children(LIBRARY.iter().map(|&(target, label, glyph)| nav_row(target, label, glyph))),
        )
        .child(playlists::sidebar_section())
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

/// 内存测量用的脚本（`ZEO_PLAYER_BENCH=1` 启动时运行，平时不做任何事）：依次打开
/// 各视图、在专辑页停留、播放并打开全屏、连续切 20 首，每步之间留时间测量。
/// 进度写到标准错误：`ZEO_PLAYER_BENCH step <名称>`。
fn bench(open_full: Rc<dyn Fn()>) {
    if std::env::var_os("ZEO_PLAYER_BENCH").is_none() {
        return;
    }
    if std::env::var("ZEO_PLAYER_BENCH").as_deref() == Ok("close") {
        return bench_close();
    }
    let nav = global::<Nav>();
    spawn(async move {
        let step = |name: &str| eprintln!("ZEO_PLAYER_BENCH step {name}");
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
        if let Some(grid) = BENCH_GRID.with(|g| g.get()).filter(|_| std::env::var_os("ZEO_PLAYER_BENCH_NO_SCROLL").is_none()) {
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

/// `ZEO_PLAYER_BENCH=close`：点关闭 → 询问 → 最小化到托盘 → 从托盘显示 → 退出。
fn bench_close() {
    use crate::library::CloseAction;
    spawn(async move {
        let s = store::store();
        let step = |name: &str| eprintln!("ZEO_PLAYER_BENCH step {name} hidden={} prompt={}", crate::window::is_hidden(), s.close_prompt.get_untracked());
        let wait = |ms: u64| timer(std::time::Duration::from_millis(ms));
        while s.tracks.with_untracked(|t| t.is_empty()) {
            wait(500).await;
        }
        store::set_close_action(CloseAction::Ask);
        wait(3000).await;
        let tracks = s.tracks.get_untracked();
        store::play_list(&tracks, 0);
        wait(1500).await;
        nsh_core::element::desktop::close_window();
        wait(1000).await;
        step("asked");
        store::answer_close(CloseAction::Tray);
        wait(4000).await;
        step("tray");
        eprintln!("ZEO_PLAYER_BENCH playing={} time={:.1}", s.playback.with_untracked(|p| p.playing), s.playback.with_untracked(|p| p.time));
        wait(6000).await;
        eprintln!("ZEO_PLAYER_BENCH playing={} time={:.1}", s.playback.with_untracked(|p| p.playing), s.playback.with_untracked(|p| p.time));
        nsh_core::element::desktop::close_window();
        wait(500).await;
        step("closed-again-while-hidden");
        crate::window::show();
        wait(2000).await;
        step("shown");
        // 已记住「托盘」：再点关闭直接隐藏，不再询问。
        nsh_core::element::desktop::close_window();
        wait(1500).await;
        step("remembered");
        store::set_close_action(CloseAction::Ask);
        wait(500).await;
        eprintln!("ZEO_PLAYER_BENCH step quit");
        crate::window::quit();
    });
}
