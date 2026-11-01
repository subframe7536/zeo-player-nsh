//! 引擎：`mod.rs` 是门面（`Player` 解码/输出/系统媒体会话、`Library` 多根
//! 曲库、`Meta` 歌词与波形），`audio` / `codec` / `media` 是引擎线程、解码
//! 与系统媒体会话的实现。纯 Rust 接口，UI（`crate::ui`）与测试直接调用。

pub(crate) mod audio;
pub(crate) mod codec;
mod media;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::config::{TrackEntry, library_store};
use crate::db::LastPlayed;
use crate::engine::audio::{EngineCmd, EngineHandle, Snapshot};
use crate::metadata::lyrics::{LyricOptions, LyricsOutput, parse_lyric};

/// 播放器引擎句柄。
pub struct Player {
    engine: EngineHandle,
    /// 上次写进数据库的播放状态（[`Player::remember`]）。
    memory: Mutex<Memory>,
}

/// 已保存的播放状态。
#[derive(Default)]
struct Memory {
    last: Option<LastPlayed>,
    playing: bool,
    queue: Option<Arc<Vec<String>>>,
}

impl Player {
    /// 新建播放器（启动解码/输出线程与系统媒体会话）。
    // 不实现 Default：新建会启动线程与系统媒体会话，不该隐式发生。
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Player {
            engine: EngineHandle::start(),
            memory: Mutex::new(Memory::default()),
        }
    }

    /// 进程内唯一的播放器（UI 用）。
    pub fn global() -> &'static Player {
        static PLAYER: OnceLock<Player> = OnceLock::new();
        PLAYER.get_or_init(Player::new)
    }

    /// 打开并播放 `path`；`queue` 为所在列表（上一首/下一首）。
    pub fn open(&self, path: String, queue: Vec<String>) {
        self.engine.send(EngineCmd::Load {
            path: PathBuf::from(path),
            queue,
        });
    }

    /// 装载 `path` 但不播放，停在 `time` 秒。
    pub fn cue(&self, path: String, queue: Vec<String>, time: f64) {
        self.engine.send(EngineCmd::Cue {
            path: PathBuf::from(path),
            queue,
            time,
        });
    }

    /// 恢复上次播放的曲目（暂停在上次的位置）：还没有曲目、文件还在时才做。
    /// 返回是否恢复了。
    pub fn restore_last(&self) -> bool {
        if self.state().path.is_some() {
            return false;
        }
        let Some((last, mut queue)) = crate::db::last_played() else {
            return false;
        };
        if !Path::new(&last.path).is_file() {
            return false;
        }
        if !queue.contains(&last.path) {
            queue = vec![last.path.clone()];
        }
        self.cue(last.path, queue, last.time.max(0.0));
        true
    }

    /// 记下播放状态（曲目、位置、队列），下次启动由 [`Self::restore_last`]
    /// 恢复。界面轮询时传入快照；换曲、播放 / 暂停、位置变了 2 秒以上或
    /// 队列变了才写（数据库写线程）。退出时不另外保存，最多丢 2 秒进度。
    pub fn remember(&self, snap: &Snapshot) {
        let Some(path) = snap.path.clone() else {
            return;
        };
        if snap.is_loading {
            return;
        }
        let last = LastPlayed {
            path,
            time: snap.current_time,
        };
        let mut memory = self.memory.lock().unwrap();
        let moved = memory.playing != snap.is_playing
            || memory
                .last
                .as_ref()
                .is_none_or(|old| old.path != last.path || (old.time - last.time).abs() >= 2.0);
        let queue = (!memory
            .queue
            .as_ref()
            .is_some_and(|q| Arc::ptr_eq(q, &snap.queue)))
        .then(|| snap.queue.clone());
        if !moved && queue.is_none() {
            return;
        }
        if moved {
            memory.last = Some(last.clone());
            memory.playing = snap.is_playing;
        }
        if let Some(queue) = &queue {
            memory.queue = Some(queue.clone());
        }
        drop(memory);
        crate::db::later(move || {
            if let Some(queue) = queue {
                crate::db::save_queue(&queue);
            }
            if moved {
                crate::db::save_last_played(&last);
            }
        });
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
        self.engine.shared.volume.store(
            (volume as f32).to_bits(),
            std::sync::atomic::Ordering::Relaxed,
        );
        self.engine.send(EngineCmd::SetVolume(volume));
    }

    /// 播放当前队列的第 `index` 首。
    pub fn jump(&self, index: usize) {
        self.engine.send(EngineCmd::Jump(index as i64));
    }

    pub fn next(&self) {
        self.engine.send(EngineCmd::Next);
    }

    pub fn previous(&self) {
        self.engine.send(EngineCmd::Previous);
    }

    /// 淡入淡出时长（毫秒；0 关闭），用于播放 / 暂停 / 跳转 / 切歌。
    pub fn set_fade_ms(&self, ms: u32) {
        self.engine
            .shared
            .fade_ms
            .store(ms.min(5000), std::sync::atomic::Ordering::Relaxed);
    }

    /// 设置循环模式（队列按新模式重排，当前曲目不变）。
    pub fn set_loop_mode(&self, mode: crate::engine::audio::LoopMode) {
        self.engine.send(EngineCmd::SetLoopMode(mode));
    }

    /// 切到下一个循环模式（顺序 → 单曲 → 随机）。
    pub fn cycle_loop_mode(&self) {
        self.set_loop_mode(self.engine.shared.loop_mode().cycle());
    }

    /// 当前循环模式。
    pub fn loop_mode(&self) -> crate::engine::audio::LoopMode {
        self.engine.shared.loop_mode()
    }

    /// 当前状态快照。
    pub fn state(&self) -> Snapshot {
        self.engine.snapshot()
    }
}

