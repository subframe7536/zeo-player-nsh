use std::sync::OnceLock;
use std::time::Duration;

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Imp, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSApplication, NSWindow};

use crate::tray::TrayState;

/// NSH 原来的 `applicationShouldHandleReopen:hasVisibleWindows:` 实现。
static ORIGINAL_REOPEN: OnceLock<usize> = OnceLock::new();

type ShouldReopen = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, Bool) -> Bool;

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

/// 挂钩 NSH 的共享代理（Dock 重开、单实例通知）并创建托盘；窗口还没
/// 建好时稍后重试。
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
        let Some(delegate) = window.delegate() else {
            return;
        };
        let object: &AnyObject = delegate.as_ref();
        // 「显示已有窗口」分布式通知：第二份实例 post 时这个对象响应
        // `zeoPlayerShow:`（`single_instance` 里说明）。
        crate::single_instance::observe(object);
        let class = object.class();
        // 应用代理与窗口代理是同一个对象（NSH 的 `Delegate`）。
        if let Some(method) =
            class.instance_method(sel!(applicationShouldHandleReopen:hasVisibleWindows:))
        {
            // SAFETY: 签名与 NSApplicationDelegate 的同名方法相同。
            let imp: Imp = unsafe { std::mem::transmute::<ShouldReopen, Imp>(should_reopen) };
            let old = unsafe { method.set_implementation(imp) };
            let _ = ORIGINAL_REOPEN.set(old as usize);
        }
        crate::tray::create();
    });
}

pub fn start() {
    hook(0);
}

pub fn update(state: TrayState) {
    DispatchQueue::main().exec_async(move || crate::tray::apply(&state));
}

pub fn trim_memory() {
    unsafe extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }
    // SAFETY: zone 传 NULL 遍历全部分区（NanoZone 管小对象，大
    // 块仍走 szone——只调默认分区碰不到它）；goal 0 表示尽量释放
    // 空闲页。返回值恒为 0、不代表没释放（实测能把已空闲的大块
    // 脏页还给系统）。
    unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
}

/// 进程内存（物理足迹, 驻留大小）：`phys_footprint` 就是活动监视器
/// 「内存」列的口径（含压缩与 IOSurface），`resident_size` 接近
/// 「真实内存」列。测量脚本用。
#[cfg(any(debug_assertions, test))]
pub fn memory_usage() -> (u64, u64) {
    // rusage_info_v4（libproc.h）：16B uuid 后跟 35 个 u64 字段——
    // ri_resident_size 是 uuid 后第 7 个（含 uuid 共第 9 个 u64），
    // ri_phys_footprint 第 8 个。缓冲区给宽一点，内核按它自己的
    // 结构大小回填。
    #[repr(C)]
    struct RusageV4 {
        raw: [u64; 40],
    }
    unsafe extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageV4) -> i32;
    }
    const RUSAGE_INFO_V4: i32 = 4;
    let mut info = RusageV4 { raw: [0; 40] };
    // SAFETY: buffer 是本进程地址空间里不小于写出结构的内存。
    let ok = unsafe { proc_pid_rusage(std::process::id() as i32, RUSAGE_INFO_V4, &mut info) };
    if ok != 0 {
        return (0, 0);
    }
    (info.raw[9], info.raw[8])
}
