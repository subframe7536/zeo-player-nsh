//! 窗口与托盘：拦截关闭（最小化到托盘或退出）、隐藏 / 显示窗口、托盘图标
//! 与菜单。NSH 0.3.0 没有这些接口，这里直接用系统 API：
//!
//! - macOS：替换 NSH 窗口代理的 `windowShouldClose:` 实现；显示 / 隐藏用
//!   `makeKeyAndOrderFront` / `orderOut`；全部在主线程（GCD 主队列）。
//! - Windows：子类化 NSH 窗口（类名 `NshWindow`）的窗口过程拦截 `WM_CLOSE`；
//!   托盘在自己的线程上，带消息循环。
//!
//! 窗口里点关闭时，[`CloseAction`] 决定做什么；`Ask` 时不关闭，只把
//! [`take_close_request`] 置位，由界面弹出选择。

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::library::CloseAction;

/// 当前关闭行为（界面设置写入）。
static MODE: AtomicU8 = AtomicU8::new(0);
/// 窗口里点了关闭、需要问用户。
static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);
/// 真的要退出（托盘「退出」、选择「退出」）：关闭不再拦截。
static QUITTING: AtomicBool = AtomicBool::new(false);
/// 窗口已隐藏到托盘。
static HIDDEN: AtomicBool = AtomicBool::new(false);

fn mode() -> CloseAction {
    match MODE.load(Ordering::Relaxed) {
        1 => CloseAction::Tray,
        2 => CloseAction::Quit,
        _ => CloseAction::Ask,
    }
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
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let mut out = Vec::new();
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            let action = match event.id.0.as_str() {
                "show" => TrayAction::Show,
                "toggle" => TrayAction::Toggle,
                "prev" => TrayAction::Previous,
                "next" => TrayAction::Next,
                "quit" => TrayAction::Quit,
                _ => continue,
            };
            out.push(action);
        }
        while let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
            // Windows：左键单击图标打开窗口（右键出菜单）。
            if let tray_icon::TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            } = event
                && cfg!(target_os = "windows")
            {
                out.push(TrayAction::Show);
            }
            if let tray_icon::TrayIconEvent::DoubleClick { .. } = event {
                out.push(TrayAction::Show);
            }
        }
        out
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Vec::new()
}

/// 启动：挂上关闭拦截、创建托盘图标。可以多次调用（只做一次）。
pub fn start() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    platform::start();
}

/// 隐藏窗口（播放继续）。
pub fn hide() {
    HIDDEN.store(true, Ordering::Relaxed);
    platform::hide();
}

