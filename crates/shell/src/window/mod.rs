//! 窗口与托盘：拦截关闭（最小化到托盘或退出）、隐藏 / 显示窗口、托盘图标
//! 与菜单。托盘图标本身是播放 / 暂停按钮：左键切播放暂停、右键开菜单；
//! macOS 是模板图（黑白、图标旁带曲名），Windows 是应用图标同款的外浅
//! 内深（浅绿渐变底 + 深绿字形，悬停提示显示曲名 — 歌手）。
//!
//! 关闭拦截走 NSH：`App()` 给主窗口设 `close_on_press(false)` 并注册
//! `on_close_press`——用户点关闭先到 [`close_requested`]，[`CloseAction`]
//! 决定做什么；`Ask` 时不关闭，只把 [`take_close_request`] 置位，由界面
//! 弹出选择；确定退出经 NSH 的 `desktop::quit()`（与 Cmd+Q / 退出菜单同
//! 一条收尾路径）。「记住窗口大小和位置」也走 NSH：界面侧订阅
//! `WindowRef::state`，`placement` 变化写进配置，启动时用 `set_placement`
//! 恢复（上游按现有显示器校正）。
//!
//! 余下的平台代码：
//!
//! - macOS：Dock 图标「重新打开」还靠替换 `applicationShouldHandleReopen:`
//!   的实现（NSH 不上报这个事件）；显示 / 隐藏用 `makeKeyAndOrderFront` /
//!   `orderOut`；托盘创建也挂在找到主窗口时。全部在主线程（GCD 主队列）。
//! - Windows：子类化 NSH 窗口（类名 `NshWindow`）的窗口过程，接管去掉系统
//!   标题栏的 `WM_NCCALCSIZE` 与顶边缩放的 `WM_NCHITTEST`（窗口按钮由应用
//!   画，NSH 的标题栏样式切到 Default 后就不画它自己的了），窗口销毁后
//!   直接结束进程（见 `exit_now`）；托盘在自己的线程上，带消息循环。

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use zeo_types::config::CloseAction;

/// 窗口的最小宽度（点）：侧栏固定 232 pt 展开，最小宽度保住内容区
/// 768 pt——曲目表「专辑」列的下限（`components::tracks::fits_album`
/// 断点 760）加页边距刚好放下。经 NSH 的 `WindowAttributes::min_size`
/// 生效（`App()` 里随标题栏样式一起设，内容为点）。
pub const MIN_WIDTH: f32 = 1000.0;

/// 窗口的最小高度（点）。
pub const MIN_HEIGHT: f32 = 720.0;

/// 当前关闭行为（界面设置写入）。
static MODE: AtomicU8 = AtomicU8::new(0);
/// 窗口里点了关闭、需要问用户。
static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);
/// 真的要退出（托盘「退出」、选择「退出」）：关闭不再拦截。
static QUITTING: AtomicBool = AtomicBool::new(false);
/// 窗口已隐藏到托盘。
static HIDDEN: AtomicBool = AtomicBool::new(false);
/// 重新打开时恢复上次的窗口大小与位置（界面设置写入）。
static REMEMBER: AtomicBool = AtomicBool::new(false);
/// 窗口事件（关闭请求、托盘操作、显示 / 隐藏）发生时唤醒界面
/// （[`changed`]），界面不必轮询。
static CHANGED: tokio::sync::Notify = tokio::sync::Notify::const_new();
/// 托盘上发生、还没被界面取走的操作。

/// 等到下一个窗口事件（之前错过的也算：`Notify` 存一个许可）。
pub async fn changed() {
    CHANGED.notified().await;
}

pub(crate) fn notify() {
    CHANGED.notify_one();
}

/// 窗口被系统重新显示（macOS 点 Dock 图标，Windows 任务栏 / Alt+Tab）而
/// 不是走 [`show`]：同样算显示，界面重新装上（否则窗口是空的）。
fn shown_by_system() {
    if HIDDEN.swap(false, Ordering::Relaxed) {
        notify();
    }
}

fn mode() -> CloseAction {
    match MODE.load(Ordering::Relaxed) {
        1 => CloseAction::Tray,
        2 => CloseAction::Quit,
        _ => CloseAction::Ask,
    }
}

/// 设置「记住窗口大小和位置」：只记录开关——位置的上报与写回、启动时
/// 的恢复都由界面侧的窗口状态效果做（`ui::store` 里订阅
/// `WindowRef::state` 的效果）；关掉时已存的位置由
/// `config::set_remember_frame` 清掉。
pub fn set_remember_frame(on: bool) {
    REMEMBER.store(on, Ordering::Relaxed);
}

