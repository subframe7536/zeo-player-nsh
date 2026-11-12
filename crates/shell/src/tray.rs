//! 系统托盘：0.5.0 起走上游 `element::desktop` 的 `set_tray` / `Tray`
//!（`install` 的响应式 effect 按 [`TrayState`] 调 [`tray_for`] 重设整
//! 个托盘）。图标是播放 / 暂停按钮的同一图形写成 SVG
//!（`ImageSrc::svg_markup`）交给上游：macOS 当模板图按系统文字色绘
//! 制；上游在 Windows 上也把 SVG 当模板（任务栏文字色），彩色垫砖图
//! 标要自己光栅化走 `ImageSrc::rgba` 位图源（svg 里垫应用图标同款的
//! 渐变圆角方块）。左键点图标 = 播放 / 暂停（`Tray::on_click`，上游
//! 只吃主键），右键出菜单。菜单项与图标的动作经 `push_tray` 排队、
//! [`take_tray_actions`] 由界面轮询统一派发——`simulate_tray`（测量脚
//! 本）与真实菜单走同一条路。

use nsh_core::element::ImageSrc;
use nsh_core::element::desktop::{MenuItem, Tray, tray};

use crate::window::notify;

/// 按当前状态构造托盘：图标字形（显示的是动作——播放中给暂停键）、
/// 悬停提示与菜单。上游的 `set_tray` 是整体替换，状态变化时重调
/// [`tray_for`] 即可，菜单文案与禁用态随重建更新。
pub fn tray_for(state: &TrayState) -> Tray {
    // 图标字形缩到格子的 80%（macOS）/ 3/4（Windows）：满格顶着显挤，
    // 底砖保持满幅。
    let scale = if cfg!(target_os = "macos") {
        0.8
    } else {
        3.0 / 4.0
    };
    let off = !state.has_track;
    let mut tray = tray()
        .tooltip(tray_tooltip(state))
        .menu([
            MenuItem::new(if state.playing { "暂停" } else { "播放" })
                .disabled(off)
                .on_select(|| push_tray(TrayAction::Toggle)),
            MenuItem::new("上一首")
                .disabled(off)
                .on_select(|| push_tray(TrayAction::Previous)),
            MenuItem::new("下一首")
                .disabled(off)
                .on_select(|| push_tray(TrayAction::Next)),
            MenuItem::separator(),
            MenuItem::new("显示 Zeo Player").on_select(|| push_tray(TrayAction::Show)),
            MenuItem::new("退出 Zeo Player").on_select(|| push_tray(TrayAction::Quit)),
        ])
        .on_click(|_| push_tray(TrayAction::Toggle));
    if let Some(icon) = tray_icon(state, scale) {
        tray = tray.icon(icon);
    }
    tray
}

/// 托盘图标源：macOS 交 SVG 给上游当模板图；上游在 Windows 上也把
/// SVG 当模板（按任务栏文字色绘制），彩色垫砖要自己光栅化走位图
/// 源——64px、`ImageData` 要非预乘 RGBA（`rasterize` 出的是预乘，
/// 逐像素除回 alpha，半透明边缘不除会发黑）。
fn tray_icon(state: &TrayState, scale: f32) -> Option<ImageSrc> {
    if cfg!(target_os = "windows") {
        let svg = zeo_icons::play_pause_svg(state.playing, true, 0.0, scale);
        let raster = nsh_core::svg::rasterize(svg.as_bytes(), 64).ok()?;
        let mut rgba = raster.pixels;
        for px in rgba.chunks_exact_mut(4) {
            let a = px[3] as u32;
            if a == 0 {
                continue;
            }
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
        Some(ImageSrc::rgba(64, 64, rgba))
    } else {
        Some(ImageSrc::svg_markup(&zeo_icons::play_pause_svg(
            state.playing,
            false,
            0.0,
            scale,
        )))
    }
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
fn track_label(s: &TrayState) -> String {
    if s.artist.is_empty() {
        s.title.clone()
    } else {
        format!("{} — {}", s.title, s.artist)
    }
}

/// 悬停提示：有曲目是「曲名 — 歌手」，没有就回到应用名。
fn tray_tooltip(s: &TrayState) -> String {
    if s.has_track {
        track_label(s)
    } else {
        "Zeo Player".to_string()
    }
}

static TRAY_ACTIONS: std::sync::Mutex<Vec<TrayAction>> = std::sync::Mutex::new(Vec::new());

/// 取走托盘上发生的操作（菜单项、单击图标）。
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
