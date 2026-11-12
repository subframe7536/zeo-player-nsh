//! 单实例：正式构建只允许一份进程；第二份把第一份实例的窗口激活后退出。
//!
//! - Windows / 打包产物：`nsh.json` 的 `"singleInstance": true` 交给上
//!   游 runner（`nsh-windows-app`）——第二份在窗口创建前经按 app id
//!   命名的互斥体判重，把启动参数用 `WM_COPYDATA` 转交第一份（藏在
//!   托盘的主窗口也会被重新显示、提到最前）后退出，这里不再有平台
//!   代码。
//! - macOS：上游不实现（`singleInstance` 只靠 LaunchServices，挡不住
//!   直接执行二进制或 `open -n`），这里保留临时目录
//!   `<bundle id>.lock` 的文件锁（flock）判重——拿不到排他锁说明已
//!   有实例，按 pid 找更旧的实例激活它，并发分布式通知
//!   `dev.subf.zeo-player.show` 让它把窗口提到最前（藏在托盘的窗口
//!   `activate` 不会放出来），然后退出。`runningApplications` 有注册
//!   延迟、同时启动互相看不见，故不用它判重。Dock / `open` 由
//!   LaunchServices 挡，这里拦的是直接执行二进制或 `open -n`；拿不
//!   到 bundle id（非 .app 启动）无从判重，放行。
//!
//! 只有开发构建与测试不拦（设了 `ZEO_PLAYER_DATA_DIR` 的正式构建也拦）。
//! 在拉起引擎与曲库之前调用（`App()` 第一行——macOS 的第二份进程死在
//! `view.load` 里、NSH `view.show` 之前，不会闪一下窗口）。

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

    /// 单实例：固定名文件锁判重——能拿到排他锁的是第一份；拿不到说明
    /// 已有实例，激活它、发「显示已有窗口」通知，再退出自己。
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

// --- 其它平台 -------------------------------------------------------------------

// Windows 由打包 runner 的 `singleInstance` 判重（见模块文档），`enforce`
// 在这些平台是空操作。
#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn enforce() {}
}
