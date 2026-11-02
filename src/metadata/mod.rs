//! 曲目派生数据：lofty 元数据读取（时长、标签、封面、技术参数，本文件）、
//! 封面缩略图缓存（`covers`）、LRC 歌词解析（`lyrics`）、波形条（`waveform`）。
//! 对齐 ap-spec 的 AudioMetadata 字段（title 缺省用文件名去扩展名、artist 缺省
//! Unknown Artist、album 缺省 Unknown Album；多 artist 用 `; ` 拼接）。

pub(crate) mod covers;
pub(crate) mod lyrics;
pub(crate) mod waveform;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::ItemKey;
use lofty::tag::Tag;

/// 完整元数据（ap-spec AudioMetadata + 库索引需要的字段）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub duration: f64,
    /// 是否有内嵌封面（字节不常驻内存：用 [`read_artwork`] 按需读取）。
    pub has_art: bool,
    pub bit_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub codec: Option<String>,
    pub quality: Option<String>,
    pub genres: Vec<String>,
    pub year: Option<u32>,
    pub track: Option<u32>,
    pub track_total: Option<u32>,
    pub disk: Option<u32>,
    pub disk_total: Option<u32>,
    pub album_artists: Vec<String>,
    pub composers: Vec<String>,
    /// 封面主色调（色相圈 0..1、饱和度 0..1）：不是标签，由 covers 工作线程
    /// 在解码封面时算好并写进同一行（`MetaCache::set_art_hue`）。
    pub art_hue: Option<(f32, f32)>,
}

fn strings(tag: &Tag, key: ItemKey) -> Vec<String> {
    tag.get_strings(key).map(|s| s.to_owned()).collect()
}

fn first(tag: &Tag, key: ItemKey) -> Option<String> {
    tag.get_string(key).map(|s| s.to_owned())
}

/// 多键合并去重后用 `; ` 拼接（参考站 tag.artists 拼接方式）。
fn joined(tag: &Tag, keys: &[ItemKey]) -> Option<String> {
    let mut all: Vec<String> = Vec::new();
    for key in keys {
        for value in strings(tag, *key) {
            if !all.contains(&value) {
                all.push(value);
            }
        }
    }
    if all.is_empty() {
        None
    } else {
        Some(all.join("; "))
    }
}

fn number(tag: &Tag, key: ItemKey) -> Option<u32> {
    tag.get_string(key)?.trim().parse().ok()
}

/// 解析 `n/n` 形式的音轨/碟号。
fn pair(tag: &Tag, key: ItemKey) -> (Option<u32>, Option<u32>) {
    match tag.get_string(key) {
        Some(raw) => {
            let raw = raw.trim();
            match raw.split_once('/') {
                Some((left, right)) => (left.trim().parse().ok(), right.trim().parse().ok()),
                None => (raw.parse().ok(), None),
            }
        }
        None => (None, None),
    }
}

fn codec_of(file_type: lofty::file::FileType) -> Option<&'static str> {
    use lofty::file::FileType::*;
    Some(match file_type {
        Mpeg => "MP3",
        Flac => "FLAC",
        Wav => "WAV",
        Ape => "APE",
        Opus => "OPUS",
        Vorbis => "Vorbis",
        Aac => "AAC",
        Mp4 => "MP4",
        WavPack => "WavPack",
        Aiff => "AIFF",
        _ => return None,
    })
}

