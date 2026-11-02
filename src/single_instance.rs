//! 单实例：正式构建只允许一份进程；第二份把第一份实例的窗口激活后退出。
//! NSH 没有「已有实例」的入口，这里直接用系统 API：
//!
//! - Windows：固定名字的互斥体（`Local\dev.subf.zeo-player.single`）判重；
//!   给第一份实例的顶层 `NshWindow` 窗口直接投递注册消息
//!   `dev.subf.zeo-player.show`（`window.rs` 的 `proc` 收进来调
//!   [`handle_message`]、`show()` 自己），找不到窗口退回广播；然后
//!   `TerminateProcess`（不跑析构，顺带绕开已知的退出死锁）。
//! - macOS：同 bundle id 的 `NSRunningApplication` 已有别人时激活它，并
//!   发分布式通知 `dev.subf.zeo-player.show` 让它把窗口提到最前（藏在
//!   托盘的窗口 `activate` 不会放出来），然后退出。Dock / `open` 由
//!   LaunchServices 挡，这里拦的是直接执行二进制或 `open -n`；拿不到
//!   bundle id（非 .app 启动，如 cargo run --release）无从判重，放行。
//!
//! 只有开发构建与测试不拦（设了 `ZEO_PLAYER_DATA_DIR` 的正式构建也拦）。
//! 在拉起引擎与曲库之前调用（`App()` 第一行——进程死在 `view.load` 里、
//! NSH `view.show` 之前；窗口建出来不带 WS_VISIBLE，第二份的窗口不会闪
//! 一下）。

/// 单实例检查；幂等，在拉起引擎与曲库之前调用。
pub(crate) fn enforce() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if cfg!(any(debug_assertions, test)) {
            return;
        }
        platform::enforce();
    });
}

// --- macOS ---------------------------------------------------------------------

/// 让 `object`（NSH 的共享 Delegate）订阅「显示已有窗口」分布式通知。
/// `window.rs` 的 `hook` 拿到代理后调用一次。
#[cfg(target_os = "macos")]
pub(crate) use platform::observe;

#[cfg(target_os = "macos")]
mod platform {
    use objc2::ffi::class_addMethod;
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{msg_send, sel};
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    use objc2_foundation::{
        NSBundle, NSDistributedNotificationCenter, NSNotificationSuspensionBehavior, NSString,
    };

    /// 「显示已有窗口」分布式通知名（第二份实例 post，第一份实例经
    /// [`observe`] 订阅）。
    const SHOW_NOTIFY: &str = "dev.subf.zeo-player.show";

    type ShowImp = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject);

    /// 收到第二份实例的「显示已有窗口」通知：把窗口提到最前（藏在托盘的
    /// 也放出来）。
    unsafe extern "C-unwind" fn show(_this: *mut AnyObject, _cmd: Sel, _note: *mut AnyObject) {
        crate::window::show();
    }

    /// 让 `object`（NSH 的共享 Delegate，整个生命期有效）订阅
    /// 「显示已有窗口」：在它的类上补一个响应方法 `zeoPlayerShow:`。
    /// `window.rs` 的 `hook` 拿到代理后调用一次。
    pub fn observe(object: &AnyObject) {
        let class = object.class();
        // SAFETY: `zeoPlayerShow:` 只加一次（hook 只跑一次）；签名
        // void(id, SEL, id) 对应编码 "v@:@"。
        unsafe {
            class_addMethod(
                class as *const AnyClass as *mut AnyClass,
                sel!(zeoPlayerShow:),
                std::mem::transmute::<ShowImp, Imp>(show),
                c"v@:@".as_ptr(),
            );
            NSDistributedNotificationCenter::defaultCenter()
                .addObserver_selector_name_object_suspensionBehavior(
                    object,
                    sel!(zeoPlayerShow:),
                    Some(&NSString::from_str(SHOW_NOTIFY)),
                    None,
                    NSNotificationSuspensionBehavior::DeliverImmediately,
                );
        }
    }

    /// 单实例：同 bundle id 已有别的实例时把它激活、发「显示已有窗口」
    /// 通知，再退出自己。
    pub fn enforce() {
        let Some(id) = NSBundle::mainBundle().bundleIdentifier() else {
            return;
        };
        let me = NSRunningApplication::currentApplication();
        let mut second = false;
        for app in NSRunningApplication::runningApplicationsWithBundleIdentifier(&id).iter() {
            if *app == *me {
                continue;
            }
            second = true;
            // SAFETY: -[NSRunningApplication activateFromApplication:options:]
            // 是 macOS 14+ 的 API，先确认响应；旧系统退回 activateWithOptions:。
            let modern: bool = unsafe {
                msg_send![app, respondsToSelector: sel!(activateFromApplication:options:)]
            };
            let _ = if modern {
                app.activateFromApplication_options(
                    &me,
                    NSApplicationActivationOptions::ActivateIgnoringOtherApps,
                )
            } else {
                app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps)
            };
        }
        if second {
            // SAFETY: 名字是有效的 NSString；不带 userInfo。
            unsafe {
                NSDistributedNotificationCenter::defaultCenter()
                    .postNotificationName_object_userInfo_deliverImmediately(
                        &NSString::from_str(SHOW_NOTIFY),
                        None,
                        None,
                        true,
                    );
            }
            std::process::exit(0);
        }
    }
}

