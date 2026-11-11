//! 跨层只读的运行期偏好镜像：低层 crate（如 `zeo-metadata` 的封面缓存键）
//! 需要读「本地文件优先」这类设置，但不能反向依赖持有设置的
//! `zeo_library::LibraryStore`——store 在加载 / 写入时同步这里的镜像。

use std::sync::atomic::{AtomicBool, Ordering};

static PREFER_LOCAL_COVERS: AtomicBool = AtomicBool::new(false);

/// 封面来源：同名本地图片优先于内嵌封面（store 加载配置时同步）。
pub fn prefer_local_covers() -> bool {
    PREFER_LOCAL_COVERS.load(Ordering::Relaxed)
}

/// `zeo_library::LibraryStore` 在配置加载与 `set_prefer_local_covers` 时调用。
pub fn sync_prefer_local_covers(on: bool) {
    PREFER_LOCAL_COVERS.store(on, Ordering::Relaxed);
}