/// 读取一个音频文件的元数据。
pub fn read_meta(path: &Path) -> Result<MetaInfo, String> {
    let tagged = lofty::read_from_path(path).map_err(|e| format!("lofty: {e}"))?;
    let props = tagged.properties();
    let file_type = tagged.file_type();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let file_stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Unknown".to_owned());

    let mut info = MetaInfo {
        title: file_stem,
        artist: "Unknown Artist".to_owned(),
        album: "Unknown Album".to_owned(),
        album_artist: String::new(),
        duration: props.duration().as_secs_f64(),
        has_art: false,
        bit_rate: props.overall_bitrate().or(props.audio_bitrate()),
        bit_depth: props.bit_depth().map(u32::from),
        sample_rate: props.sample_rate(),
        channels: props.channels().map(u32::from),
        codec: codec_of(file_type).map(|s| s.to_owned()),
        quality: match file_type {
            lofty::file::FileType::Flac
            | lofty::file::FileType::Wav
            | lofty::file::FileType::Aiff => Some("Lossless".to_owned()),
            lofty::file::FileType::Mpeg => Some("Lossy".to_owned()),
            _ => None,
        },
        genres: Vec::new(),
        year: None,
        track: None,
        track_total: None,
        disk: None,
        disk_total: None,
        album_artists: Vec::new(),
        composers: Vec::new(),
        art_hue: None,
    };

    if let Some(tag) = tag {
        if let Some(title) = first(tag, ItemKey::TrackTitle)
            && !title.trim().is_empty()
        {
            info.title = title;
        }
        if let Some(artist) = joined(tag, &[ItemKey::TrackArtist, ItemKey::TrackArtists]) {
            info.artist = artist;
        }
        if let Some(album) = first(tag, ItemKey::AlbumTitle)
            && !album.trim().is_empty()
        {
            info.album = album;
        }
        if let Some(album_artist) = joined(tag, &[ItemKey::AlbumArtist, ItemKey::AlbumArtists]) {
            info.album_artist = album_artist;
        }
        info.genres = strings(tag, ItemKey::Genre);
        info.year = number(tag, ItemKey::Year).or_else(|| number(tag, ItemKey::RecordingDate));
        let (track, track_total) = pair(tag, ItemKey::TrackNumber);
        info.track = track;
        info.track_total = track_total;
        let (disk, disk_total) = pair(tag, ItemKey::DiscNumber);
        info.disk = disk;
        info.disk_total = disk_total;
        info.album_artists = strings(tag, ItemKey::AlbumArtist);
        info.composers = strings(tag, ItemKey::Composer);
        info.has_art = !tag.pictures().is_empty();
    }

    Ok(info)
}

/// 按需读取内嵌封面（原始字节 + MIME）。不进缓存：用完即释放。
pub fn read_artwork(path: &Path) -> Option<(Vec<u8>, Option<String>)> {
    let tagged = lofty::read_from_path(path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let picture = tag
        .pictures()
        .first()
        .or_else(|| tagged.tags().iter().flat_map(|t| t.pictures()).next())?;
    Some((
        picture.data().to_vec(),
        picture.mime_type().map(|m| m.as_str().to_owned()),
    ))
}

/// 按需读取内嵌歌词原文（同 [`read_artwork`]：用完即释放）。歌词不进
/// 全局元数据缓存 / 数据库——全库常驻歌词文本太占内存，只缓存播放
/// 位置附近的几首（`engine::Meta::lyrics`）。
pub fn read_lyrics(path: &Path) -> Option<String> {
    let tagged = lofty::read_from_path(path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    first(tag, ItemKey::Lyrics).or_else(|| first(tag, ItemKey::UnsyncLyrics))
}

/// 文件指纹：修改时间（纳秒）、大小，再加 Unix 的 inode 与 ctime 或
/// Windows 的创建时间。复制时保留修改时间的工具（Finder、`cp -p`、
/// 资源管理器）改不了 ctime / 新文件的创建时间，所以内容换了总能发现。
pub fn file_stamp(meta: &std::fs::Metadata) -> u64 {
    let nanos = |t: std::io::Result<std::time::SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos() as u64)
    };
    #[allow(unused_mut)]
    let mut parts = vec![nanos(meta.modified()), meta.len()];
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        parts.push(meta.creation_time());
        parts.push(meta.last_write_time());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        parts.push(meta.ino());
        parts.push(
            (meta.ctime() as u64)
                .wrapping_mul(1_000_000_000)
                .wrapping_add(meta.ctime_nsec() as u64),
        );
    }
    parts
        .iter()
        .flat_map(|p| p.to_le_bytes())
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

