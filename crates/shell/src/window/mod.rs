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
//! `WindowRef::state`，`placement` 变化写进配置；启动时在窗口内容首渲染
//! 里随开窗下发 `hide()` + `set_placement` 恢复（首个窗口状态要等首次
//! 显示后才到、按它再藏就闪了；上游按现有显示器校正）。
//!
//! 余下的平台代码：
//!
//! - macOS：找到主窗口后挂单实例的「显示已有窗口」通知（主线程，GCD
//!   主队列）。Dock 图标「重新打开」与 Windows 任务栏 / Alt+Tab 都由 NSH
//!   自己显示窗口，界面侧订阅窗口状态、`visible` 由假变真时调
//!   [`shown_by_system`]。
//! - Windows：子类化 NSH 窗口（类名 `NshWindow`）的窗口过程——激活时把
//!   媒体键钩子挪回链首，窗口销毁后直接结束进程（见 `exit_now`），句柄
//!   交给引擎的 SMTC 缩略图（`zeo_engine::set_hwnd`）；托盘在自己的
//!   线程上，带消息循环。

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use zeo_types::config::CloseAction;

/// 窗口的最小宽度（点）：侧栏固定 232 pt 展开，最小宽度保住内容区
/// 768 pt——曲目表「专辑」列的下限（`components::tracks::fits_album`
/// 断点 760）加页边距刚好放下。经 NSH 的 `WindowAttributes::min_size`
/// 生效（`App()` 里随标题栏样式一起设，内容为点）。
pub const MIN_WIDTH: f32 = 1000.0;

/// 窗口的最小高度（点）。
pub const MIN_HEIGHT: f32 = 720.0;

// 应用实例：并行宿主（界面测试一个进程挂多个引擎）下区分窗口事件的
// 归属。实例 id 绑在调用线程上——事件的生产与消费都在各实例自己的
// 引擎线程（`install` 里 `bind_instance`）；托盘、主线程、测量脚本
// 这些没绑的线程记 0（广播：任一实例可处理——真实进程只有一个实例）。
thread_local! {
    static INSTANCE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// 主窗口的 `WindowRef`（窗口模型 v2：`main_window()` 是不可见的应用
    /// 宿主，窗口请求得发给用户窗口自己）。`zeo_ui_store::install` 在窗口
    /// 内容里拿 `current_window()` 存进来；hide / show 只在界面线程调。
    static MAIN_WINDOW: std::cell::RefCell<Option<nsh_core::element::desktop::WindowRef>> =
        const { std::cell::RefCell::new(None) };
}

/// 由 `zeo_ui_store::install` 在窗口内容里调用（那里 `current_window()`
/// 才是真正的用户窗口；装在窗口外的调用拿到的是宿主）。
pub fn set_main_window(win: nsh_core::element::desktop::WindowRef) {
    MAIN_WINDOW.with(|w| *w.borrow_mut() = Some(win));
}

fn main_window() -> Option<nsh_core::element::desktop::WindowRef> {
    MAIN_WINDOW.with(|w| w.borrow().clone())
}
static INSTANCE_SEQ: AtomicU64 = AtomicU64::new(1);

/// 把当前线程绑到一个应用实例（`zeo_ui_store::install` 在引擎线程调）。
pub fn bind_instance() -> u64 {
    let id = INSTANCE_SEQ.fetch_add(1, Ordering::Relaxed);
    INSTANCE.set(id);
    id
}

fn instance() -> u64 {
    INSTANCE.get()
}

/// 当前关闭行为（界面设置写入；配置存在共享的曲库存储里，各实例一致）。
static MODE: AtomicU8 = AtomicU8::new(0);
/// 点了关闭、在等回答的实例集（0 是系统侧不指明实例的调用，任一实例
/// 可取走）。记归属才不会被并行宿主的别的实例取走、弹窗弹错窗口。
static CLOSE_REQUESTED: std::sync::Mutex<std::collections::BTreeSet<u64>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());
/// 真的要退出（托盘「退出」、选择「退出」）：关闭不再拦截。
static QUITTING: AtomicBool = AtomicBool::new(false);
/// 隐藏到托盘的实例集（0 是系统侧不指明实例的调用，所有实例都当隐藏）。
static HIDDEN: std::sync::Mutex<std::collections::BTreeSet<u64>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());
/// 重新打开时恢复上次的窗口大小与位置（界面设置写入）。
static REMEMBER: AtomicBool = AtomicBool::new(false);
/// 窗口事件（关闭请求、托盘操作、显示 / 隐藏）每实例一条唤醒通道：
/// `Notify` 一次只醒一个等待者，事件会被先醒的实例偷走——每个实例
/// 发自己的 `Notify`（[`listener`]），许可各存各的，不偷不漏。
static LISTENERS: std::sync::Mutex<Vec<std::sync::Weak<tokio::sync::Notify>>> =
    std::sync::Mutex::new(Vec::new());

