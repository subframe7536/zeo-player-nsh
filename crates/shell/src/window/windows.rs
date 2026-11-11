use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_VISIBLE_FRAME_BORDER_THICKNESS, DwmGetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::System::Memory::{GetProcessHeap, HeapCompact};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, SetProcessWorkingSetSize,
    TerminateProcess,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CallWindowProcW, DefWindowProcW, DispatchMessageW, EnumWindows, GW_OWNER,
    GWL_STYLE, GWLP_WNDPROC, GetClassNameW, GetClientRect, GetMessageW, GetWindow,
    GetWindowLongPtrW, GetWindowThreadProcessId, HTTOP, HTTOPLEFT, HTTOPRIGHT, IsIconic, IsZoomed,
    KBDLLHOOKSTRUCT, MSG, NCCALCSIZE_PARAMS, PM_REMOVE, PeekMessageW, PostMessageW,
    PostThreadMessageW, SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE, SM_CXPADDEDBORDER, SM_CYFRAME,
    SW_RESTORE, SW_SHOW, SetForegroundWindow, SetWindowLongPtrW, SetWindowsHookExW,
    ShowWindowAsync, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_ACTIVATE, WM_CLOSE,
    WM_DESTROY, WM_KEYDOWN, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST, WM_SHOWWINDOW,
    WM_SYSCOMMAND, WM_SYSKEYDOWN, WNDPROC, WS_CAPTION, WS_THICKFRAME,
};

use crate::tray::TrayState;

pub static HWND_VALUE: OnceLock<u64> = OnceLock::new();
static OLD_PROC: AtomicIsize = AtomicIsize::new(0);
static TRAY_TX: OnceLock<Mutex<Sender<TrayMsg>>> = OnceLock::new();

/// 让钩子线程重挂低级键盘钩子（钩子链后装先调：窗口每次激活都
/// 把我们的钩子挪回链首，跟 AIMP 这类自己拦媒体键的播放器争抢）。
const WM_REHOOK_MEDIA: u32 = 0x8000 + 0x5A; // WM_APP + 0x5A
/// 键盘钩子线程的线程 id（PostThreadMessage 重挂用）。
static HOOK_TID: AtomicU32 = AtomicU32::new(0);

fn hwnd() -> Option<HWND> {
    HWND_VALUE.get().map(|h| *h as usize as HWND)
}

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
    let mut l = l;
    match msg {
        // 第二份实例「显示已有窗口」（`single_instance` 直接投递或广播）。
        m if crate::single_instance::handle_message(m) => return 0,
        // 窗口激活：把键盘钩子挪回链首（后装先调，抢回媒体键）。
        WM_ACTIVATE if w != 0 => {
            let tid = HOOK_TID.load(Ordering::Relaxed);
            if tid != 0 {
                // SAFETY: 给本进程钩子线程投私有消息。
                unsafe { PostThreadMessageW(tid, WM_REHOOK_MEDIA, 0, 0) };
            }
        }
        // 隐藏后被重新显示（任务栏、Alt+Tab、`show`）。
        WM_SHOWWINDOW if w != 0 => super::shown_by_system(),
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
        let old: WNDPROC = std::mem::transmute::<isize, WNDPROC>(OLD_PROC.load(Ordering::Relaxed));
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

/// 与点系统关闭按钮相同（`WM_CLOSE` 经 NSH 报成 close 按压，由
/// `App()` 的 `on_close_press` 走询问 / 托盘 / 退出）。
pub fn close() {
    if let Some(h) = hwnd() {
        // SAFETY: 同上。
        unsafe { PostMessageW(h, WM_CLOSE, 0, 0) };
    }
}

/// 显示并激活主窗口（托盘「显示」）：直接 `ShowWindowAsync` +
/// `SetForegroundWindow`，不走 NSH 的 `WindowRequest::Show`——那条链是
/// 引擎 batch → `WM_NSH_WAKE` → `surface.drain` → `shell::request` →
/// `window::show` → `UpdateWindow` 同步重画，隐藏后再走它会整串卡死。
/// `ShowWindowAsync` 只把显示事件投进窗口队列，调用线程永不阻塞；
/// `WM_SHOWWINDOW` 经子类过程报 `shown_by_system`，界面照旧重挂。
pub(crate) fn show() {
    let Some(h) = hwnd() else {
        return;
    };
    // SAFETY: 普通调用；`ShowWindowAsync` 可跨线程且不等待目标线程。
    unsafe {
        let _ = ShowWindowAsync(h, SW_SHOW);
        if IsIconic(h) != 0 {
            let _ = ShowWindowAsync(h, SW_RESTORE);
        }
        let _ = SetForegroundWindow(h);
    }
}

/// 主窗口已销毁：移除托盘图标后直接结束进程。
///
/// NSH 0.3.0 在 UI 线程退出时由线程局部析构释放 D3D / D2D 设备。Windows
/// 在持有加载器锁时运行这些析构，部分显卡驱动（Intel `igd10umt64xe`）
/// 释放设备时要等自己的工作线程退出，而那个线程退出又要加载器锁，进程
/// 就停在那里：窗口没了，进程不退。这里不走那条路径。应用数据都在改动时
/// 写盘，退出时没有要收尾的东西。
pub(crate) fn exit_now() {
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
            crate::tray::create();
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
                        TrayMsg::State(state) => crate::tray::apply(&state),
                        TrayMsg::Remove(done) => {
                            crate::tray::remove();
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

pub fn update(state: TrayState) {
    if let Some(tx) = TRAY_TX.get() {
        let _ = tx.lock().unwrap().send(TrayMsg::State(state));
    }
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
