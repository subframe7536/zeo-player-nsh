//! ZeoPlayer 的界面（纯 Rust）：侧栏导航 + 内容区 + 浮动播放条；全屏播放为
//! 带淡入的覆盖层。

mod components;
mod fonts;
mod frame;
mod now_playing;
pub mod store;
mod theme;
mod views;

use std::rc::Rc;

use nsh_core::desktop::TitleBarStyle;
use nsh_core::element::desktop::set_title_bar;
use nsh_core::prelude::*;
use nsh_core::{ContextMenuEvent, TouchEvent};

use theme::*;

/// 侧栏的去处。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Nav {
    #[default]
    Folders,
    Artists,
    Albums,
    Genres,
    /// 一个歌单（id）。
    Playlist(u64),
}

/// 应用根组件（`nsh.json` `"root"`）。
#[component]
pub fn App() -> impl IntoElement {
    // 单实例兜底：真正的检查在 `native()`（窗口创建之前），这里 `Once`
    // 幂等，防不走 `native()` 的挂载路径。
    crate::single_instance::enforce();
    let s = store::install();
    set_title_bar(TitleBarStyle::Hidden, Some(px(TITLE_BAR)));
    #[cfg(target_os = "windows")]
    frame::caption::install();
    on_key("space", |_| store::player().toggle());

    let full_open = signal(false);
    // 正在收起（淡出动画中）；收起途中再打开会取消它。
    let full_closing = signal(false);
    let fade = animated_value(0.0);
    let open = {
        Rc::new(move || {
            if full_open.get_untracked() && !full_closing.get_untracked() {
                return;
            }
            full_closing.set(false);
            full_open.set(true);
            global::<now_playing::Reveal>().update(|r| r.0 = r.0.wrapping_add(1));
            if untracked(reduce_motion) {
                fade.set(1.0);
            } else {
                let _ = fade.timing(1.0, now_playing::IN_MS, now_playing::IN_EASE);
            }
        }) as Rc<dyn Fn()>
    };
    let close = {
        Rc::new(move || {
            if !full_open.get_untracked() || full_closing.get_untracked() {
                return;
            }
            full_closing.set(true);
            if untracked(reduce_motion) {
                fade.set(0.0);
                full_closing.set(false);
                full_open.set(false);
                return;
            }
            let end = fade.timing(0.0, now_playing::OUT_MS, now_playing::OUT_EASE);
            spawn(async move {
                // 收起途中又打开了（动画被替换，返回 false）：保持打开。
                if end.await && full_closing.get_untracked() {
                    full_closing.set(false);
                    full_open.set(false);
                }
            });
        }) as Rc<dyn Fn()>
    };
    let _ = s;
    bench(open.clone());

    let nav = global::<Nav>();

    let queue_open = signal(false);
    let search = frame::SearchFocus::new();
    // 搜索页页顶的输入框（与侧栏框共用同一个 query，ElementRef 各一份）。
    let search_page = frame::SearchFocus::new();
    // 窗口隐藏到托盘时整个界面拆掉（视图、图片、列表都释放），只留播放与
    // 状态轮询；再次显示时重建，导航位置在全局信号里，不丢。
    // 侧栏与内容区一直铺到窗口顶边，标题栏透明地浮在上面；全屏播放盖住
    // 整个窗口（包括标题栏），只有系统的窗口按钮还在它上面。
    let shell = move || {
        let content = page(nav, search_page);
        let (open, close) = (open.clone(), close.clone());
        view()
            .flex_1()
            .child(
                view()
                    .flex_1()
                    .flex_row()
                    .child(frame::sidebar(search, search_page))
                    .child(
                        view()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(view().h(move || px(top_inset())).flex_shrink_0())
                            .child(view().flex_1().child(content))
                            .child(frame::bar::player_bar(open, queue_open)),
                    ),
            )
            .child(frame::title_bar())
            .child(when(
                move || full_open.get(),
                move || now_playing::full_player(fade, close.clone()),
            ))
    };

    let root = view()
        .flex_1()
        .bg(move || bg())
        .transition_colors()
        .duration(300.0)
        .text_color(fg())
        // 文字样式由里面的文字继承：整个界面跟着设置里的字体走。
        .font_family(store::font_family)
        // 点在搜索框以外的地方时让它失去焦点（否则焦点一直留在里面，
        // Space 等快捷键也被它吃掉）。
        .on_touch(move |e: &TouchEvent| {
            search.outside_touch(e);
            search_page.outside_touch(e);
        })
        // 右键事件只派发给最近的监听者：点到没有菜单的地方才轮到根——
        // 清掉「当前菜单」令牌，开着的菜单随之关闭（右键不产生
        // PressOutside，不这样处理旧菜单会一直留着）。
        .on_context_menu(move |_: &ContextMenuEvent| s.menu_open.set(None))
        .child(when(move || !s.hidden.get(), shell))
        // 弹窗按叠放顺序挂载：设置在最底，「关闭窗口」的询问盖过一切。
        .child(when(
            move || s.settings.get() && !s.hidden.get(),
            views::settings_dialog,
        ))
        .child(when(
            move || s.editor.with(Option::is_some),
            views::playlists::editor_dialog,
        ))
        .child(when(
            move || s.delete_ask.with(Option::is_some) && !s.hidden.get(),
            delete_prompt,
        ))
        .child(when(
            move || s.close_prompt.get() && !s.hidden.get(),
            close_prompt,
        ));
    // Windows 的窗口按钮在最上层（盖过全屏播放与弹窗，和系统按钮一样）。
    #[cfg(target_os = "windows")]
    let root = root.child(frame::caption::buttons(move || full_open.get()));
    root
}

