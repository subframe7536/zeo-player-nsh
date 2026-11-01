//! 窗口与托盘：拦截关闭（最小化到托盘或退出）、隐藏 / 显示窗口、托盘图标
//! 与菜单。NSH 0.3.0 没有这些接口，这里直接用系统 API：
//!
//! - macOS：替换 NSH 窗口代理的 `windowShouldClose:` 实现；显示 / 隐藏用
//!   `makeKeyAndOrderFront` / `orderOut`；全部在主线程（GCD 主队列）。
//! - Windows：子类化 NSH 窗口（类名 `NshWindow`）的窗口过程拦截 `WM_CLOSE`，
//!   接管去掉系统标题栏的 `WM_NCCALCSIZE` 与顶边缩放的 `WM_NCHITTEST`（窗口
//!   按钮由应用画，NSH 的标题栏样式切到 Default 后就不画它自己的了），
//!   窗口销毁后直接结束进程（见 `exit_now`）；托盘在自己的线程上，带消息循环。
//!
//! 窗口里点关闭时，[`CloseAction`] 决定做什么；`Ask` 时不关闭，只把
//! [`take_close_request`] 置位，由界面弹出选择。

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::config::CloseAction;

/// 窗口的最小宽度（点）：播放条还能同时放下进度条两侧时间的最小值
///（`ui::frame::bar` 的 `fits_times` 阈值）。NSH 没有最小尺寸的接口，
/// 两个平台都在系统层设（macOS `contentMinSize`，Windows `WM_GETMINMAXINFO`）。
pub const MIN_WIDTH: f32 = 720.0;

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
static TRAY_ACTIONS: std::sync::Mutex<Vec<TrayAction>> = std::sync::Mutex::new(Vec::new());

/// 等到下一个窗口事件（之前错过的也算：`Notify` 存一个许可）。
pub async fn changed() {
    CHANGED.notified().await;
}

fn notify() {
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

/// 设置「记住窗口大小和位置」：立即应用到现有窗口（macOS 自动保存开关，
/// Windows 只决定退出 / 缩放结束时是否记录；恢复都在下次启动的挂钩里做）。
pub fn set_remember_frame(on: bool) {
    REMEMBER.store(on, Ordering::Relaxed);
    platform::remember(on);
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

/// 用户点了关闭：返回 true 表示照常关闭（退出）。
fn on_close_clicked() -> bool {
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
    pub playing: bool,
    pub has_track: bool,
}

/// 取走托盘上发生的操作（菜单项、Windows 上单击图标）。
pub fn take_tray_actions() -> Vec<TrayAction> {
    std::mem::take(&mut *TRAY_ACTIONS.lock().unwrap())
}

/// 测量脚本用：像点了窗口的关闭按钮（主线程上，不经 NSH 的 `performClose`：
/// 那会在 NSH 应用界面变更的途中跑嵌套的事件循环）。
pub fn simulate_close() {
    #[cfg(target_os = "macos")]
    dispatch2::DispatchQueue::main().exec_async(|| {
        if on_close_clicked() {
            quit();
        }
    });
    #[cfg(not(target_os = "macos"))]
    if on_close_clicked() {
        quit();
    }
}

/// 测量脚本用：像托盘菜单那样发出一个操作（macOS 在主线程上，与菜单
/// 回调相同）。
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
fn install_tray_handlers() {
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
            // Windows：左键单击图标打开窗口（右键出菜单）。
            tray_icon::TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            } if cfg!(target_os = "windows") => push_tray(TrayAction::Show),
            tray_icon::TrayIconEvent::DoubleClick { .. } => push_tray(TrayAction::Show),
            _ => {}
        },
    ));
}

/// 启动：挂上关闭拦截、创建托盘图标。可以多次调用（只做一次）。
pub fn start() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    install_tray_handlers();
    platform::start();
}

/// 隐藏窗口（播放继续）。
pub fn hide() {
    HIDDEN.store(true, Ordering::Relaxed);
    notify();
    platform::hide();
}

/// 显示并激活窗口。
pub fn show() {
    HIDDEN.store(false, Ordering::Relaxed);
    notify();
    platform::show();
}