/// 「记住窗口大小和位置」开关（界面设置经 [`set_remember_frame`] 写入）。
pub fn remembered() -> bool {
    REMEMBER.load(Ordering::Relaxed)
}

pub fn set_close_action(action: CloseAction) {
    MODE.store(
        match action {
            CloseAction::Ask => 0,
            CloseAction::Tray => 1,
            CloseAction::Quit => 2,
        },
        Ordering::Relaxed,
    );
}

/// 有没有待回答的关闭请求（读取后清除）。
pub fn take_close_request() -> bool {
    CLOSE_REQUESTED.swap(false, Ordering::Relaxed)
}

pub fn is_hidden() -> bool {
    HIDDEN.load(Ordering::Relaxed)
}

/// 用户点了关闭（NSH 的 `on_close_press`、测量脚本的 [`simulate_close`]）：
/// 返回 true 表示照常关闭（退出）。
pub fn close_requested() -> bool {
    if QUITTING.load(Ordering::Relaxed) {
        return true;
    }
    match mode() {
        CloseAction::Quit => true,
        CloseAction::Tray => {
            hide();
            false
        }
        CloseAction::Ask => {
            CLOSE_REQUESTED.store(true, Ordering::Relaxed);
            notify();
            false
        }
    }
}

/// 测量脚本用：像点了窗口的关闭按钮（主线程上，不经 NSH 的 `performClose`：
/// 那会在 NSH 应用界面变更的途中跑嵌套的事件循环）。
#[cfg(any(debug_assertions, test))]
pub fn simulate_close() {
    #[cfg(target_os = "macos")]
    dispatch2::DispatchQueue::main().exec_async(|| {
        if close_requested() {
            quit();
        }
    });
    #[cfg(not(target_os = "macos"))]
    if close_requested() {
        quit();
    }
}

/// 启动：挂上关闭拦截、创建托盘图标。可以多次调用（只做一次）。
pub fn start() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::single_instance::enforce();
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    crate::tray::install_tray_handlers();
    platform::start();
}

/// 隐藏窗口（播放继续）。
pub fn hide() {
    HIDDEN.store(true, Ordering::Relaxed);
    notify();
    nsh_core::element::desktop::main_window().hide();
}

/// 显示并激活窗口。
pub fn show() {
    HIDDEN.store(false, Ordering::Relaxed);
    notify();
    let window = nsh_core::element::desktop::main_window();
    window.show();
    window.focus();
}

/// 退出应用（结束播放）：与 Cmd+Q / 退出菜单同一条收尾路径
/// （NSH `desktop::quit` 不再回到关闭拦截）。
pub fn quit() {
    QUITTING.store(true, Ordering::Relaxed);
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    nsh_core::element::desktop::quit();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    std::process::exit(0);
}

/// 更新托盘（当前曲目、播放 / 暂停）：图标字形、悬停提示、菜单、
/// macOS 的菜单栏文字。
pub fn update_tray(state: crate::tray::TrayState) {
    platform::update(state);
}

/// 把空闲内存还给系统（隐藏到托盘后调用）：界面拆掉释放的堆页
/// 一直留在进程工作集 / malloc 分区里，任务管理器 / 活动监视器的
/// 数值不会降，要手动裁剪。它只回收**调用时已经空闲**的页——隐藏
/// 期间后台线程新产生的垃圾要再调一次才走，所以隐藏状态下由界面
/// 侧周期重复调用（`store` 的隐藏清理循环）。
pub fn trim_memory() {
    platform::trim_memory();
}

/// Windows 的窗口句柄（系统媒体控件要用）；找到窗口前为 `None`。
#[cfg(target_os = "windows")]
pub fn hwnd() -> Option<u64> {
    platform::HWND_VALUE.get().copied()
}

/// 进程内存采样，测量脚本用：macOS 是（物理足迹, 驻留大小），Windows
/// 是（私有字节, 工作集）；其它平台未实现，返回 (0, 0)。
#[cfg(any(debug_assertions, test))]
pub fn memory_usage() -> (u64, u64) {
    platform::memory_usage()
}

/// Windows 的窗口按钮由应用画（`ui::caption`），这里是它们的操作与状态。
#[cfg(target_os = "windows")]
pub use platform::{caption_ready, close, is_full_screen, is_maximized, minimize, toggle_maximize};

pub use crate::tray::{TrayAction, TrayState, take_tray_actions};

#[cfg(any(debug_assertions, test))]
pub use crate::tray::simulate_tray;

// --- macOS ---------------------------------------------------------------------

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;

// --- Windows -------------------------------------------------------------------

#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod platform;

// --- 其它平台：没有托盘，关闭即退出 -------------------------------------------

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[path = "other.rs"]
mod platform;

#[cfg(test)]
mod tests;
