//! 界面底座：主题色 / 自适应色（`theme`）、界面字体栈（`fonts`），以及
//! 界面状态——引擎与曲库的信号（轮询共享状态）、换曲时的加载流水线
//! （元数据 / 歌词 / 波形）、专辑封面缓存、播放命令。
//! （theme 读这里的 accent / tint 信号、这里又调 theme 的 accent_of ——
//! 真循环，所以主题与状态同属一个 crate。）

pub mod fonts;
pub mod theme;

mod commands;
mod covers;
mod format;
mod install;
mod menu;
mod nav;
mod ops;
mod playlists;
mod settings;
mod tags;
#[cfg(test)]
mod tests;
mod types;

pub(crate) use std::cell::{Cell, RefCell};
pub(crate) use std::collections::{BTreeSet, HashMap};
pub(crate) use std::path::Path;
pub(crate) use std::sync::atomic::{AtomicU64, Ordering};
pub(crate) use std::sync::{Arc, mpsc};
pub(crate) use std::time::{Duration, Instant};

pub(crate) use nsh_core::prelude::*;

pub(crate) use zeo_engine::{LyricsSource, Meta, Player};
pub(crate) use zeo_library::config::library_store;
pub(crate) use zeo_metadata::lyrics::LyricsOutput;
pub(crate) use zeo_metadata::{MetaCache, MetaInfo};
pub(crate) use zeo_playlists::{Kind, Playlist, Rule};
pub(crate) use zeo_types::config::{ScanProgress, TrackEntry};

pub use commands::*;
pub use covers::*;
pub use format::*;
pub use install::*;
pub use menu::*;
pub use nav::*;
pub use ops::*;
pub use playlists::*;
pub use settings::*;
pub use tags::*;
pub use types::*;
