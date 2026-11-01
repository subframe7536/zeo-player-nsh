//! 曲目提供方：每个子模块是一个数据来源，同步时产出一组
//! `crate::config::TrackEntry` 写进全局库，它自己的设置项挂在
//! `crate::config::LibraryConfig` 上。
//!
//! 目前只有 [`local`]（本地文件夹，含根目录监视）；jellyfin、
//! YouTube Music 等按同样的形状加在旁边，并在 [`initial_index`] /
//! [`sync`] / [`rebuild`] 里各加一段。

pub(crate) mod local;

use crate::config::{LibraryConfig, LibraryStore, TrackEntry};

/// 各源的初始索引（用已缓存的元数据预热，界面马上有曲库）。
/// 多源时在这里合并每个源的产出。
pub(crate) fn initial_index(config: &LibraryConfig) -> Vec<TrackEntry> {
    local::initial_index(config)
}

/// 同步所有源并重建全局索引。多源时逐个同步、合并写回。
pub(crate) fn sync(store: &LibraryStore) -> usize {
    local::scan(store)
}

/// 重建所有源：丢掉缓存，重新产出全部曲目。
pub(crate) fn rebuild(store: &LibraryStore) -> usize {
    local::rebuild(store)
}
