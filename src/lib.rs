//! ZeoPlayer：本地优先的桌面音乐播放器，纯 Rust NSH 应用。
//! 引擎：lofty 标签、symphonia 解码、cpal 输出、playwire 系统媒体会话、
//! LRC 解析、波形与多根曲库；界面根组件为 [`App`]（领域层在 `crates/`
//! 下的 zeo-* 各 crate）。

#[cfg(test)]
mod e2e;

use std::rc::Rc;

use nsh_core::desktop::{TitleBarOptions, TitleBarStyle, WindowAttributes};
use nsh_core::element::desktop::{current_window, main_window};
use nsh_core::prelude::*;

use zeo_ui_store::theme::*;

// 侧栏去向与测量用的网格句柄在 zeo_ui_store（下层 crate 也要读）。

// 测试（tests/、e2e）与外部照旧用 `zeo_player::config` / `engine` 等路径。
pub use zeo_db as db;
pub use zeo_engine as engine;
pub use zeo_library::{config, provider};
pub use zeo_metadata as metadata;
pub use zeo_playlists as playlists;
pub use zeo_shell::{single_instance, window};

#[cfg(any(debug_assertions, test))]
pub use zeo_ui_store::BENCH_GRID;
pub use zeo_ui_store::Nav;

/// 应用根组件（`nsh.json` `"root"`）。
#[component]
pub fn App() -> impl IntoElement {
    // 单实例兜底：真正的检查在 `native()`（窗口创建之前），这里 `Once`
    // 幂等，防不走 `native()` 的挂载路径。
    zeo_shell::single_instance::enforce();
    let s = zeo_ui_store::install();
    let mut title_bar = TitleBarOptions::default();
    title_bar.style = TitleBarStyle::Hidden;
    title_bar.height = Some(px(TITLE_BAR));
    let mut attributes = WindowAttributes::default();
    attributes.title_bar = Some(title_bar);
    attributes.min_size = Some(Some(size(
        px(zeo_shell::MIN_WIDTH),
        px(zeo_shell::MIN_HEIGHT),
    )));
    // 点关闭不直接关窗：先到 `close_requested`（询问 / 最小化到托盘时窗口
    // 留着），真要退出经 `desktop::quit`（与 Cmd+Q / 退出菜单同一条收尾
    // 路径，不再回到这里）。
    attributes.close_on_press = Some(false);
    current_window().set(attributes);
    main_window().on_close_press(|_| {
        if zeo_shell::close_requested() {
            nsh_core::element::desktop::quit();
        }
    });
    #[cfg(target_os = "windows")]
    zeo_ui_frame::caption::install();

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
            s.full_screen.set(true);
            global::<zeo_ui_now_playing::Reveal>().update(|r| r.0 = r.0.wrapping_add(1));
            if untracked(reduce_motion) {
                fade.set(1.0);
            } else {
                let _ = fade.timing(1.0, zeo_ui_now_playing::IN_MS, zeo_ui_now_playing::IN_EASE);
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
                s.full_screen.set(false);
                return;
            }
            let end = fade.timing(
                0.0,
                zeo_ui_now_playing::OUT_MS,
                zeo_ui_now_playing::OUT_EASE,
            );
            spawn(async move {
                // 收起途中又打开了（动画被替换，返回 false）：保持打开。
                if end.await && full_closing.get_untracked() {
                    full_closing.set(false);
                    full_open.set(false);
                    s.full_screen.set(false);
                }
            });
        }) as Rc<dyn Fn()>
    };
    // 外部（快捷键 / 命令面板）写的是 store 的期望状态，这里对账成真实
    // 开合；open / close 里同步回写它，信号按值通知，不会有回声。
    {
        let (open, close) = (open.clone(), close.clone());
        effect(move || if s.full_screen.get() { open() } else { close() });
    }
    let _ = s;
    #[cfg(any(debug_assertions, test))]
    bench(open.clone());

    let nav = global::<Nav>();

    let queue_open = signal(false);
    // 窗口隐藏到托盘时整个界面拆掉（视图、图片、列表都释放），只留播放与
    // 状态轮询；再次显示时重建，导航位置在全局信号里，不丢。
    // 侧栏与内容区一直铺到窗口顶边，标题栏透明地浮在上面；全屏播放盖住
    // 整个窗口（包括标题栏），只有系统的窗口按钮还在它上面。
    let shell = move || {
        let content = page(nav);
        let (open, close) = (open.clone(), close.clone());
        view()
            .flex_1()
            .child(
                view()
                    .flex_1()
                    .flex_row()
                    .child(zeo_ui_frame::sidebar())
                    .child(
                        view()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(view().h(move || px(top_inset())).flex_shrink_0())
                            .child(view().flex_1().child(content))
                            .child(zeo_ui_frame::bar::player_bar(open, queue_open)),
                    ),
            )
            .child(zeo_ui_views::folders::drag_ghost())
            .child(zeo_ui_frame::title_bar())
            .child(when(
                move || full_open.get(),
                move || zeo_ui_now_playing::full_player(fade, close.clone()),
            ))
    };

    let root = view()
        .flex_1()
        .bg(move || bg())
        .transition_colors()
        .duration(300.0)
        .text_color(fg())
        // 文字样式由里面的文字继承：整个界面跟着设置里的字体走。
        .font_family(zeo_ui_store::font_family)
        .child(when(move || !s.hidden.get(), shell))
        // 弹窗按叠放顺序挂载：设置在最底，「关闭窗口」的询问盖过一切。
        .child(when(
            move || s.settings.get() && !s.hidden.get(),
            zeo_ui_views::settings_dialog,
        ))
        .child(when(
            move || s.search_open.get() && !s.hidden.get(),
            zeo_ui_views::command_palette_dialog,
        ))
        .child(when(
            move || s.editor.with(Option::is_some),
            zeo_ui_views::playlists::editor_dialog,
        ))
        .child(when(
            move || s.edit_info.with(Option::is_some),
            zeo_ui_views::edit_info_dialog,
        ))
        .child(when(
            move || s.folder_op.with(Option::is_some) && !s.hidden.get(),
            zeo_ui_views::folders::op_dialog,
        ))
        .child(when(
            move || s.delete_ask.with(Option::is_some) && !s.hidden.get(),
            delete_prompt,
        ))
        .child(when(
            move || s.close_prompt.get() && !s.hidden.get(),
            close_prompt,
        ))
        .child(keys());
    // Windows 的窗口按钮在最上层（盖过全屏播放与弹窗，和系统按钮一样）。
    #[cfg(target_os = "windows")]
    let root = root.child(zeo_ui_frame::caption::buttons(move || full_open.get()));
    root
}

