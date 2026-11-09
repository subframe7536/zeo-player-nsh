use std::cell::RefCell;

use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::window::notify;

struct Tray {
    icon: TrayIcon,
    toggle: MenuItem,
    prev: MenuItem,
    next: MenuItem,
    /// 播放 / 暂停两个字形图标（左键点图标切播放暂停，图标跟状态换）。
    play: Option<Icon>,
    pause: Option<Icon>,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
    /// 上次应用到托盘的状态，相同就跳过系统调用（store 的 memo 已经
    /// 拦掉进度类高频刷新，这层兜底重复调用）。
    static LAST: RefCell<Option<TrayState>> = const { RefCell::new(None) };
}

/// 托盘图标：播放 / 暂停按钮的同一图形（`icons::play_pause_svg`——
/// 同一组顶点 + 同款圆角描边）交给 `nsh_core::svg` 光栅化。macOS 是
/// 模板图（只取 alpha，系统按菜单栏明暗反色成黑白）；Windows 的 svg
/// 在字形下垫了应用图标同款的浅绿渐变圆角方块（`tile`），深浅任务栏
/// 都可见。
fn glyph_icon(pause: bool) -> Option<Icon> {
    // 44px：菜单栏 22pt @2x（tray-icon 会把更高的缩到 22pt）；
    // 64px：Windows 按 DPI 缩到 16–32px。
    let size = if cfg!(target_os = "macos") { 44 } else { 64 };
    // 图标旁不再排文字，图形居中。
    let dy = 0.0;
    // macOS 字形缩到格子的 80%（满格顶着菜单栏上下沿显挤）；
    // Windows 字形缩到 3/4，底砖保持满幅。
    let scale = if cfg!(target_os = "macos") {
        0.8
    } else {
        3.0 / 4.0
    };
    let svg = zeo_icons::play_pause_svg(pause, cfg!(target_os = "windows"), dy, scale);
    let raster = nsh_core::svg::rasterize(svg.as_bytes(), size).ok()?;
    // Raster 是预乘 RGBA，`Icon::from_rgba` 要非预乘——半透明边缘不
    // 除回 alpha 会发黑。macOS 模板图只看 alpha，RGB 给黑即可。
    let mut rgba = raster.pixels;
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a == 0 {
            continue;
        }
        if cfg!(target_os = "macos") {
            px[..3].fill(0);
        } else {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Icon::from_rgba(rgba, size, size).ok()
}

/// 在托盘所属线程上创建（macOS 主线程；Windows 托盘线程）。
pub(crate) fn create() {
    let menu = Menu::new();
    let toggle = MenuItem::with_id("toggle", "播放", false, None);
    let prev = MenuItem::with_id("prev", "上一首", false, None);
    let next = MenuItem::with_id("next", "下一首", false, None);
    let show = MenuItem::with_id("show", "显示 Zeo Player", true, None);
    let quit = MenuItem::with_id("quit", "退出 Zeo Player", true, None);
    let _ = menu.append_items(&[
        &toggle,
        &prev,
        &next,
        &PredefinedMenuItem::separator(),
        &show,
        &quit,
    ]);
    let (play, pause) = (glyph_icon(false), glyph_icon(true));
    // 左键点图标 = 播放 / 暂停（`TrayIconEvent` 里处理），右键出菜单。
    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Zeo Player")
        .with_menu_on_left_click(false);
    if let Some(icon) = play.clone() {
        #[cfg(target_os = "macos")]
        {
            builder = builder.with_icon_templated(icon);
        }
        #[cfg(target_os = "windows")]
        {
            builder = builder.with_icon(icon);
        }
    }
    match builder.build() {
        Ok(icon) => TRAY.with(|t| {
            *t.borrow_mut() = Some(Tray {
                icon,
                toggle,
                prev,
                next,
                play,
                pause,
            })
        }),
        Err(err) => eprintln!("zeo-player: 托盘图标创建失败：{err}"),
    }
}

/// 移除托盘图标（`TrayIcon` 析构时从通知区删除）。
#[cfg(target_os = "windows")]
pub(crate) fn remove() {
    TRAY.with(|t| drop(t.borrow_mut().take()));
    LAST.with(|l| *l.borrow_mut() = None);
}

pub(crate) fn apply(state: &TrayState) {
    if LAST.with(|l| l.borrow().as_ref() == Some(state)) {
        return;
    }
    LAST.with(|l| *l.borrow_mut() = Some(state.clone()));
    TRAY.with(|t| {
        let t = t.borrow();
        let Some(t) = t.as_ref() else {
            return;
        };
        t.toggle
            .set_text(if state.playing { "暂停" } else { "播放" });
        for item in [&t.toggle, &t.prev, &t.next] {
            item.set_enabled(state.has_track);
        }
        let _ = t.icon.set_tooltip(Some(tray_tooltip(state)));
        // 图标即播放 / 暂停按钮：显示的是动作（播放中给暂停键）。
        let glyph = if state.playing { &t.pause } else { &t.play };
        if let Some(glyph) = glyph.clone() {
            #[cfg(target_os = "macos")]
            let _ = t.icon.set_icon_templated(Some(glyph));
            #[cfg(target_os = "windows")]
            let _ = t.icon.set_icon(Some(glyph));
        }
    });
}

/// 托盘菜单的操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    Toggle,
    Previous,
    Next,
    Quit,
}

