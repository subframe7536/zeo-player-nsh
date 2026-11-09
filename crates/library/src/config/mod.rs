//! 全局库 [`LibraryStore`]：配置 + 索引（各提供方的曲目合成一份索引；
//! 提供方实现在 `crate::provider` 下，`scan`/`rebuild` 经它的门面同步）。
//! 数据模型（`TrackEntry`、`LibraryConfig`、`ScanProgress`…）在
//! `zeo_types::config`，经 `pub use` 平铺进本模块，调用方照旧 `config::X`。

mod index;
mod split;
mod store;
#[cfg(test)]
mod tests;

pub use split::*;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

use zeo_metadata::MetaCache;

pub use zeo_types::config::*;

/// 窗口应用请求：设置变了要把行为应用到窗口，但 store 在 shell
/// （`zeo-shell`）之下不能反向依赖——由上层（`zeo_ui_store::install`）注入钩子。
pub enum WindowApply {
    /// 「点关闭按钮」的行为。
    CloseAction(CloseAction),
    /// 「记住窗口大小和位置」开关。
    RememberFrame(bool),
}

static ON_WINDOW_APPLY: OnceLock<Box<dyn Fn(WindowApply) + Send + Sync>> = OnceLock::new();

/// 注入窗口应用钩子（上层 crate 调用；重复调用覆盖）。
pub fn set_window_apply_hook(hook: Box<dyn Fn(WindowApply) + Send + Sync>) {
    let _ = ON_WINDOW_APPLY.set(hook);
}

fn window_apply(apply: WindowApply) {
    if let Some(hook) = ON_WINDOW_APPLY.get() {
        hook(apply);
    }
}

/// 全局库存储：配置 + 索引。锁粒度粗但只在扫描/读取列表时短暂持有。
pub struct LibraryStore {
    dir: PathBuf,
    /// `Arc` 共享快照：界面轮询按指针判变化——所有写路径都必须经
    /// `update_config` 整份换新 `Arc`（原地改会让读侧漏掉变化）。
    config: std::sync::Mutex<Arc<LibraryConfig>>,
    // 索引与扫描进度由提供方（`crate::provider`）写；按惯例只经方法访问。
    pub(crate) tracks: std::sync::RwLock<Arc<Vec<TrackEntry>>>,
    pub(crate) dirs: std::sync::RwLock<Arc<Vec<String>>>,
    pub(crate) scanning: std::sync::atomic::AtomicBool,
    pub(crate) done: std::sync::atomic::AtomicUsize,
    pub(crate) total: std::sync::atomic::AtomicUsize,
    pub(crate) current: std::sync::Mutex<String>,
}
pub fn library_store() -> &'static LibraryStore {
    static STORE: OnceLock<LibraryStore> = OnceLock::new();
    STORE.get_or_init(|| {
        let dir = data_dir();
        // 单元测试不碰真实数据目录（自己设了 ZEO_PLAYER_DATA_DIR 的测试除外，
        // 同 db() 的护栏）：谁先调谁就绑，把全局绑到 dev-data 会让别人的
        // 断言吃到真实配置。
        #[cfg(test)]
        let dir = if std::env::var_os("ZEO_PLAYER_DATA_DIR").is_none() {
            std::env::temp_dir().join(format!("zeo-player-store-{}", std::process::id()))
        } else {
            dir
        };
        let _ = std::fs::create_dir_all(&dir);
        let config = load_config(&dir);
        // 低层 crate 读「本地封面优先」走 prefs 镜像，不反向依赖 store。
        zeo_types::prefs::sync_prefer_local_covers(config.prefer_local_covers);
        // 先用数据库里上次的元数据建索引，界面马上有曲库；随后的同步只重读变了的内容。
        let tracks = crate::provider::initial_index(&config);
        let dirs = crate::provider::initial_dirs(&config.roots);
        LibraryStore {
            dir,
            config: std::sync::Mutex::new(Arc::new(config)),
            tracks: std::sync::RwLock::new(Arc::new(tracks)),
            dirs: std::sync::RwLock::new(Arc::new(dirs)),
            scanning: std::sync::atomic::AtomicBool::new(false),
            done: std::sync::atomic::AtomicUsize::new(0),
            total: std::sync::atomic::AtomicUsize::new(0),
            current: std::sync::Mutex::new(String::new()),
        }
    })
}

fn config_path(dir: &Path) -> PathBuf {
    dir.join("library.json")
}

fn load_config(dir: &Path) -> LibraryConfig {
    std::fs::read(config_path(dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LibraryConfig>(&bytes).ok())
        .unwrap_or_default()
        .normalized()
}
