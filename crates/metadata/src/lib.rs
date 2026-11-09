//! 曲目派生数据：lofty 元数据读取（时长、标签、封面、技术参数，本文件）、
//! 封面缩略图缓存（`covers`）、LRC 歌词解析（`lyrics`）、波形条（`waveform`）。
//! 对齐 ap-spec 的 AudioMetadata 字段（title 缺省用文件名去扩展名、artist 缺省
//! Unknown Artist、album 缺省 Unknown Album；多 artist 用 `; ` 拼接）。

pub mod codec;
pub mod covers;
pub mod lyrics;
pub mod rename;
pub mod waveform;
pub mod write;

mod cache;
mod read;
#[cfg(test)]
mod tests;

pub use zeo_types::meta::{CacheEntry, MetaInfo, ReplayGain, file_stamp};

pub use cache::*;
pub use read::*;