fn page(nav: Signal<Nav>, search_page: frame::SearchFocus) -> AnyElement {
    let s = store::store();
    let search_mode = move || s.search_open.get() || !s.search.with(|q| q.trim().is_empty());
    // 输入框挂在 `dynamic` 外面：输入时只重建下面的结果，框本身不重建
    // （不然每个键击都把焦点弄丢）。
    view()
        .flex_1()
        .child(when(search_mode, move || {
            view()
                .px(px(24.0))
                .pt(px(20.0))
                .pb(px(4.0))
                .child(frame::search_box(search_page, false, true))
        }))
        .child(dynamic(move || {
            if search_mode() {
                return views::search_results().into_element();
            }
            match nav.get() {
                Nav::Folders => views::folders().into_element(),
                Nav::Artists => views::artists().into_element(),
                Nav::Albums => views::albums().into_element(),
                Nav::Genres => views::genres().into_element(),
                Nav::Playlist(id) => views::playlists::page(id).into_element(),
            }
        }))
        .into_element()
}

/// 第一次点关闭：最小化到托盘还是退出。应用内的弹窗（nsh-base `dialog`，
/// shadcn AlertDialog 的外观）；选择会记住（设置里可改）。取消、Esc 或点
/// 遮罩时不关窗口。
fn close_prompt() -> AnyElement {
    use crate::config::CloseAction;
    use components::button::{Size, Variant, text_button};
    let s = store::store();
    let cancel = move || s.close_prompt.set(false);
    let content = components::form::dialog_content(440.0)
        .child(components::form::dialog_header(
            "关闭窗口",
            Some("最小化到托盘后音乐继续播放，可以从托盘图标打开窗口、切歌或退出。这个选择会被记住，可以在「设置」里更改。"),
        ))
        .child(
            components::form::dialog_footer()
                .child(text_button(Variant::Outline, Size::Default, None, "取消", cancel))
                .child(text_button(Variant::Secondary, Size::Default, None, "退出 Zeo Player", || {
                    store::answer_close(CloseAction::Quit)
                }))
                .child(text_button(Variant::Default, Size::Default, None, "最小化到托盘", || {
                    store::answer_close(CloseAction::Tray)
                })),
        );
    components::form::modal(content, cancel)
}