// --- Windows -------------------------------------------------------------------

/// 「显示已有窗口」消息到了第一份实例的窗口（`window.rs` 的 `proc` 转
/// 过来）。返回是不是单实例消息（要不要消费掉）。
#[cfg(target_os = "windows")]
pub(crate) use platform::handle_message;

#[cfg(target_os = "windows")]
mod platform {
    use std::sync::atomic::{AtomicU32, Ordering};

    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{
        CreateMutexW, GetCurrentProcess, GetCurrentProcessId, TerminateProcess,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetClassNameW, GetWindow, GetWindowThreadProcessId, HWND_BROADCAST,
        PostMessageW, RegisterWindowMessageW,
    };

    /// 「显示已有窗口」的注册消息值（`enforce` 注册，同名同值）。
    static INSTANCE_MSG: AtomicU32 = AtomicU32::new(0);

    /// 「显示已有窗口」消息到了第一份实例的窗口（`window.rs` 的 `proc` 转
    /// 过来）：显示自己（藏在托盘的也放出来）。返回是不是单实例消息
    /// （要不要消费掉）。
    pub(crate) fn handle_message(msg: u32) -> bool {
        let m = INSTANCE_MSG.load(Ordering::Relaxed);
        if m == 0 || msg != m {
            return false;
        }
        crate::window::show();
        true
    }

    /// 固定名字的互斥体判重。已存在说明另一份在跑——给它发「显示已有
    /// 窗口」再退出。互斥体句柄由第一份实例保有，随进程退出自动释放，
    /// 无需显式关闭。
    pub fn enforce() {
        // SAFETY: 注册消息同名同值；消息与互斥体都是会话级的系统对象。
        unsafe {
            INSTANCE_MSG.store(
                RegisterWindowMessageW(windows_sys::w!("dev.subf.zeo-player.show")),
                Ordering::Relaxed,
            );
            let mutex = CreateMutexW(
                std::ptr::null(),
                1,
                windows_sys::w!("Local\\dev.subf.zeo-player.single"),
            );
            if mutex.is_null() || GetLastError() != ERROR_ALREADY_EXISTS {
                return;
            }
            post_show();
            TerminateProcess(GetCurrentProcess(), 0);
        }
    }

    /// 找第一份实例的顶层 `NshWindow` 窗口直接投递（比广播可靠，也只发给
    /// 同类窗口）；它的窗口还没建好（几乎同时启动）时退回广播——那份实
    /// 例自己本来就要显示，丢了也无妨。
    fn post_show() {
        struct Ctx {
            hwnd: HWND,
            msg: u32,
        }
        unsafe extern "system" fn each(hwnd: HWND, ctx: LPARAM) -> i32 {
            let ctx = unsafe { &mut *(ctx as *mut Ctx) };
            let mut pid = 0u32;
            let mut name = [0u16; 64];
            // SAFETY: 读窗口进程、所有者与类名；别的进程的窗口也能查。
            let n = unsafe {
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid == GetCurrentProcessId() || !GetWindow(hwnd, GW_OWNER).is_null() {
                    return 1;
                }
                GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32)
            };
            if String::from_utf16_lossy(&name[..n.max(0) as usize]) != "NshWindow" {
                return 1;
            }
            // SAFETY: 向别的进程的窗口投递我们刚注册的消息；对不认识它的
            // 窗口过程无害（默认处理）。
            unsafe { PostMessageW(hwnd, ctx.msg, 0, 0) };
            ctx.hwnd = hwnd;
            0
        }
        let mut ctx = Ctx {
            hwnd: std::ptr::null_mut(),
            msg: INSTANCE_MSG.load(Ordering::Relaxed),
        };
        // SAFETY: ctx 只在枚举期间被回调写。
        unsafe { EnumWindows(Some(each), &mut ctx as *mut Ctx as LPARAM) };
        if ctx.hwnd.is_null() {
            // SAFETY: 广播给所有顶层窗口。
            unsafe { PostMessageW(HWND_BROADCAST, ctx.msg, 0, 0) };
        }
    }
}

// --- 其它平台 -------------------------------------------------------------------

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    pub fn enforce() {}
}
