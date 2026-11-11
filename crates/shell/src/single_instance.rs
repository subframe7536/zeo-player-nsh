//! 单实例：正式构建只允许一份进程；第二份把第一份实例的窗口激活后退出。
//! NSH 没有「已有实例」的入口，这里直接用系统 API：
//!
//! - Windows：固定名字的互斥体（`Local\dev.subf.zeo-player.single`）判重；
//!   给第一份实例的顶层 `NshWindow` 窗口直接投递注册消息
//!   `dev.subf.zeo-player.show`（`window.rs` 的 `proc` 收进来调
//!   [`handle_message`]、`show()` 自己），找不到窗口退回广播；然后
//!   `TerminateProcess`（不跑析构，顺带绕开已知的退出死锁）。
//! - macOS：临时目录下的 `<bundle id>.lock` 文件锁（flock）判重——拿不
//!   到排他锁说明已有实例，按 pid 找更旧的实例激活它，并发分布式通知
//!   `dev.subf.zeo-player.show` 让它把窗口提到最前（藏在托盘的窗口
//!   `activate` 不会放出来），然后退出。`runningApplications` 有注册延
//!   迟、同时启动互相看不见，故不用它判重。Dock / `open` 由
//!   LaunchServices 挡，这里拦的是直接执行二进制或 `open -n`；拿不到
//!   bundle id（非 .app 启动，如 cargo run --release）无从判重，放行。
//!
//! 只有开发构建与测试不拦（设了 `ZEO_PLAYER_DATA_DIR` 的正式构建也拦）。
//! 在拉起引擎与曲库之前调用（`App()` 第一行——进程死在 `view.load` 里、
//! NSH `view.show` 之前；窗口建出来不带 WS_VISIBLE，第二份的窗口不会闪
//! 一下）。

/// 单实例检查；幂等，在拉起引擎与曲库之前调用。
pub fn enforce() {
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
        crate::show();
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

    /// macOS 14 之前的激活路径：带 ActivateIgnoringOtherApps 才抢得到前
    /// 台；该选项在新系统被弃用，而这条路径只有旧系统会走到。
    #[allow(deprecated)]
    fn activate_legacy(app: &NSRunningApplication) -> bool {
        app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps)
    }

    /// 单实例：固定名文件锁（对应 Windows 的命名互斥体）判重——能拿到排
    /// 他锁的是第一份；拿不到说明已有实例，激活它、发「显示已有窗口」通
    /// 知，再退出自己。
    pub fn enforce() {
        let Some(id) = NSBundle::mainBundle().bundleIdentifier() else {
            return;
        };
        // per-user 临时目录下的 `<bundle id>.lock`；第一份实例一直持有
        // 句柄，进程退出时内核自动释放，不留残骸。`runningApplications`
        // 有注册延迟（同时启动互相看不见），只靠扫描会放行多份。
        let contended = match std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(std::env::temp_dir().join(format!("{id}.lock")))
        {
            Ok(file) => match file.try_lock() {
                // 拿到锁：第一份实例。句柄不关闭，活到进程结束。
                Ok(()) => return std::mem::forget(file),
                Err(std::fs::TryLockError::WouldBlock) => true,
                // 锁不可用（文件系统不支持等）：退回按 pid 扫描。
                Err(_) => false,
            },
            Err(_) => false,
        };
        let me = NSRunningApplication::currentApplication();
        // `currentApplication` 这时还没注册好，`processIdentifier` 是 -1；
        // 自己的 pid 走 `std::process::id()`。
        let my_pid = std::process::id() as i32;
        let mut second = contended;
        for app in NSRunningApplication::runningApplicationsWithBundleIdentifier(&id).iter() {
            // 跳过自己与 pid 更大的实例：同时启动时都拿着各自的锁，按 pid
            // 对称只激活（并退给）更旧的那份。
            if *app == *me || app.processIdentifier() >= my_pid {
                continue;
            }
            second = true;
            // SAFETY: -[NSRunningApplication activateFromApplication:options:]
            // 是 macOS 14+ 的 API，先确认响应；旧系统退回 activateWithOptions:。
            let modern: bool = unsafe {
                msg_send![&*app, respondsToSelector: sel!(activateFromApplication:options:)]
            };
            let _ = if modern {
                // ActivateIgnoringOtherApps 在 macOS 14 起无效；激活已有实
                // 例时把它所有窗口提到前面。
                app.activateFromApplication_options(
                    &me,
                    NSApplicationActivationOptions::ActivateAllWindows,
                )
            } else {
                activate_legacy(&app)
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
    pub fn handle_message(msg: u32) -> bool {
        let m = INSTANCE_MSG.load(Ordering::Relaxed);
        if m == 0 || msg != m {
            return false;
        }
        crate::show();
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