// --- 元数据缓存（按 path + 文件指纹，持久化到 SQLite） -----------------------

#[derive(Clone)]
pub struct CacheEntry {
    /// 文件指纹（[`file_stamp`]）。
    pub stamp: u64,
    pub size: u64,
    /// 修改时间（Unix 秒）。
    pub modified: u64,
    pub value: Arc<MetaInfo>,
}

/// 新读出、还没写进数据库的条目超过这个数就写一次（扫描中）。
const FLUSH_AT: usize = 256;

pub struct MetaCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
    /// 新读出、还没写进数据库的条目。
    pending: Mutex<Vec<(String, CacheEntry)>>,
    /// 扫描中：攒一批再写（见 [`Self::begin_batch`]）。
    batching: AtomicBool,
}

impl MetaCache {
    /// 进程内唯一的缓存；第一次用时从数据库读出上次保存的全部元数据。
    pub fn global() -> &'static MetaCache {
        static CACHE: OnceLock<MetaCache> = OnceLock::new();
        CACHE.get_or_init(|| MetaCache {
            entries: Mutex::new(crate::db::load_meta().into_iter().collect()),
            pending: Mutex::new(Vec::new()),
            batching: AtomicBool::new(false),
        })
    }

    /// 全部条目（启动时据此先显示上次的曲库）。
    pub fn entries(&self) -> Vec<(String, CacheEntry)> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// 丢掉全部缓存，数据库里的也删掉（重建曲库）。
    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
        self.pending.lock().unwrap().clear();
        crate::db::clear_meta();
    }

    /// 移除单条缓存（界面「删除记录」）：连待写队列里同一路径的条目
    /// 一起丢，否则下次 flush 的 `INSERT OR REPLACE` 会把它写回。
    pub fn remove(&self, path: &Path) {
        let key = path.to_string_lossy().into_owned();
        self.entries.lock().unwrap().remove(&key);
        self.pending.lock().unwrap().retain(|(p, _)| *p != key);
    }

    /// 移除 `path` 下（含自身）的全部条目（文件夹删除 / 根摘除）：连
    /// 待写队列与数据库行一起清，文件还在时下次扫描会重新入库。
    pub fn remove_under(&self, path: &str) {
        // 先把待写的旧键落库：否则删完它们还会以旧路径写回一条死行。
        self.flush();
        let sep = std::path::MAIN_SEPARATOR;
        let trimmed = path.trim_end_matches(sep);
        let under = format!("{trimmed}{sep}");
        self.entries
            .lock()
            .unwrap()
            .retain(|p, _| p != trimmed && !p.starts_with(&under));
        self.pending
            .lock()
            .unwrap()
            .retain(|(p, _)| p != trimmed && !p.starts_with(&under));
        crate::db::remove_meta_under(trimmed);
    }

    /// `rows = (旧路径, 新路径, 新指纹, 新修改时间)`：文件 / 文件夹移
    /// 动或改名后把缓存键换到新路径并带上新指纹（改名改了 ctime，不换
    /// 指纹下次扫描会误判内容变了重读标签）。数据库更新由调用方排。
    pub fn move_path(&self, rows: &[(String, String, u64, u64)]) {
        if rows.is_empty() {
            return;
        }
        // 先把待写的旧键落库：数据库随后整体 UPDATE 到新键；不然
        // pending 里的旧键会在换键之后被 flush 写回一条死行。
        self.flush();
        {
            let mut entries = self.entries.lock().unwrap();
            for (old, new, stamp, modified) in rows {
                if let Some(mut e) = entries.remove(old) {
                    e.stamp = *stamp;
                    e.modified = *modified;
                    entries.insert(new.clone(), e);
                }
            }
        }
        // 待写队列同一路径的也换：不然下次 flush 会按旧路径写回一条死行。
        let mut pending = self.pending.lock().unwrap();
        for (old, new, stamp, modified) in rows {
            for (p, e) in pending.iter_mut() {
                if p == old {
                    *p = new.clone();
                    e.stamp = *stamp;
                    e.modified = *modified;
                }
            }
        }
    }

    /// 只留下 `keep` 里的路径（扫描后去掉已不存在的文件）。
    pub fn retain(&self, keep: &HashSet<String>) {
        self.flush();
        self.entries
            .lock()
            .unwrap()
            .retain(|path, _| keep.contains(path));
        crate::db::retain_meta(keep);
    }

    /// 开始批量读取：新条目攒够 [`FLUSH_AT`] 条才写一次数据库。
    pub fn begin_batch(&self) {
        self.batching.store(true, Ordering::Relaxed);
    }

    /// 结束批量读取，写入剩下的条目。
    pub fn end_batch(&self) {
        self.batching.store(false, Ordering::Relaxed);
        self.flush();
    }

    fn flush(&self) {
        let rows = std::mem::take(&mut *self.pending.lock().unwrap());
        crate::db::save_meta(&rows);
    }

    /// 读取（带缓存）：文件未变时直接返回缓存。
    pub fn get(&self, path: &Path) -> Result<Arc<MetaInfo>, String> {
        self.entry(path).map(|e| e.value)
    }

    /// `path` 已存的封面主色调（同步读，界面用）。
    pub fn art_hue(&self, path: &Path) -> Option<(f32, f32)> {
        self.entries
            .lock()
            .unwrap()
            .get(path.to_string_lossy().as_ref())?
            .value
            .art_hue
    }

    /// 写封面主色调（covers 工作线程）：条目在且文件指纹未变才写
    /// （指纹变了说明算的是旧封面，等下次读取重建）。写入走 `pending`
    /// 与元数据同一批落库。
    pub fn set_art_hue(&self, path: &Path, hue: Option<(f32, f32)>) {
        let stamp = std::fs::metadata(path).map(|m| file_stamp(&m)).unwrap_or(0);
        let key = path.to_string_lossy().into_owned();
        let row = {
            let mut entries = self.entries.lock().unwrap();
            let Some(entry) = entries.get_mut(&key) else {
                return;
            };
            if entry.stamp != stamp || entry.value.art_hue == hue {
                return;
            }
            let mut info = (*entry.value).clone();
            info.art_hue = hue;
            entry.value = Arc::new(info);
            (key, entry.clone())
        };
        let flush = {
            let mut pending = self.pending.lock().unwrap();
            pending.push(row);
            !self.batching.load(Ordering::Relaxed) || pending.len() >= FLUSH_AT
        };
        if flush {
            self.flush();
        }
    }

    /// 同 [`Self::get`]，连同文件指纹、大小与修改时间。
    pub fn entry(&self, path: &Path) -> Result<CacheEntry, String> {
        let key = path.to_string_lossy().into_owned();
        let file = std::fs::metadata(path).ok();
        let stamp = file.as_ref().map_or(0, file_stamp);
        if let Some(entry) = self.entries.lock().unwrap().get(&key)
            && entry.stamp == stamp
        {
            return Ok(entry.clone());
        }
        let entry = CacheEntry {
            stamp,
            size: file.as_ref().map_or(0, |m| m.len()),
            modified: file
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs()),
            value: Arc::new(read_meta(path)?),
        };
        self.entries
            .lock()
            .unwrap()
            .insert(key.clone(), entry.clone());
        let flush = {
            let mut pending = self.pending.lock().unwrap();
            pending.push((key, entry.clone()));
            !self.batching.load(Ordering::Relaxed) || pending.len() >= FLUSH_AT
        };
        if flush {
            self.flush();
        }
        Ok(entry)
    }
}
