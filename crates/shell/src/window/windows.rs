use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::time::Duration;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Memory::{GetProcessHeap, HeapCompact};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, SetProcessWorkingSetSize,
    TerminateProcess,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CallWindowProcW, DispatchMessageW, EnumWindows, GW_OWNER, GWLP_WNDPROC,
    GetClassNameW, GetMessageW, GetWindow, GetWindowThreadProcessId, KBDLLHOOKSTRUCT, MSG,
    PostThreadMessageW, SetWindowLongPtrW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_ACTIVATE, WM_DESTROY, WM_KEYDOWN, WM_SYSKEYDOWN,
    WNDPROC,
};

static OLD_PROC: AtomicIsize = AtomicIsize::new(0);

/// 让钩子线程重挂低级键盘钩子（钩子链后装先调：窗口每次激活都
/// 把我们的钩子挪回链首，跟 AIMP 这类自己拦媒体键的播放器争抢）。
const WM_REHOOK_MEDIA: u32 = 0x8000 + 0x5A; // WM_APP + 0x5A
/// 键盘钩子线程的线程 id（PostThreadMessage 重挂用）。
static HOOK_TID: AtomicU32 = AtomicU32::new(0);

/// 低级键盘钩子：截下 4 个媒体键转成引擎命令并吞掉（issue #11 的
/// 成因是别的播放器用同类钩子抢走了按键；RegisterHotKey 争不过吞
/// 键钩子，只能在同一层抢）。只在装钩子的线程的消息循环里被调。
unsafe extern "system" fn media_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    // HC_ACTION == 0
    if code == 0 && (w as u32 == WM_KEYDOWN || w as u32 == WM_SYSKEYDOWN) {
        // SAFETY: l 指向 KBDLLHOOKSTRUCT。
        let vk = unsafe { (*(l as *const KBDLLHOOKSTRUCT)).vkCode } as u16;
        // 先判 vk 再取 player：每次击键都会过钩子，别在热路径上取全局。
        let player = match vk {
            VK_MEDIA_PLAY_PAUSE | VK_MEDIA_NEXT_TRACK | VK_MEDIA_PREV_TRACK | VK_MEDIA_STOP => {
                zeo_engine::Player::global()
            }
            // SAFETY: 放行其余输入。
            _ => return unsafe { CallNextHookEx(std::ptr::null_mut(), code, w, l) },
        };
        match vk {
            VK_MEDIA_PLAY_PAUSE => player.toggle(),
            VK_MEDIA_NEXT_TRACK => player.next(),
            VK_MEDIA_PREV_TRACK => player.previous(),
            _ => player.stop(),
        }
        return 1;
    }
    // SAFETY: 放行其余输入。
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, w, l) }
}

/// 键盘钩子线程：装 `WH_KEYBOARD_LL` + 消息泵；`WM_REHOOK_MEDIA`
/// 触发重挂（先卸再装，回到钩子链首）。
fn media_hook_thread() {
    let tid = unsafe { GetCurrentThreadId() };
    HOOK_TID.store(tid, Ordering::Relaxed);
    // SAFETY: 全局低级键盘钩子；回调在本线程消息循环里执行。
    let mut hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(media_hook), std::ptr::null_mut(), 0) };
    let mut msg = MSG::default();
    // SAFETY: 标准消息循环；钩子句柄由本线程持有。
    unsafe {
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_REHOOK_MEDIA {
                if !hook.is_null() {
                    UnhookWindowsHookEx(hook);
                }
                hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(media_hook), std::ptr::null_mut(), 0);
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if !hook.is_null() {
            UnhookWindowsHookEx(hook);
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // 窗口激活：把键盘钩子挪回链首（后装先调，抢回媒体键）。
    if msg == WM_ACTIVATE && w != 0 {
        let tid = HOOK_TID.load(Ordering::Relaxed);
        if tid != 0 {
            // SAFETY: 给本进程钩子线程投私有消息。
            unsafe { PostThreadMessageW(tid, WM_REHOOK_MEDIA, 0, 0) };
        }
    }
    // SAFETY: 原窗口过程，在子类化时保存。
    let result = unsafe {
        let old: WNDPROC = std::mem::transmute::<isize, WNDPROC>(OLD_PROC.load(Ordering::Relaxed));
        CallWindowProcW(old, hwnd, msg, w, l)
    };
    if msg == WM_DESTROY {
        exit_now();
    }
    result
}

/// 重启（见 `window::restart`）：先拉起新进程——NSH 的
/// `spawn_after_exit` 在 Windows 上立刻启动程序（更新前
/// `remember_program` 记下的路径），并用 `NSH_RESTART_AFTER` 告诉它等
/// 本进程退出后再开窗、再做单实例判断；随后硬结束本进程。拉起失败就
/// 留在原地，界面仍是「重启并更新」，可以再试或手动重开。
pub(crate) fn restart() {
    nsh_core::restart::run_at_exit();
    if nsh_core::restart::spawn_after_exit().is_ok() {
        exit_now();
    }
}

/// 主窗口已销毁：直接结束进程。
///
/// NSH 0.3.0 在 UI 线程退出时由线程局部析构释放 D3D / D2D 设备。Windows
/// 在持有加载器锁时运行这些析构，部分显卡驱动（Intel `igd10umt64xe`）
/// 释放设备时要等自己的工作线程退出，而那个线程退出又要加载器锁，进程
/// 就停在那里：窗口没了，进程不退。这里不走那条路径。应用数据都在改动时
/// 写盘，退出时没有要收尾的东西。
pub(crate) fn exit_now() {
    // SAFETY: 结束本进程；不运行线程局部析构与 DLL 卸载回调。
    unsafe { TerminateProcess(GetCurrentProcess(), 0) };
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
    // 引擎的系统媒体会话也要挂这个窗口（zeo_engine::media::smtc_thumbnail）。
    zeo_engine::set_hwnd(found as usize as u64);
    true
}

pub fn start() {
    // 全局媒体键：低级键盘钩子线程（与 AIMP 等播放器在同一层争抢，
    // issue #11）。
    std::thread::Builder::new()
        .name("zeo-player-kbhook".into())
        .spawn(media_hook_thread)
        .ok();
    // 根组件挂载时窗口通常已建好（runner 先建窗口再装应用）：当场子类化
    // 接管 `WM_DESTROY` 的硬退出（`exit_now`）与媒体键重挂，引擎的
    // SMTC 缩略图也要这个句柄。没找到再交给线程轮询。
    if hook() {
        return;
    }
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
}

pub fn trim_memory() {
    // SAFETY: 先让进程堆压缩（相邻空闲块合并、大块空闲 decommit——
    // Rust 的默认分配器走的就是这个堆），再把工作集裁空：伪句柄加
    // 两个 usize::MAX 等价于 EmptyWorkingSet（系统只在最小化窗口时
    // 自动裁剪，SW_HIDE 不会，所以手动做）。
    unsafe {
        HeapCompact(GetProcessHeap(), 0);
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX)
    };
}

/// 进程内存（私有字节, 工作集）：`PrivateUsage` 是本进程独占的提交
/// 内存，泄漏看它最准；`WorkingSetSize` 是任务管理器显示的驻留集。
#[cfg(any(debug_assertions, test))]
pub fn memory_usage() -> (u64, u64) {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: 伪句柄 + 已按 cb 填好大小的本进程结构体。
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    if ok == 0 {
        return (0, 0);
    }
    (
        counters.PagefileUsage as u64,
        counters.WorkingSetSize as u64,
    )
}