/// 托盘上显示的状态。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrayState {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub has_track: bool,
}

/// 「曲名 — 歌手」（无歌手只留曲名）：菜单行与悬停提示共用。
#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn track_label(s: &TrayState) -> String {
    if s.artist.is_empty() {
        s.title.clone()
    } else {
        format!("{} — {}", s.title, s.artist)
    }
}

/// 悬停提示：有曲目是「曲名 — 歌手」，没有就回到应用名。
#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn tray_tooltip(s: &TrayState) -> String {
    if s.has_track {
        track_label(s)
    } else {
        "Zeo Player".to_string()
    }
}

static TRAY_ACTIONS: std::sync::Mutex<Vec<TrayAction>> = std::sync::Mutex::new(Vec::new());

/// 取走托盘上发生的操作（菜单项、Windows 上单击图标）。
pub fn take_tray_actions() -> Vec<TrayAction> {
    std::mem::take(&mut *TRAY_ACTIONS.lock().unwrap())
}

/// 测量脚本用：像托盘菜单那样发出一个操作（macOS 在主线程上，与菜单
/// 回调相同）。
#[cfg(any(debug_assertions, test))]
pub fn simulate_tray(action: TrayAction) {
    #[cfg(target_os = "macos")]
    dispatch2::DispatchQueue::main().exec_async(move || push_tray(action));
    #[cfg(not(target_os = "macos"))]
    push_tray(action);
}

fn push_tray(action: TrayAction) {
    TRAY_ACTIONS.lock().unwrap().push(action);
    notify();
}

/// 托盘事件直接回调（不进 tray-icon 的通道），每个事件唤醒界面。
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn install_tray_handlers() {
    tray_icon::menu::MenuEvent::set_event_handler(Some(|event: tray_icon::menu::MenuEvent| {
        let action = match event.id.0.as_str() {
            "show" => TrayAction::Show,
            "toggle" => TrayAction::Toggle,
            "prev" => TrayAction::Previous,
            "next" => TrayAction::Next,
            "quit" => TrayAction::Quit,
            _ => return,
        };
        push_tray(action);
    }));
    tray_icon::TrayIconEvent::set_event_handler(Some(
        |event: tray_icon::TrayIconEvent| match event {
            // 左键点图标 = 播放 / 暂停（两平台一致；右键出菜单，
            // `menu_on_right_click` 默认开）。不做双击判别——单击即时
            // toggle，快速双击就是两次 toggle（净状态不变）。
            tray_icon::TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            } => push_tray(TrayAction::Toggle),
            _ => {}
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str, artist: &str) -> TrayState {
        TrayState {
            title: title.into(),
            artist: artist.into(),
            playing: true,
            has_track: true,
        }
    }

    #[test]
    fn tray_tooltip_shows_label_or_app_name() {
        assert_eq!(tray_tooltip(&track("歌名", "歌手")), "歌名 — 歌手");
        assert_eq!(tray_tooltip(&track("歌名", "")), "歌名");
        assert_eq!(tray_tooltip(&TrayState::default()), "Zeo Player");
    }
}
