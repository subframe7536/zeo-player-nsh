use super::*;

/// 在根组件的生命周期里、下一轮再做 `f`：菜单项触发、但做的事会拆掉
/// 菜单自己所在的那一行时用（行里的任务会随行一起取消）。
pub fn later(f: impl FnOnce() + 'static) {
    store().owner.run(|| {
        spawn(async move {
            // 让菜单先关完（关闭也是一轮更新），再做会拆掉菜单的事。
            timer(Duration::from_millis(16)).await;
            f()
        });
    });
}

/// 下一个受控弹出层的 id（`menu_control`）。
pub(crate) static NEXT_MENU: AtomicU64 = AtomicU64::new(1);

/// 一个弹出层（`menu` / `context_menu` / `popover`）的互斥门：共享
/// `store().menu_open` 令牌，同一时刻只开一个。桌面上右键按下不产生
/// PressOutside（macOS / Windows 的右键只发 ContextMenu 事件，没有
/// touch Start），平台不会替我们关掉上一个——开新弹层时令牌换号，
/// 旧弹层的 `open` 翻 false 自己关掉。
#[derive(Clone, Copy)]
pub struct MenuGate {
    id: u64,
    open: Signal<Option<u64>>,
}

impl MenuGate {
    /// `menu` / `context_menu` 的 `.open(..)`。
    pub fn open(self) -> impl Fn() -> Option<bool> + 'static {
        let (id, open) = (self.id, self.open);
        move || Some(open.get() == Some(id))
    }

    /// `menu` / `context_menu` 的 `.on_open_change(..)`：开关请求写回令牌。
    pub fn menu_change(self) -> impl Fn(&nsh_base::MenuEvent) + 'static {
        let (id, open) = (self.id, self.open);
        move |e| {
            if let nsh_base::MenuEvent::OpenChange(next, _) = e {
                if *next {
                    open.set(Some(id));
                } else if open.get_untracked() == Some(id) {
                    open.set(None);
                }
            }
        }
    }

    /// `select` 的 `.on_open_change(..)`：开关请求写回令牌（同
    /// `menu_change`，配 `.open(gate.open())` 用）。
    pub fn select_change(self) -> impl Fn(&nsh_base::SelectEvent) + 'static {
        let (id, open) = (self.id, self.open);
        move |e| {
            if let nsh_base::SelectEvent::OpenChange(next, _) = e {
                if *next {
                    open.set(Some(id));
                } else if open.get_untracked() == Some(id) {
                    open.set(None);
                }
            }
        }
    }

    /// `combobox` 的 `.on_open_change(..)`：开关请求写回令牌（同
    /// `select_change`，配 `.open(gate.open())` 用）。
    pub fn combobox_change(self) -> impl Fn(&nsh_base::ComboboxEvent) + 'static {
        let (id, open) = (self.id, self.open);
        move |e| {
            if let nsh_base::ComboboxEvent::OpenChange(next, _) = e {
                if *next {
                    open.set(Some(id));
                } else if open.get_untracked() == Some(id) {
                    open.set(None);
                }
            }
        }
    }

    /// `popover` 的 `.open(..)`：它另有自己的开关信号 `local`，弹层开着
    /// 且令牌还在时才显示（令牌被夺走即被挤掉）。
    pub fn open_with(self, local: Signal<bool>) -> impl Fn() -> Option<bool> + 'static {
        let (id, open) = (self.id, self.open);
        move || Some(local.get() && open.get() == Some(id))
    }

    /// `popover` 的 `.on_open_change(..)`：请求写回本地信号与令牌。
    pub fn popover_change(self, local: Signal<bool>) -> impl Fn(&nsh_base::PopoverEvent) + 'static {
        let (id, open) = (self.id, self.open);
        move |e| {
            if let nsh_base::PopoverEvent::OpenChange(next, _) = e {
                local.set(*next);
                if *next {
                    open.set(Some(id));
                } else if open.get_untracked() == Some(id) {
                    open.set(None);
                }
            }
        }
    }

    /// 主动归还令牌。
    pub fn close(self) {
        if self.open.get_untracked() == Some(self.id) {
            self.open.set(None);
        }
    }

    /// 当前是否持有令牌。
    pub fn is_open(self) -> bool {
        self.open.get() == Some(self.id)
    }

    /// `effect(gate.evicted(local))`：令牌被别的弹层夺走时复位 `popover`
    /// 的本地信号（触发器高亮、面板状态读它，不复位会留下「看着还开着」
    /// 的残影）。
    pub fn evicted(self, local: Signal<bool>) -> impl Fn() + 'static {
        let (id, open) = (self.id, self.open);
        move || {
            if open.get() != Some(id) {
                local.set(false);
            }
        }
    }
}

/// 一个弹出层的互斥门：`.open(..)` 用 `gate.open()`、`.on_open_change(..)`
/// 用 `gate.menu_change()`；`popover` 用 `gate.open_with(local)` /
/// `gate.popover_change(local)` 并 `effect(gate.evicted(local))`。
pub fn menu_control() -> MenuGate {
    MenuGate {
        id: NEXT_MENU.fetch_add(1, Ordering::Relaxed),
        open: store().menu_open,
    }
}

/// 「把一件事排回界面侧跑」的把手：`Send`，可以带进后台线程。`STORE`
/// 是线程局部的，工作线程拿不到 `store()`，在界面侧拿 `poster()` 的
/// 克隆带去，`post(f)` 排的事由 `install` 的轮询循环在界面侧执行。
/// 每个应用实例一条通道（界面测试里多个实例并行互不串）。
#[derive(Clone)]
pub struct Post(pub(crate) mpsc::Sender<Box<dyn FnOnce() + Send>>);

impl Post {
    /// 把 `f` 排给界面侧执行。
    pub fn post(&self, f: impl FnOnce() + Send + 'static) {
        let _ = self.0.send(Box::new(f));
    }
}

thread_local! {
    pub(crate) static POSTER: RefCell<Option<Post>> = const { RefCell::new(None) };
}

/// 当前应用实例的回排把手（组件上下文里拿）。
pub fn poster() -> Post {
    POSTER
        .with(|p| p.borrow().clone())
        .expect("store installed by the root component")
}
