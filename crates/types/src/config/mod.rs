//! 曲库共享的模型：[`TrackEntry`] 索引行、[`ScanProgress`] 同步进度、
//! 持久化设置 [`LibraryConfig`]（`library.json`），外加流派拆分与时间工具。
//!
//! 全局库 [`zeo_library::LibraryStore`]（各源的曲目合成一份索引）与提供方
//! 实现在 `zeo-library` crate；这里只放数据。

mod model;
mod paths;
mod settings;
mod time;

pub use model::*;
pub use paths::*;
pub use settings::*;
pub use time::*;