/// 退出应用（结束播放）。
pub fn quit() {
    QUITTING.store(true, Ordering::Relaxed);
    platform::quit();
}

/// 更新托盘菜单（当前曲目、播放 / 暂停）。
pub fn update_tray(state: TrayState) {
    platform::update(state);
}

/// 把空闲内存还给系统（隐藏到托盘后调用）：界面拆掉释放的堆页
/// 一直留在进程工作集 / malloc 分区里，任务管理器 / 活动监视器的
/// 数值不会降，要手动裁剪。
pub fn trim_memory() {
    platform::trim_memory();
}

/// Windows 的窗口句柄（系统媒体控件要用）；找到窗口前为 `None`。
#[cfg(target_os = "windows")]
pub fn hwnd() -> Option<u64> {
    platform::HWND_VALUE.get().copied()
}

/// Windows 的窗口按钮由应用画（`ui::caption`），这里是它们的操作与状态。
#[cfg(target_os = "windows")]
pub use platform::{caption_ready, close, is_full_screen, is_maximized, minimize, toggle_maximize};

// --- 托盘菜单（两个平台共用） ---------------------------------------------------

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod tray {
    use std::cell::RefCell;

    use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

    use super::TrayState;

    struct Tray {
        _icon: TrayIcon,
        now: MenuItem,
        toggle: MenuItem,
        prev: MenuItem,
        next: MenuItem,
    }

    thread_local! {
        static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
    }

    /// 应用图标缩到托盘尺寸（RGBA）。
    fn icon() -> Option<Icon> {
        let img = image::load_from_memory(include_bytes!("../icon.png")).ok()?;
        let img = img
            .resize(64, 64, image::imageops::FilterType::Triangle)
            .to_rgba8();
        let (w, h) = img.dimensions();
        Icon::from_rgba(img.into_raw(), w, h).ok()
    }

    /// 在托盘所属线程上创建（macOS 主线程；Windows 托盘线程）。
    pub fn create() {
        let menu = Menu::new();
        let now = MenuItem::with_id("now", "未在播放", false, None);
        let toggle = MenuItem::with_id("toggle", "播放", false, None);
        let prev = MenuItem::with_id("prev", "上一首", false, None);
        let next = MenuItem::with_id("next", "下一首", false, None);
        let show = MenuItem::with_id("show", "显示 Zeo Player", true, None);
        let quit = MenuItem::with_id("quit", "退出 Zeo Player", true, None);
        let _ = menu.append_items(&[
            &now,
            &PredefinedMenuItem::separator(),
            &toggle,
            &prev,
            &next,
            &PredefinedMenuItem::separator(),
            &show,
            &quit,
        ]);
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Zeo Player");
        if let Some(icon) = icon() {
            builder = builder.with_icon(icon);
        }
        // Windows：左键单击打开窗口，右键出菜单。
        if cfg!(target_os = "windows") {
            builder = builder.with_menu_on_left_click(false);
        }
        match builder.build() {
            Ok(icon) => TRAY.with(|t| {
                *t.borrow_mut() = Some(Tray {
                    _icon: icon,
                    now,
                    toggle,
                    prev,
                    next,
                })
            }),
            Err(err) => eprintln!("zeo-player: 托盘图标创建失败：{err}"),
        }
    }

    /// 移除托盘图标（`TrayIcon` 析构时从通知区删除）。
    #[cfg(target_os = "windows")]
    pub fn remove() {
        TRAY.with(|t| drop(t.borrow_mut().take()));
    }

    pub fn apply(state: &TrayState) {
        TRAY.with(|t| {
            let Some(t) = t.borrow().as_ref().map(|t| {
                (
                    t.now.clone(),
                    t.toggle.clone(),
                    t.prev.clone(),
                    t.next.clone(),
                )
            }) else {
                return;
            };
            let (now, toggle, prev, next) = t;
            now.set_text(if state.has_track {
                state.title.as_str()
            } else {
                "未在播放"
            });
            toggle.set_text(if state.playing { "暂停" } else { "播放" });
            for item in [&toggle, &prev, &next] {
                item.set_enabled(state.has_track);
            }
        });
    }
}