/// 「删除文件」的确认弹窗：回收站 / 直接删（不可恢复）按设置里的「删除
/// 文件」选项决定。确认后后台线程做文件操作，成功再删曲库记录。
fn delete_prompt() -> AnyElement {
    use components::button::{Size, Variant, text_button};
    let s = store::store();
    let cancel = move || s.delete_ask.set(None);
    let permanent = s
        .delete_ask
        .with_untracked(|a| a.as_ref().is_some_and(|a| a.permanent));
    let (desc, confirm) = if permanent {
        (
            "将被永久删除（不经过回收站，无法恢复），曲库记录一并移除。",
            "永久删除",
        )
    } else {
        ("将被移到回收站，曲库记录一并移除。", "移到回收站")
    };
    let name = move || {
        s.delete_ask.with(|a| {
            a.as_ref()
                .and_then(|a| {
                    std::path::Path::new(&a.path)
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                })
                .unwrap_or_default()
        })
    };
    let content = components::form::dialog_content(440.0)
        .child(components::form::dialog_header("删除文件", Some(desc)))
        .child(
            text(name)
                .number_of_lines(1)
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(
            components::form::dialog_footer()
                .child(text_button(
                    Variant::Outline,
                    Size::Default,
                    None,
                    "取消",
                    cancel,
                ))
                .child(text_button(
                    Variant::Destructive,
                    Size::Default,
                    None,
                    confirm,
                    || store::confirm_delete_file(),
                )),
        );
    components::form::modal(content, cancel)
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
    // `scroll`：只跑浏览 + 滚网格，不放歌不开全屏（测列表滚动内存）。
    let scroll_only = std::env::var("ZEO_PLAYER_BENCH").as_deref() == Ok("scroll");
    let nav = global::<Nav>();
    spawn(async move {
        let step = |name: &str| eprintln!("ZEO_PLAYER_BENCH step {name}");
        let wait = |s: u64| timer(std::time::Duration::from_secs(s));
        // 等扫描结束。
        while store::store().library.with_untracked(|l| l.progress.scanning)
            || store::store().tracks.with_untracked(|t| t.is_empty())
        {
            wait(1).await;
        }
        step("idle");
        wait(6).await;
        for (target, name) in [
            (Nav::Artists, "artists"),
            (Nav::Genres, "genres"),
            (Nav::Albums, "albums"),
        ] {
            nav.set(target);
            step(name);
            wait(6).await;
        }
        // 专辑网格从头滚到底：每张封面都解码一遍（最坏情况）。
        if let Some(grid) = BENCH_GRID
            .with(|g| g.get())
            .filter(|_| std::env::var_os("ZEO_PLAYER_BENCH_NO_SCROLL").is_none())
        {
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
        if scroll_only {
            step("done");
            return;
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
    use crate::config::CloseAction;
    spawn(async move {
        let s = store::store();
        let step = |name: &str| {
            eprintln!(
                "ZEO_PLAYER_BENCH step {name} hidden={} prompt={}",
                crate::window::is_hidden(),
                s.close_prompt.get_untracked()
            )
        };
        let wait = |ms: u64| timer(std::time::Duration::from_millis(ms));
        while s.tracks.with_untracked(|t| t.is_empty()) {
            wait(500).await;
        }
        store::set_close_action(CloseAction::Ask);
        wait(3000).await;
        let tracks = s.tracks.get_untracked();
        store::play_list(&tracks, 0);
        wait(1500).await;
        crate::window::simulate_close();
        wait(1000).await;
        step("asked");
        store::answer_close(CloseAction::Tray);
        wait(4000).await;
        step("tray");
        eprintln!(
            "ZEO_PLAYER_BENCH playing={} time={:.1}",
            s.playback.with_untracked(|p| p.playing),
            s.playback.with_untracked(|p| p.time)
        );
        wait(6000).await;
        eprintln!(
            "ZEO_PLAYER_BENCH playing={} time={:.1}",
            s.playback.with_untracked(|p| p.playing),
            s.playback.with_untracked(|p| p.time)
        );
        crate::window::simulate_close();
        wait(500).await;
        step("closed-again-while-hidden");
        crate::window::simulate_tray(crate::window::TrayAction::Show);
        wait(2000).await;
        step("shown");
        // 已记住「托盘」：再点关闭直接隐藏，不再询问。
        crate::window::simulate_close();
        wait(1500).await;
        step("remembered");
        store::set_close_action(CloseAction::Ask);
        wait(500).await;
        eprintln!("ZEO_PLAYER_BENCH step quit");
        crate::window::quit();
    });
}
