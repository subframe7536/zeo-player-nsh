//! 引擎门面：`Player`（解码/输出/系统媒体会话）、`Library`（多根曲库）、
//! `Meta`（歌词与波形）。纯 Rust 接口，UI（`crate::ui`）与测试直接调用。

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::audio::{EngineCmd, EngineHandle, Snapshot};
use crate::library::{library_store, TrackEntry};
use crate::lyrics::{parse_lyric, LyricOptions, LyricsOutput};
use crate::metadata::MetaCache;

/// 播放器引擎句柄。
pub struct Player {
    engine: EngineHandle,
}

impl Player {
    /// 新建播放器（启动解码/输出线程与系统媒体会话）。
    pub fn new() -> Self {
        Player { engine: EngineHandle::start() }
    }

    /// 进程内唯一的播放器（UI 用）。
    pub fn global() -> &'static Player {
        static PLAYER: OnceLock<Player> = OnceLock::new();
        PLAYER.get_or_init(Player::new)
    }

    /// 打开并播放 `path`；`queue` 为所在列表（上一首/下一首）。
    pub fn open(&self, path: String, queue: Vec<String>) {
        self.engine.send(EngineCmd::Load { path: PathBuf::from(path), queue });
    }

    pub fn play(&self) {
        self.engine.send(EngineCmd::Play);
    }

    pub fn pause(&self) {
        self.engine.send(EngineCmd::Pause);
    }

    pub fn toggle(&self) {
        self.engine.send(EngineCmd::Toggle);
    }

    pub fn stop(&self) {
        self.engine.send(EngineCmd::Stop);
    }

    pub fn seek(&self, seconds: f64) {
        self.engine.send(EngineCmd::Seek(seconds));
    }

    pub fn seek_by(&self, delta: f64) {
        self.engine.send(EngineCmd::SeekBy(delta));
    }

    /// 设置音量：立即写入共享状态（输出回调下一块就生效），再通知引擎线程。
    pub fn set_volume(&self, volume: f64) {
        let volume = volume.clamp(0.0, 1.0);
        self.engine
            .shared
            .volume
            .store((volume as f32).to_bits(), std::sync::atomic::Ordering::Relaxed);
        self.engine.send(EngineCmd::SetVolume(volume));
    }

    pub fn next(&self) {
        self.engine.send(EngineCmd::Next);
    }

    pub fn previous(&self) {
        self.engine.send(EngineCmd::Previous);
    }

    /// 当前状态快照。
    pub fn state(&self) -> Snapshot {
        self.engine.snapshot()
    }
}

/// 本地曲库（多根目录、索引、流派拆分）。
pub struct Library;

impl Library {
    pub fn new() -> Self {
        Library
    }

    pub fn roots(&self) -> Vec<String> {
        library_store().roots()
    }

    /// 添加音乐根目录（规范化、去重、持久化）；返回新列表。
    pub fn add_root(&self, path: String) -> Result<Vec<String>, String> {
        library_store().add_root(&path)
    }

    /// 移除音乐根目录；返回新列表。
    pub fn remove_root(&self, path: String) -> Vec<String> {
        library_store().remove_root(&path)
    }

    pub fn tracks(&self) -> Arc<Vec<TrackEntry>> {
        library_store().tracks()
    }
}

/// 歌词来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricsSource {
    External,
    Embedded,
    None,
}

/// 歌词与波形。
pub struct Meta;

impl Meta {
    /// 歌词：外置同名 `.lrc` 优先，其次内嵌；按 ap-spec 算法解析。
    pub fn lyrics(path: &str) -> (LyricsOutput, LyricsSource) {
        let path = Path::new(path);
        if let Some(lrc) = find_external_lrc(path)
            && let Ok(text) = std::fs::read_to_string(&lrc)
        {
            return (parse_lyric(&text, LyricOptions::default()), LyricsSource::External);
        }
        if let Ok(meta) = MetaCache::global().get(path)
            && let Some(text) = meta.lyrics.as_deref()
        {
            return (parse_lyric(text, LyricOptions::default()), LyricsSource::Embedded);
        }
        (LyricsOutput { lines: Vec::new(), timed: false }, LyricsSource::None)
    }

    /// 曲目的外置歌词文件路径（同名 `.lrc`）。
    pub fn external_lrc(path: String) -> Option<String> {
        find_external_lrc(Path::new(&path)).map(|p| p.to_string_lossy().into_owned())
    }
}

/// 同目录同名 `.lrc`（大小写两种尝试）。
fn find_external_lrc(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let dir = path.parent()?;
    [format!("{stem}.lrc"), format!("{stem}.LRC")]
        .into_iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file())
}
