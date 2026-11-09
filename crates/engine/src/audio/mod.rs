//! 播放引擎：symphonia 解码 →（rubato 重采样）→ cpal 输出。
//! 引擎线程拥有解码器与命令循环；cpal 回调经共享环形缓冲取数；
//! UI 通过快照 + 轮询任务读取状态（PlayerState 等价字段）。

mod cmd;
mod load;
mod output;
mod queue;
mod ring;
mod run;
mod state;
mod stream;
#[cfg(test)]
mod tests;

pub(crate) use crate::media::MediaSession;
pub(crate) use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
pub(crate) use rubato::Resampler;
pub(crate) use std::path::PathBuf;
pub(crate) use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
pub(crate) use std::sync::{Arc, Mutex, OnceLock};
pub(crate) use std::time::{Duration, Instant};
pub(crate) use zeo_metadata::MetaCache;
pub(crate) use zeo_metadata::codec::Source;

// 持久化在 library.json 的播放设置类型在 zeo-types；平铺重导出，
// 调用方照旧 `engine::audio::LoopMode`。
pub use zeo_types::audio::*;

/// 环形缓冲的填充目标（秒）。CPU 忙时引擎线程可能几百毫秒排不上，
/// 缓冲要够撑过去；淡变、音量、暂停都在输出回调里生效，缓冲长短不影响
/// 它们的响应，seek / 换曲会作废整段缓冲。
pub(crate) const RING_TARGET_SECS: usize = 2;
/// 缓冲容量（秒）：目标之外留出一个大解码包的余量。
pub(crate) const RING_CAPACITY_SECS: usize = 3;

/// 每轮最多解码的包数（防止一轮占太久，命令响应变慢）。
pub(crate) const MAX_PACKETS_PER_TICK: usize = 64;
/// 播放中两轮之间最多等多久（有命令时立即醒）。缓冲有 2 秒，不必频繁醒。
pub(crate) const PLAYING_TICK: Duration = Duration::from_millis(10);
/// 停着时两轮之间最多等多久。
pub(crate) const IDLE_TICK: Duration = Duration::from_millis(100);
/// 输出设备缓冲的目标时长（毫秒）：不少设备的默认缓冲只有 ~10ms——系统
/// CPU 忙时回调线程稍一排队就欠载（环形缓冲再大也救不了设备侧的空窗）。
/// 40ms 把调度余量放大几倍，对交互延迟仍无感。
pub(crate) const OUTPUT_BUFFER_MS: u32 = 40;

pub(crate) use cmd::*;
pub(crate) use load::*;
pub(crate) use output::*;
pub(crate) use queue::*;
pub use ring::*;
pub(crate) use run::*;
pub use state::*;
pub(crate) use stream::*;
