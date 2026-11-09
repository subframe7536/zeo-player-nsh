//! 曲库领域：[`config`] 的全局库 [`config::LibraryStore`]（设置 +
//! 索引，持久化 `library.json`）与 [`provider`]（曲目来源；`local` 是
//! 本地文件夹扫描 + 文件夹监视）。共享数据模型在 `zeo_types::config`。

pub mod config;
pub mod provider;