/// 全局快捷键：动作与键位在 `zeo_ui_store::keybinds`（默认见
/// `KeyAction::default_key`，可在「设置 → 快捷键」改）。绑在读
/// `s.keybinds` 的 `dynamic` 里——换绑写信号、dynamic 换 owner，旧绑定
/// 连 owner 销毁、按新键位重绑。全屏播放页的键在 `ui-now-playing`。
///
/// `mod`：macOS 是 ⌘，Windows 是 Ctrl。文本框聚焦时编辑键（裸字符、
/// 箭头、空格）按 `When::Auto` 自动放行不触发——所以默认键位里裸方向
/// 键只出现在全屏页，全局只绑 `mod` 组合；裸字母也不默认绑，select /
/// listbox 的键入定位要用（用户仍可自绑，是自己的选择）。
fn keys() -> AnyElement {
    dynamic(move || {
        for &action in zeo_ui_store::KeyAction::ALL {
            if !action.in_full_player() {
                zeo_ui_store::bind_key(action, move || zeo_ui_store::run_keybind(action));
            }
        }
        view()
    })
}

fn page(nav: Signal<Nav>) -> AnyElement {
    view()
        .flex_1()
        .child(dynamic(move || match nav.get() {
            Nav::Home => zeo_ui_views::home().into_element(),
            Nav::Library => zeo_ui_views::library().into_element(),
            Nav::Metadata => zeo_ui_views::metadata::page().into_element(),
            Nav::Folders => zeo_ui_views::folders().into_element(),
            Nav::Artists => zeo_ui_views::artists().into_element(),
            Nav::Albums => zeo_ui_views::albums().into_element(),
            Nav::Genres => zeo_ui_views::genres().into_element(),
            Nav::Playlist(id) => zeo_ui_views::playlists::page(id).into_element(),
        }))
        .into_element()
}

