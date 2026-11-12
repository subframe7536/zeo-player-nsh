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