/// 显示并激活窗口。
pub fn show() {
    HIDDEN.store(false, Ordering::Relaxed);
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
        let img = img.resize(64, 64, image::imageops::FilterType::Triangle).to_rgba8();
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
        let show = MenuItem::with_id("show", "显示 Zeo", true, None);
        let quit = MenuItem::with_id("quit", "退出 Zeo", true, None);
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
        let mut builder = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("Zeo");
        if let Some(icon) = icon() {
            builder = builder.with_icon(icon);
        }
        // Windows：左键单击打开窗口，右键出菜单。
        if cfg!(target_os = "windows") {
            builder = builder.with_menu_on_left_click(false);
        }
        match builder.build() {
            Ok(icon) => TRAY.with(|t| *t.borrow_mut() = Some(Tray { _icon: icon, now, toggle, prev, next })),
            Err(err) => eprintln!("zeo: 托盘图标创建失败：{err}"),
        }
    }

    pub fn apply(state: &TrayState) {
        TRAY.with(|t| {
            let Some(t) = t.borrow().as_ref().map(|t| (t.now.clone(), t.toggle.clone(), t.prev.clone(), t.next.clone()))
            else {
                return;
            };
            let (now, toggle, prev, next) = t;
            now.set_text(if state.has_track { state.title.as_str() } else { "未在播放" });
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
    use objc2::{msg_send, sel, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSWindow};

    use super::TrayState;

    /// NSH 原来的 `windowShouldClose:` 实现。
    static ORIGINAL: OnceLock<usize> = OnceLock::new();

    type ShouldClose = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> Bool;

    unsafe extern "C-unwind" fn should_close(this: *mut AnyObject, cmd: Sel, window: *mut AnyObject) -> Bool {
        if !super::on_close_clicked() {
            return Bool::NO;
        }
        match ORIGINAL.get() {
            // SAFETY: 这是被替换下来的同签名实现。
            Some(&imp) => unsafe { std::mem::transmute::<usize, ShouldClose>(imp)(this, cmd, window) },
            None => Bool::YES,
        }
    }

    fn main_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
        let app = NSApplication::sharedApplication(mtm);
        app.windows().iter().find(|w| w.delegate().is_some() && w.canBecomeMainWindow())
    }

    /// 替换窗口代理类的 `windowShouldClose:`；窗口还没建好时稍后重试。
    fn hook(attempt: u32) {
        DispatchQueue::main().exec_async(move || {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let Some(window) = main_window(mtm) else {
                if attempt < 100 {
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(200));
                        hook(attempt + 1);
                    });
                }
                return;
            };
            let Some(delegate) = window.delegate() else { return };
            let object: &AnyObject = delegate.as_ref();
            let class = object.class();
            if let Some(method) = class.instance_method(sel!(windowShouldClose:)) {
                // SAFETY: 签名与 NSWindowDelegate 的 windowShouldClose: 相同。
                let imp: Imp = unsafe { std::mem::transmute::<ShouldClose, Imp>(should_close) };
                let old = unsafe { method.set_implementation(imp) };
                let _ = ORIGINAL.set(old as usize);
            }
            super::tray::create();
        });
    }

    pub fn start() {
        hook(0);
    }

    pub fn hide() {
        DispatchQueue::main().exec_async(|| {
            let Some(mtm) = MainThreadMarker::new() else { return };
            if let Some(window) = main_window(mtm) {
                window.orderOut(None);
            }
        });
    }

    pub fn show() {
        DispatchQueue::main().exec_async(|| {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let app = NSApplication::sharedApplication(mtm);
            if let Some(window) = main_window(mtm) {
                window.makeKeyAndOrderFront(None);
            }
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
        });
    }

    pub fn quit() {
        DispatchQueue::main().exec_async(|| {
            let Some(mtm) = MainThreadMarker::new() else { return };
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
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, SetProcessWorkingSetSize};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, DispatchMessageW, EnumWindows, GetClassNameW, GetWindow, GetWindowThreadProcessId,
        PeekMessageW, PostMessageW, SetForegroundWindow, SetWindowLongPtrW, ShowWindowAsync,
        TranslateMessage, GWLP_WNDPROC, GW_OWNER, MSG, PM_REMOVE, SW_HIDE, SW_SHOW, WM_CLOSE, WNDPROC,
    };

    use super::TrayState;

    pub static HWND_VALUE: OnceLock<u64> = OnceLock::new();
    static OLD_PROC: AtomicIsize = AtomicIsize::new(0);
    static TRAY_TX: OnceLock<Mutex<Sender<TrayState>>> = OnceLock::new();

    fn hwnd() -> Option<HWND> {
        HWND_VALUE.get().map(|h| *h as usize as HWND)
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if msg == WM_CLOSE && !super::on_close_clicked() {
            return 0;
        }
        // SAFETY: 原窗口过程，在子类化时保存。
        unsafe {
            let old: WNDPROC = std::mem::transmute::<isize, WNDPROC>(OLD_PROC.load(Ordering::Relaxed));
            CallWindowProcW(old, hwnd, msg, w, l)
        }
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
        let old = unsafe { SetWindowLongPtrW(found, GWLP_WNDPROC, proc as *const () as usize as isize) };
        OLD_PROC.store(old, Ordering::Relaxed);
        let _ = HWND_VALUE.set(found as usize as u64);
        true
    }

    pub fn start() {
        std::thread::Builder::new()
            .name("zeo-window".into())
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
        let (tx, rx) = channel::<TrayState>();
        let _ = TRAY_TX.set(Mutex::new(tx));
        std::thread::Builder::new()
            .name("zeo-tray".into())
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
                    while let Ok(state) = rx.try_recv() {
                        super::tray::apply(&state);
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

    pub fn show() {
        if let Some(h) = hwnd() {
            // SAFETY: 同上。
            unsafe {
                ShowWindowAsync(h, SW_SHOW);
                SetForegroundWindow(h);
            }
        }
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
            let _ = tx.lock().unwrap().send(state);
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
    pub fn update(_state: TrayState) {}
    pub fn trim_memory() {}
}
