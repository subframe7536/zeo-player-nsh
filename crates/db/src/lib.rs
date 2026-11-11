//! SQLite 存储：曲目元数据（封面与歌词之外的全部：标签、技术参数，
//! 按文件指纹失效；歌词不入库——只缓存播放位置附近的几首）与播放状态
//! （上次播放的曲目、位置和队列）。文件是数据目录里的 `zeo-player.db`；
//! 打不开时什么也不存，应用照常只用内存。

mod conn;
mod meta;
mod state;
mod stats;
#[cfg(test)]
mod tests;

pub use conn::{later, shrink_memory};
pub use meta::*;
pub use state::*;
pub use stats::*;