/// 本实例的窗口事件通道（`install` 时拿一条，事件来时 `notified`）。
/// 之前错过的也算：`Notify` 存一个许可。
pub fn listener() -> std::sync::Arc<tokio::sync::Notify> {
    let notify = std::sync::Arc::new(tokio::sync::Notify::new());
    LISTENERS
        .lock()
        .unwrap()
        .push(std::sync::Arc::downgrade(&notify));
    notify
}

pub(crate) fn notify() {
    LISTENERS.lock().unwrap().retain(|w| match w.upgrade() {
        Some(n) => {
            n.notify_one();
            true
        }
        None => false,
    });
}

/// 窗口被系统重新显示（macOS 点 Dock 图标，Windows 任务栏 / Alt+Tab）而
/// 不是走 [`show`]：同样算显示，界面重新装上（否则窗口是空的）。由界面
/// 侧订阅 NSH 窗口状态、在 `visible` 由假变真时调（`zeo-ui-store` 的
/// `install`），按调用线程的实例清——并行宿主里别的实例的隐藏态不动。
pub fn shown_by_system() {
    let mut hidden = HIDDEN.lock().unwrap();
    let removed = hidden.remove(&instance()) | hidden.remove(&0);
    drop(hidden);
    if removed {
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

/// 有没有发给本实例、还没回答的关闭请求（读取后清除）。别实例的请
/// 求不动（[`listener`] 的唤醒所有实例都收得到，各取各的）。
pub fn take_close_request() -> bool {
    let mut requested = CLOSE_REQUESTED.lock().unwrap();
    requested.remove(&instance()) || requested.remove(&0)
}

pub fn is_hidden() -> bool {
    let hidden = HIDDEN.lock().unwrap();
    hidden.contains(&instance()) || hidden.contains(&0)
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
            CLOSE_REQUESTED.lock().unwrap().insert(instance());
            notify();
            false
        }
    }
}

/// 用户要求退出（⌘Q / Ctrl+Q、应用菜单「退出」、macOS Dock 菜单「退出」；
/// NSH 的 `on_quit_request`）：窗口在时与点关闭同样按「关闭窗口」设置
/// 询问 / 最小化到托盘 / 退出；窗口已藏到托盘时直接退出——没有窗口可藏，
/// 这时的退出请求只能是真想退（Dock 菜单）。系统关机 / 注销不经这里。
/// 返回 true 表示照常退出。
pub fn quit_requested() -> bool {
    is_hidden() || close_requested()
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
    platform::start();
}

/// 隐藏窗口（播放继续）。
pub fn hide() {
    HIDDEN.lock().unwrap().insert(instance());
    notify();
    if let Some(window) = main_window() {
        window.hide();
    }
}

/// 显示并激活窗口。
pub fn show() {
    let mut hidden = HIDDEN.lock().unwrap();
    hidden.remove(&instance());
    hidden.remove(&0);
    drop(hidden);
    notify();
    if let Some(window) = main_window() {
        window.show();
        window.focus();
    }
}

/// 退出应用（结束播放）：与 Cmd+Q / 退出菜单同一条收尾路径
/// （NSH `desktop::quit` 不再回到关闭拦截）。Windows 上窗口销毁时子类
/// 过程仍走 `exit_now` 硬收尾（先转发 `WM_DESTROY` 给上游过程，托盘图
/// 标随窗口销毁摘掉）。
pub fn quit() {
    QUITTING.store(true, Ordering::Relaxed);
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    nsh_core::element::desktop::quit();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    std::process::exit(0);
}

/// 重启应用（更新装好后「重启生效」）。Windows 上不走 NSH 的
/// `desktop::restart`：那条路要运行器完整收尾（停视图、释放设备、进程
/// 正常退出）后才拉起新进程，而本应用平时退出都在 `WM_DESTROY` 里硬
/// 结束（见 `exit_now`），收尾路径会卡住——旧进程不退，新进程按
/// `singleInstance` 把启动转交给旧进程后自己退出，重启就成了单纯退出。
/// 这里先拉起新进程（它会等本进程退出再开窗），再像普通退出一样硬结束。
/// macOS 照常走 NSH。
pub fn restart() {
    #[cfg(target_os = "windows")]
    platform::restart();
    #[cfg(not(target_os = "windows"))]
    nsh_core::element::desktop::restart();
}

/// 把空闲内存还给系统（隐藏到托盘后调用）：界面拆掉释放的堆页
/// 一直留在进程工作集 / malloc 分区里，任务管理器 / 活动监视器的
/// 数值不会降，要手动裁剪。它只回收**调用时已经空闲**的页——隐藏
/// 期间后台线程新产生的垃圾要再调一次才走，所以隐藏状态下由界面
/// 侧周期重复调用（`store` 的隐藏清理循环）。
pub fn trim_memory() {
    platform::trim_memory();
}

/// 进程内存采样，测量脚本用：macOS 是（物理足迹, 驻留大小），Windows
/// 是（私有字节, 工作集）；其它平台未实现，返回 (0, 0)。
#[cfg(any(debug_assertions, test))]
pub fn memory_usage() -> (u64, u64) {
    platform::memory_usage()
}

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