// --- macOS ---------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod platform {
    use std::sync::OnceLock;
    use std::time::Duration;

    use dispatch2::DispatchQueue;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, Imp, Sel};
    use objc2::{MainThreadMarker, msg_send, sel};
    use objc2_app_kit::{NSApplication, NSWindow};
    use objc2_foundation::{NSString, NSSize};

    use super::TrayState;

    /// frame 自动保存的名字（`NSWindow Frame <name>` 存进用户默认）。
    const FRAME_NAME: &str = "ZeoPlayerMain";

    /// 「记住窗口大小和位置」：开时用系统的自动保存（`NSWindow` 的
    /// `frameAutosaveName` 会在移动 / 缩放与退出时自己写默认，下次启动
    /// `setFrameUsingName` 恢复）；关时停止保存并清掉已存的记录。
    fn apply_frame(window: &NSWindow, mtm: MainThreadMarker, on: bool) {
        let name = NSString::from_str(FRAME_NAME);
        if on {
            window.setFrameUsingName(&name);
            window.setFrameAutosaveName(&name);
        } else {
            window.setFrameAutosaveName(&NSString::new());
            NSWindow::removeFrameUsingName(&name, mtm);
        }
    }

    /// 设置改动时应用到现有窗口（启动时窗口还没建好，`hook` 里也会做）。
    pub fn remember(on: bool) {
        DispatchQueue::main().exec_async(move || {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            if let Some(window) = main_window(mtm) {
                apply_frame(&window, mtm, on);
            }
        });
    }

    /// NSH 原来的 `windowShouldClose:` 实现。
    static ORIGINAL: OnceLock<usize> = OnceLock::new();

    type ShouldClose = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> Bool;

    unsafe extern "C-unwind" fn should_close(
        this: *mut AnyObject,
        cmd: Sel,
        window: *mut AnyObject,
    ) -> Bool {
        if !super::on_close_clicked() {
            return Bool::NO;
        }
        match ORIGINAL.get() {
            // SAFETY: 这是被替换下来的同签名实现。
            Some(&imp) => unsafe {
                std::mem::transmute::<usize, ShouldClose>(imp)(this, cmd, window)
            },
            None => Bool::YES,
        }
    }

    /// NSH 原来的 `applicationShouldHandleReopen:hasVisibleWindows:` 实现。
    static ORIGINAL_REOPEN: OnceLock<usize> = OnceLock::new();

    type ShouldReopen =
        unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, Bool) -> Bool;

    /// 点 Dock 图标：NSH 把窗口提到前面，这里先告诉界面窗口回来了。
    unsafe extern "C-unwind" fn should_reopen(
        this: *mut AnyObject,
        cmd: Sel,
        app: *mut AnyObject,
        visible: Bool,
    ) -> Bool {
        super::shown_by_system();
        match ORIGINAL_REOPEN.get() {
            // SAFETY: 这是被替换下来的同签名实现。
            Some(&imp) => unsafe {
                std::mem::transmute::<usize, ShouldReopen>(imp)(this, cmd, app, visible)
            },
            None => Bool::NO,
        }
    }

    thread_local! {
        /// 主窗口，挂钩时记下。隐藏（`orderOut`）后它的 `canBecomeMainWindow`
        /// 变成 NO，再按条件去 `windows()` 里找会找不到（托盘「显示」因此
        /// 无效），而系统警告框（`_NSAlertPanel`）也有代理，不能只看代理。
        static MAIN: std::cell::RefCell<Option<Retained<NSWindow>>> = const { std::cell::RefCell::new(None) };
    }

    fn main_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
        if let Some(window) = MAIN.with(|m| m.borrow().clone()) {
            return Some(window);
        }
        let app = NSApplication::sharedApplication(mtm);
        let window = app
            .windows()
            .iter()
            .find(|w| w.delegate().is_some() && w.canBecomeMainWindow())?;
        MAIN.with(|m| *m.borrow_mut() = Some(window.clone()));
        Some(window)
    }

    /// 替换窗口代理类的 `windowShouldClose:`；窗口还没建好时稍后重试。
    fn hook(attempt: u32) {
        DispatchQueue::main().exec_async(move || {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let Some(window) = main_window(mtm) else {
                if attempt < 100 {
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(200));
                        hook(attempt + 1);
                    });
                }
                return;
            };
            // 最小宽度（见 `MIN_WIDTH`）：保留原来的最小高度。
            let mut min: NSSize = window.contentMinSize();
            min.width = super::MIN_WIDTH as f64;
            window.setContentMinSize(min);
            apply_frame(
                &window,
                mtm,
                super::REMEMBER.load(std::sync::atomic::Ordering::Relaxed),
            );
            let Some(delegate) = window.delegate() else {
                return;
            };
            let object: &AnyObject = delegate.as_ref();
            let class = object.class();
            if let Some(method) = class.instance_method(sel!(windowShouldClose:)) {
                // SAFETY: 签名与 NSWindowDelegate 的 windowShouldClose: 相同。
                let imp: Imp = unsafe { std::mem::transmute::<ShouldClose, Imp>(should_close) };
                let old = unsafe { method.set_implementation(imp) };
                let _ = ORIGINAL.set(old as usize);
            }
            // 应用代理与窗口代理是同一个对象（NSH 的 `Delegate`）。
            if let Some(method) =
                class.instance_method(sel!(applicationShouldHandleReopen:hasVisibleWindows:))
            {
                // SAFETY: 签名与 NSApplicationDelegate 的同名方法相同。
                let imp: Imp = unsafe { std::mem::transmute::<ShouldReopen, Imp>(should_reopen) };
                let old = unsafe { method.set_implementation(imp) };
                let _ = ORIGINAL_REOPEN.set(old as usize);
            }
            super::tray::create();
        });
    }

    pub fn start() {
        hook(0);
    }

    pub fn hide() {
        DispatchQueue::main().exec_async(|| {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            if let Some(window) = main_window(mtm) {
                window.orderOut(None);
            }
        });
    }

    /// 先激活应用再把窗口提到最前：从托盘菜单打开时应用不在前台，只
    /// `makeKeyAndOrderFront` 窗口会留在别的应用后面。macOS 14 起
    /// `activateIgnoringOtherApps:` 已弃用、常常不生效，有 `activate` 时用它；
    /// `orderFrontRegardless` 保证激活被系统推迟时窗口也在最上层。
    pub fn show() {
        DispatchQueue::main().exec_async(|| {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let app = NSApplication::sharedApplication(mtm);
            let object: &AnyObject = app.as_ref();
            // SAFETY: -[NSApplication activate]（macOS 14+），先确认响应。
            let modern: bool = unsafe { msg_send![object, respondsToSelector: sel!(activate)] };
            if modern {
                let _: () = unsafe { msg_send![object, activate] };
            } else {
                #[allow(deprecated)]
                app.activateIgnoringOtherApps(true);
            }
            if let Some(window) = main_window(mtm) {
                if window.isMiniaturized() {
                    window.deminiaturize(None);
                }
                window.makeKeyAndOrderFront(None);
                window.orderFrontRegardless();
            }
        });
    }

    pub fn quit() {
        DispatchQueue::main().exec_async(|| {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let app = NSApplication::sharedApplication(mtm);
            // SAFETY: -[NSApplication terminate:]，NSH 的应用代理负责收尾。
            let _: () = unsafe { msg_send![&app, terminate: Option::<&AnyObject>::None] };
        });
    }

    pub fn update(state: TrayState) {
        DispatchQueue::main().exec_async(move || super::tray::apply(&state));
    }

    pub fn trim_memory() {
        unsafe extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
        }
        // SAFETY: zone 传 NULL 表示默认分区；goal 0 表示尽量释放空闲页。
        unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
    }
}