/// 本地曲库（多根目录、索引、流派拆分）。
#[derive(Default)]
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

/// 已解析歌词的小缓存：只留播放位置附近的几首（当前曲目 + 队尾预载）。
/// 歌词文本大，不进全局元数据缓存 / 数据库；条目带音频与 `.lrc` 的
/// 文件指纹，任一变了自动失效。命中时移到队尾（LRU）。
const LYRICS_CAP: usize = 8;
type LyricsCache = Mutex<Vec<(String, u64, Arc<(LyricsOutput, LyricsSource)>)>>;

fn lyrics_cache() -> &'static LyricsCache {
    static CACHE: OnceLock<LyricsCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

impl Meta {
    /// 歌词：外置同名 `.lrc` 优先，其次内嵌（按需 lofty 读，不进全局
    /// 缓存）；按 ap-spec 算法解析。结果进 [`lyrics_cache`]——在后台
    /// 线程调它即为队尾曲目的歌词预载。
    pub fn lyrics(path: &str) -> (LyricsOutput, LyricsSource) {
        let path = Path::new(path);
        let lrc = find_external_lrc(path);
        let prefer = library_store().prefer_local_lyrics();
        let stamp = |p: &Path| {
            std::fs::metadata(p).map_or(0, |m| crate::metadata::file_stamp(&m))
        };
        // 缓存键含 .lrc 指纹与「本地优先」开关：任一变化自动失效。
        let key = lrc
            .as_deref()
            .map_or_else(|| stamp(path), |l| stamp(path) ^ stamp(l).wrapping_mul(31))
            ^ prefer as u64;
        let name = path.to_string_lossy().into_owned();
        {
            let mut cache = lyrics_cache().lock().unwrap();
            if let Some(pos) = cache.iter().position(|(p, k, _)| *k == key && p == &name) {
                let entry = cache.remove(pos);
                let out = (*entry.2).clone();
                cache.push(entry);
                return out;
            }
        }
        let loaded = load_lyrics(path, lrc.as_deref(), prefer);
        let out = loaded.0.clone();
        let out_source = loaded.1;
        {
            let mut cache = lyrics_cache().lock().unwrap();
            cache.retain(|(p, _, _)| p != &name);
            cache.push((name, key, Arc::new(loaded)));
            while cache.len() > LYRICS_CAP {
                cache.remove(0);
            }
        }
        (out, out_source)
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

/// 真正读歌词：外置 `.lrc` 与内嵌按「本地优先」开关取序，首选读不到
/// 回退另一头；解析后由 [`Meta::lyrics`] 放进小缓存。
fn load_lyrics(
    path: &Path,
    lrc: Option<&Path>,
    prefer_local: bool,
) -> (LyricsOutput, LyricsSource) {
    let mut external = || {
        let text = std::fs::read_to_string(lrc?).ok()?;
        Some((
            parse_lyric(&text, LyricOptions::default()),
            LyricsSource::External,
        ))
    };
    let mut embedded = || {
        let text = crate::metadata::read_lyrics(path)?;
        Some((
            parse_lyric(&text, LyricOptions::default()),
            LyricsSource::Embedded,
        ))
    };
    let loaded = if prefer_local {
        external().or_else(&mut embedded)
    } else {
        embedded().or_else(&mut external)
    };
    loaded.unwrap_or((
        LyricsOutput {
            lines: Vec::new(),
            timed: false,
        },
        LyricsSource::None,
    ))
}