/// 第一次点关闭：最小化到托盘还是退出。应用内的弹窗（nsh-base `dialog`，
/// shadcn AlertDialog 的外观）；选择会记住（设置里可改）。取消、Esc 或点
/// 遮罩时不关窗口。
fn close_prompt() -> AnyElement {
    use zeo_types::config::CloseAction;
    use zeo_ui_components::button::{Size, Variant, text_button};
    let s = zeo_ui_store::store();
    let cancel = move || s.close_prompt.set(false);
    let content = zeo_ui_components::form::dialog_content(440.0)
        .child(zeo_ui_components::form::dialog_header(
            "关闭窗口",
            Some("最小化到托盘后音乐继续播放，可以从托盘图标打开窗口、切歌或退出。这个选择会被记住，可以在「设置」里更改。"),
        ))
        .child(
            zeo_ui_components::form::dialog_footer()
                .child(text_button(Variant::Outline, Size::Default, None, "取消", cancel))
                .child(text_button(Variant::Secondary, Size::Default, None, "退出 Zeo Player", || {
                    zeo_ui_store::answer_close(CloseAction::Quit)
                }))
                .child(text_button(Variant::Default, Size::Default, None, "最小化到托盘", || {
                    zeo_ui_store::answer_close(CloseAction::Tray)
                })),
        );
    zeo_ui_components::form::modal(content, cancel)
}

/// 「删除文件」的确认弹窗：回收站 / 直接删（不可恢复）按设置里的「删除
/// 文件」选项决定。确认后后台线程做文件操作，成功再删曲库记录。
fn delete_prompt() -> AnyElement {
    use zeo_ui_components::button::{Size, Variant, text_button};
    let s = zeo_ui_store::store();
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
            let Some(a) = a.as_ref() else {
                return String::new();
            };
            if a.paths.len() == 1 {
                std::path::Path::new(&a.paths[0])
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else {
                format!("选中的 {} 个文件", a.paths.len())
            }
        })
    };
    let content = zeo_ui_components::form::dialog_content(440.0)
        .child(zeo_ui_components::form::dialog_header(
            "删除文件",
            Some(desc),
        ))
        .child(
            text(name)
                .number_of_lines(1)
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(
            zeo_ui_components::form::dialog_footer()
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
                    || zeo_ui_store::confirm_delete_file(),
                )),
        );
    zeo_ui_components::form::modal(content, cancel)
}