// --- Windows -------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod platform {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::mpsc::{Sender, channel};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_VISIBLE_FRAME_BORDER_THICKNESS, DwmGetWindowAttribute,
    };
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::System::Threading::{
        AttachThreadInput, GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId,
        SetProcessWorkingSetSize, TerminateProcess,
    };
    use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, CallWindowProcW, DefWindowProcW, DispatchMessageW, EnumWindows, GW_OWNER,
        GWL_STYLE, GWLP_WNDPROC, GetClassNameW, GetClientRect, GetForegroundWindow, GetWindow,
        GetWindowLongPtrW, GetWindowThreadProcessId, HTTOP, HTTOPLEFT, HTTOPRIGHT, IsIconic,
        IsWindowVisible, IsZoomed, MINMAXINFO, MSG, NCCALCSIZE_PARAMS, PM_REMOVE, PeekMessageW,
        PostMessageW, SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE, SM_CXPADDEDBORDER, SM_CYFRAME, SW_HIDE,
        SW_RESTORE, SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWNOACTIVATE, SetForegroundWindow,
        SetWindowLongPtrW, SetWindowPlacement, ShowWindowAsync, TranslateMessage,
        GetWindowPlacement, WINDOWPLACEMENT, WM_CLOSE, WM_DESTROY, WM_EXITSIZEMOVE,
        WM_GETMINMAXINFO, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST, WM_SHOWWINDOW,
        WM_SYSCOMMAND, WNDPROC, WS_CAPTION, WS_THICKFRAME,
    };

    use super::TrayState;

    pub static HWND_VALUE: OnceLock<u64> = OnceLock::new();
    static OLD_PROC: AtomicIsize = AtomicIsize::new(0);
    static TRAY_TX: OnceLock<Mutex<Sender<TrayMsg>>> = OnceLock::new();

    fn hwnd() -> Option<HWND> {
        HWND_VALUE.get().map(|h| *h as usize as HWND)
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        let mut l = l;
        match msg {
            WM_CLOSE if !super::on_close_clicked() => return 0,
            // 最小宽度（见 `MIN_WIDTH`）：先让原窗口过程填默认值，再把
            // ptMinTrackSize 抬到应用的下限（点 → 物理像素）。
            WM_GETMINMAXINFO => {
                // SAFETY: 原窗口过程，在子类化时保存。
                let result = unsafe {
                    let old: WNDPROC =
                        std::mem::transmute::<isize, WNDPROC>(OLD_PROC.load(Ordering::Relaxed));
                    CallWindowProcW(old, hwnd, msg, w, l)
                };
                // SAFETY: 本消息的 l 指向 MINMAXINFO。
                let info = unsafe { &mut *(l as *mut MINMAXINFO) };
                // SAFETY: 普通查询。
                let dpi = unsafe { GetDpiForWindow(hwnd) } as f32;
                let min_x = (super::MIN_WIDTH * dpi / 96.0).ceil() as i32;
                info.ptMinTrackSize.x = info.ptMinTrackSize.x.max(min_x);
                return result;
            }
            // 隐藏后被重新显示（任务栏、Alt+Tab、`show`）。
            WM_SHOWWINDOW if w != 0 => super::shown_by_system(),
            // 拖动 / 缩放结束：记录窗口位置（「记住窗口大小和位置」）。
            WM_EXITSIZEMOVE => save_frame(hwnd),
            // 标题栏由应用自己画（NSH 的标题栏样式为 Default，不再处理这些）。
            WM_NCCALCSIZE if w != 0 && has_style(hwnd, WS_CAPTION) => {
                // SAFETY: w 非 0 时 l 指向本消息的 NCCALCSIZE_PARAMS。
                return unsafe { nc_calc_size(hwnd, w, l) };
            }
            WM_NCHITTEST => {
                if let Some(code) = top_edge(hwnd, l) {
                    return code as LRESULT;
                }
            }
            // 不让系统在应用顶部重画旧式标题栏（`lparam` -1）。
            WM_NCACTIVATE => l = -1,
            _ => {}
        }
        // SAFETY: 原窗口过程，在子类化时保存。
        let result = unsafe {
            let old: WNDPROC =
                std::mem::transmute::<isize, WNDPROC>(OLD_PROC.load(Ordering::Relaxed));
            CallWindowProcW(old, hwnd, msg, w, l)
        };
        if msg == WM_DESTROY {
            exit_now();
        }
        result
    }

    fn has_style(hwnd: HWND, style: u32) -> bool {
        // SAFETY: 读本进程窗口的样式。
        let current = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        current & style == style
    }

    fn zoomed(hwnd: HWND) -> bool {
        // SAFETY: 普通查询。
        unsafe { IsZoomed(hwnd) != 0 }
    }

    /// 「记住窗口大小和位置」：记下常规矩形与是否最大化（最大化时的
    /// `rcNormalPosition` 是还原后的位置，正好是要存的）。
    fn save_frame(hwnd: HWND) {
        if !super::REMEMBER.load(Ordering::Relaxed) {
            return;
        }
        // SAFETY: 输出参数，先填长度。
        let mut wp = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        // SAFETY: 本进程窗口。
        if unsafe { GetWindowPlacement(hwnd, &mut wp) } == 0 {
            return;
        }
        let r = wp.rcNormalPosition;
        crate::config::library_store().set_window_frame(crate::config::SavedFrame {
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
            maximized: zoomed(hwnd),
        });
    }

    /// 启动时恢复上次的窗口位置（`hook` 子类化后调用）。
    fn restore_frame(hwnd: HWND) {
        if !super::REMEMBER.load(Ordering::Relaxed) {
            return;
        }
        let Some(saved) = crate::config::library_store().window_frame() else {
            return;
        };
        if saved.w <= 0 || saved.h <= 0 {
            return;
        }
        // SAFETY: 结构由调用方填充，字段都是普通值。
        let wp = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            flags: 0,
            showCmd: if saved.maximized {
                SW_SHOWMAXIMIZED
            } else {
                SW_SHOWNOACTIVATE
            } as u32,
            ptMinPosition: POINT { x: -1, y: -1 },
            ptMaxPosition: POINT { x: -1, y: -1 },
            rcNormalPosition: RECT {
                left: saved.x,
                top: saved.y,
                right: saved.x + saved.w,
                bottom: saved.y + saved.h,
            },
        };
        // SAFETY: 本进程窗口。
        unsafe { SetWindowPlacement(hwnd, &wp) };
    }

    /// 缩放边框的厚度（像素）；最大化窗口也正好超出显示器这么多。
    fn frame_thickness(hwnd: HWND) -> i32 {
        // SAFETY: 普通查询。
        unsafe {
            let dpi = GetDpiForWindow(hwnd);
            GetSystemMetricsForDpi(SM_CYFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
        }
    }

    /// Windows 11 沿窗口顶边画的那条可见边框（像素；没有时为 0）。
    fn top_border(hwnd: HWND) -> i32 {
        let mut thickness: u32 = 0;
        // SAFETY: 该属性的值是一个 u32。
        let hr = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_VISIBLE_FRAME_BORDER_THICKNESS as _,
                &mut thickness as *mut u32 as *mut _,
                std::mem::size_of::<u32>() as u32,
            )
        };
        if hr >= 0 { thickness.min(8) as i32 } else { 0 }
    }

    /// `WM_NCCALCSIZE`：去掉系统标题栏，客户区铺到窗口顶边；两侧与底部的
    /// 缩放边框照旧（在 Windows 10 / 11 上不可见）。与 NSH 隐藏标题栏时
    /// 的算法相同。
    unsafe fn nc_calc_size(hwnd: HWND, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe {
            let params = &mut *(l as *mut NCCALCSIZE_PARAMS);
            let top = params.rgrc[0].top;
            DefWindowProcW(hwnd, WM_NCCALCSIZE, w, l);
            // 最大化时窗口超出显示器一个边框厚度；否则让出 Windows 11 顶边
            // 那条边框，应用的标题栏完整可见。
            params.rgrc[0].top = top
                + if zoomed(hwnd) {
                    frame_thickness(hwnd)
                } else {
                    top_border(hwnd)
                };
        }
        0
    }

    /// `WM_NCHITTEST`：客户区顶边一个边框厚度内可以拖动改变窗口高度。
    fn top_edge(hwnd: HWND, l: LPARAM) -> Option<u32> {
        if !has_style(hwnd, WS_CAPTION | WS_THICKFRAME) || zoomed(hwnd) {
            return None;
        }
        let mut p = POINT {
            x: (l & 0xFFFF) as i16 as i32,
            y: ((l >> 16) & 0xFFFF) as i16 as i32,
        };
        let mut client = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: 本地结构上的普通调用。
        unsafe {
            ScreenToClient(hwnd, &mut p);
            GetClientRect(hwnd, &mut client);
        }
        let border = frame_thickness(hwnd);
        if p.y < 0 || p.y >= border {
            return None;
        }
        Some(if p.x < border * 2 {
            HTTOPLEFT
        } else if p.x >= client.right - border * 2 {
            HTTOPRIGHT
        } else {
            HTTOP
        })
    }

    /// 已找到并子类化窗口：可以由应用画窗口按钮了。
    pub fn caption_ready() -> bool {
        HWND_VALUE.get().is_some()
    }

    pub fn is_maximized() -> bool {
        hwnd().is_some_and(zoomed)
    }

    /// NSH 的全屏去掉了窗口边框与标题栏样式。
    pub fn is_full_screen() -> bool {
        hwnd().is_some_and(|h| !has_style(h, WS_CAPTION))
    }

    /// 异步发系统命令：在点击处理里同步改窗口大小会重入 NSH 的事件分发。
    fn sys_command(command: u32) {
        if let Some(h) = hwnd() {
            // SAFETY: 投递到本进程窗口的消息队列。
            unsafe { PostMessageW(h, WM_SYSCOMMAND, command as WPARAM, 0) };
        }
    }

    pub fn minimize() {
        sys_command(SC_MINIMIZE);
    }

    pub fn toggle_maximize() {
        sys_command(if is_maximized() {
            SC_RESTORE
        } else {
            SC_MAXIMIZE
        });
    }

    /// 与点系统关闭按钮相同（经过 `WM_CLOSE` 拦截：询问 / 托盘 / 退出）。
    pub fn close() {
        if let Some(h) = hwnd() {
            // SAFETY: 同上。
            unsafe { PostMessageW(h, WM_CLOSE, 0, 0) };
        }
    }

    /// 「记住窗口大小和位置」只影响是否记录：标志在保存时读，运行中
    /// 改动不用对窗口做什么；关掉时已存的位置由 `set_remember_frame` 清掉。
    pub fn remember(_on: bool) {}

    /// 主窗口已销毁：移除托盘图标后直接结束进程。
    ///
    /// NSH 0.3.0 在 UI 线程退出时由线程局部析构释放 D3D / D2D 设备。Windows
    /// 在持有加载器锁时运行这些析构，部分显卡驱动（Intel `igd10umt64xe`）
    /// 释放设备时要等自己的工作线程退出，而那个线程退出又要加载器锁，进程
    /// 就停在那里：窗口没了，进程不退。这里不走那条路径。应用数据都在改动时
    /// 写盘，退出时没有要收尾的东西。
    fn exit_now() {
        if let Some(h) = hwnd() {
            save_frame(h);
        }
        if let Some(tx) = TRAY_TX.get() {
            let (done_tx, done_rx) = channel();
            if tx.lock().unwrap().send(TrayMsg::Remove(done_tx)).is_ok() {
                let _ = done_rx.recv_timeout(Duration::from_millis(500));
            }
        }
        // SAFETY: 结束本进程；不运行线程局部析构与 DLL 卸载回调。
        unsafe { TerminateProcess(GetCurrentProcess(), 0) };
    }

    /// 发给托盘线程的消息。
    enum TrayMsg {
        State(TrayState),
        /// 移除托盘图标，完成后回复。
        Remove(Sender<()>),
    }

    unsafe extern "system" fn find(hwnd: HWND, out: LPARAM) -> i32 {
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != GetCurrentProcessId() || !GetWindow(hwnd, GW_OWNER).is_null() {
                return 1;
            }
            let mut name = [0u16; 64];
            let n = GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32);
            if String::from_utf16_lossy(&name[..n.max(0) as usize]) == "NshWindow" {
                *(out as *mut HWND) = hwnd;
                return 0;
            }
            1
        }
    }

    /// 找到 NSH 的窗口并子类化（同进程内可跨线程设置窗口过程）。
    fn hook() -> bool {
        let mut found: HWND = std::ptr::null_mut();
        // SAFETY: 回调只写 `found`。
        unsafe { EnumWindows(Some(find), &mut found as *mut HWND as LPARAM) };
        if found.is_null() {
            return false;
        }
        // SAFETY: 同进程的窗口；保存原过程后替换。
        let old =
            unsafe { SetWindowLongPtrW(found, GWLP_WNDPROC, proc as *const () as usize as isize) };
        OLD_PROC.store(old, Ordering::Relaxed);
        let _ = HWND_VALUE.set(found as usize as u64);
        restore_frame(found);
        true
    }

    pub fn start() {
        std::thread::Builder::new()
            .name("zeo-player-window".into())
            .spawn(|| {
                for _ in 0..150 {
                    if hook() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            })
            .ok();
        // 托盘：自己的线程，带消息循环（托盘图标的隐藏窗口在这里收消息）。
        let (tx, rx) = channel::<TrayMsg>();
        let _ = TRAY_TX.set(Mutex::new(tx));
        std::thread::Builder::new()
            .name("zeo-player-tray".into())
            .spawn(move || {
                super::tray::create();
                loop {
                    // SAFETY: 本线程的消息循环。
                    unsafe {
                        let mut msg: MSG = std::mem::zeroed();
                        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                    while let Ok(msg) = rx.try_recv() {
                        match msg {
                            TrayMsg::State(state) => super::tray::apply(&state),
                            TrayMsg::Remove(done) => {
                                super::tray::remove();
                                let _ = done.send(());
                                return;
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(30));
                }
            })
            .ok();
    }

    pub fn hide() {
        if let Some(h) = hwnd() {
            // SAFETY: 异步显示命令，不等窗口线程。
            unsafe { ShowWindowAsync(h, SW_HIDE) };
        }
    }

    /// 显示并提到最前。Windows 只允许前台进程调用 `SetForegroundWindow`
    /// 抢前台（否则只在任务栏闪烁）：托盘菜单的点击已经结束，所以暂时把
    /// 本线程的输入挂到当前前台窗口的线程上，再提前台。窗口由 nsh-ui 线程
    /// 处理显示命令，等它显示出来（最多约 0.5 秒）再提。
    pub fn show() {
        let Some(h) = hwnd() else { return };
        let value = h as usize;
        // SAFETY: 异步显示命令，不等窗口线程。
        unsafe {
            ShowWindowAsync(
                h,
                if IsIconic(h) != 0 {
                    SW_RESTORE
                } else {
                    SW_SHOW
                },
            )
        };
        std::thread::spawn(move || {
            let h = value as HWND;
            for _ in 0..25 {
                // SAFETY: 只读窗口状态。
                if unsafe { IsWindowVisible(h) } != 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            // SAFETY: 挂接只在本次调用内，结束时解除；各函数对无效句柄返回失败。
            unsafe {
                let me = GetCurrentThreadId();
                let foreground =
                    GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
                let attached = foreground != 0
                    && foreground != me
                    && AttachThreadInput(me, foreground, 1) != 0;
                BringWindowToTop(h);
                SetForegroundWindow(h);
                if attached {
                    AttachThreadInput(me, foreground, 0);
                }
            }
        });
    }

    pub fn quit() {
        if let Some(h) = hwnd() {
            // SAFETY: 让 NSH 照常关闭窗口（QUITTING 已置位，不再拦截）。
            unsafe { PostMessageW(h, WM_CLOSE, 0, 0) };
        } else {
            std::process::exit(0);
        }
    }

    pub fn update(state: TrayState) {
        if let Some(tx) = TRAY_TX.get() {
            let _ = tx.lock().unwrap().send(TrayMsg::State(state));
        }
    }

    pub fn trim_memory() {
        // SAFETY: 伪句柄加两个 usize::MAX 等价于 EmptyWorkingSet：系统只
        // 在最小化窗口时自动裁剪，SW_HIDE 不会，所以这里手动做。
        unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
    }
}

// --- 其它平台：没有托盘，关闭即退出 -------------------------------------------

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use super::TrayState;

    pub fn start() {}
    pub fn hide() {}
    pub fn show() {}
    pub fn quit() {
        std::process::exit(0);
    }
    pub fn remember(_on: bool) {}
    pub fn update(_state: TrayState) {}
    pub fn trim_memory() {}
}
