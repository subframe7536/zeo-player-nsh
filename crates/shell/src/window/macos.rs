use std::time::Duration;

use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSApplication, NSWindow};

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

/// 挂钩 NSH 的共享代理（单实例通知）；窗口还没建好时稍后重试。Dock
/// 重开由 NSH 自己显示窗口，界面侧从窗口状态的 `visible` 跳变得知。
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
    });
}

pub fn start() {
    hook(0);
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
