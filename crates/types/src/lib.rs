//! 各领域 crate 共享的纯数据模型与小工具：`config`（曲库设置 `LibraryConfig`、
//! 索引行 `TrackEntry`、时间工具、`data_dir`）、`meta`（标签 `MetaInfo`、
//! `ReplayGain`、缓存行 `CacheEntry`、文件指纹 `file_stamp`）、`audio`
//! （播放 / EQ / ReplayGain 的持久化设置类型）、`prefs`（跨层只读的运行期
//! 偏好镜像——低层 crate 读设置不反向依赖 [`zeo_library`]）。

pub mod audio;
pub mod config;
pub mod meta;
pub mod prefs;