/// 内存测量用的脚本（`ZEO_PLAYER_BENCH=1` 启动时运行，平时不做任何事）：依次打开
/// 各视图、在专辑页停留、播放并打开全屏、连续切 20 首，每步之间留时间测量。
/// 进度写到标准错误：`ZEO_PLAYER_BENCH step <名称>`。只在 debug / 测试构建里
/// 编译——生产 release 里环境变量无效、代码不存在。
#[cfg(any(debug_assertions, test))]
fn bench(open_full: Rc<dyn Fn()>) {
    if std::env::var_os("ZEO_PLAYER_BENCH").is_none() {
        return;
    }
    if std::env::var("ZEO_PLAYER_BENCH").as_deref() == Ok("close") {
        return bench_close();
    }
    // `hidden`：放歌后隐藏窗口（后台播放），每 30 秒打一次播放状态
    // 与进程内存到 stderr——泄漏 / 内存增长的观察台。
    if std::env::var("ZEO_PLAYER_BENCH").as_deref() == Ok("hidden") {
        spawn(async move {
            let wait = |s: u64| timer(std::time::Duration::from_secs(s));
            while zeo_ui_store::store()
                .library
                .with_untracked(|l| l.progress.scanning)
                || zeo_ui_store::store()
                    .tracks
                    .with_untracked(|t| t.is_empty())
            {
                wait(1).await;
            }
            let tracks = zeo_ui_store::store().tracks.get_untracked();
            zeo_ui_store::play_list(&tracks, 0);
            wait(2).await;
            zeo_shell::hide();
            eprintln!("ZEO_PLAYER_BENCH step hidden");
            let mem = || {
                // macOS 是（物理足迹, 驻留），Windows 是（私有字节, 工作集）。
                let (a, b) = zeo_shell::memory_usage();
                let (a, b) = (a as f64 / 1048576.0, b as f64 / 1048576.0);
                if cfg!(target_os = "macos") {
                    format!("footprint={a:.1}MB rss={b:.1}MB")
                } else {
                    format!("priv={a:.1}MB ws={b:.1}MB")
                }
            };
            loop {
                wait(30).await;
                let st = zeo_ui_store::player().state();
                eprintln!(
                    "ZEO_PLAYER_BENCH hidden playing={} time={:.1} {} track={:?}",
                    st.is_playing,
                    st.current_time,
                    mem(),
                    st.track.path
                );
            }
        });
        return;
    }
    // `scroll`：只跑浏览 + 滚网格，不放歌不开全屏（测列表滚动内存）。
    let scroll_only = std::env::var("ZEO_PLAYER_BENCH").as_deref() == Ok("scroll");
    let nav = global::<Nav>();
    spawn(async move {
        let step = |name: &str| eprintln!("ZEO_PLAYER_BENCH step {name}");
        let wait = |s: u64| timer(std::time::Duration::from_secs(s));
        // 等扫描结束。
        while zeo_ui_store::store()
            .library
            .with_untracked(|l| l.progress.scanning)
            || zeo_ui_store::store()
                .tracks
                .with_untracked(|t| t.is_empty())
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
        let tracks = zeo_ui_store::store().tracks.get_untracked();
        zeo_ui_store::play_list(&tracks, 0);
        open_full();
        step("full");
        wait(8).await;
        for i in 0..20 {
            zeo_ui_store::player().next();
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
#[cfg(any(debug_assertions, test))]
fn bench_close() {
    use zeo_types::config::CloseAction;
    spawn(async move {
        let s = zeo_ui_store::store();
        let step = |name: &str| {
            eprintln!(
                "ZEO_PLAYER_BENCH step {name} hidden={} prompt={}",
                zeo_shell::is_hidden(),
                s.close_prompt.get_untracked()
            )
        };
        let wait = |ms: u64| timer(std::time::Duration::from_millis(ms));
        while s.tracks.with_untracked(|t| t.is_empty()) {
            wait(500).await;
        }
        zeo_ui_store::set_close_action(CloseAction::Ask);
        wait(3000).await;
        let tracks = s.tracks.get_untracked();
        zeo_ui_store::play_list(&tracks, 0);
        wait(1500).await;
        zeo_shell::simulate_close();
        wait(1000).await;
        step("asked");
        zeo_ui_store::answer_close(CloseAction::Tray);
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
        zeo_shell::simulate_close();
        wait(500).await;
        step("closed-again-while-hidden");
        zeo_shell::simulate_tray(zeo_shell::TrayAction::Show);
        wait(2000).await;
        step("shown");
        // 已记住「托盘」：再点关闭直接隐藏，不再询问。
        zeo_shell::simulate_close();
        wait(1500).await;
        step("remembered");
        zeo_ui_store::set_close_action(CloseAction::Ask);
        wait(500).await;
        eprintln!("ZEO_PLAYER_BENCH step quit");
        // 走托盘「退出」：重新显示后界面轮询（托盘派发）还活着才退得掉。
        zeo_shell::simulate_tray(zeo_shell::TrayAction::Quit);
    });
}

/// The crate's UI set (`nsh.json` `"rust"`).
#[native_module("zeo-player/native")]
fn native(registry: &mut Registry) {
    // 打包 runner 在 `nsh_windows_app::main()` / `nsh_macos_app::main()`
    // 之前调这里（`set_ui_sets` 的参数）：Windows 的判重已由 runner 的
    // `singleInstance` 在窗口创建前做完；macOS 的第二份实例在这个点
    // 退出，连 NSH 的窗口都不会创建，不会闪一下。
    zeo_shell::single_instance::enforce();
    registry.register::<App>();
}
