//! lofty 元数据读取：时长、标签、封面、技术参数。对齐 ap-spec 的
//! AudioMetadata 字段（title 缺省用文件名去扩展名、artist 缺省
//! Unknown Artist、album 缺省 Unknown Album；多 artist 用 `; ` 拼接）。

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::ItemKey;
use lofty::tag::Tag;

/// 完整元数据（ap-spec AudioMetadata + 库索引需要的字段）。
#[derive(Debug, Clone)]
pub struct MetaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub duration: f64,
    /// 是否有内嵌封面（字节不常驻内存：用 [`read_artwork`] 按需读取）。
    pub has_art: bool,
    /// 内嵌歌词原文。
    pub lyrics: Option<String>,
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
    if all.is_empty() { None } else { Some(all.join("; ")) }
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
        lyrics: None,
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
        info.lyrics = first(tag, ItemKey::Lyrics).or_else(|| first(tag, ItemKey::UnsyncLyrics));
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
    let picture = tag.pictures().first().or_else(|| tagged.tags().iter().flat_map(|t| t.pictures()).next())?;
    Some((picture.data().to_vec(), picture.mime_type().map(|m| m.as_str().to_owned())))
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
        parts.push((meta.ctime() as u64).wrapping_mul(1_000_000_000).wrapping_add(meta.ctime_nsec() as u64));
    }
    parts
        .iter()
        .flat_map(|p| p.to_le_bytes())
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

// --- 元数据缓存（按 path + 文件指纹） ---------------------------------------

#[derive(Clone)]
pub struct CacheEntry {
    pub modified: u64,
    pub value: Arc<MetaInfo>,
}

pub struct MetaCache {
    entries: Mutex<std::collections::HashMap<String, CacheEntry>>,
}

impl MetaCache {
    pub fn global() -> &'static MetaCache {
        static CACHE: OnceLock<MetaCache> = OnceLock::new();
        CACHE.get_or_init(|| MetaCache { entries: Mutex::new(std::collections::HashMap::new()) })
    }


    /// 丢掉全部缓存（重建曲库）。
    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }

    /// 读取（带缓存）：文件未变时直接返回缓存。
    pub fn get(&self, path: &Path) -> Result<Arc<MetaInfo>, String> {
        let key = path.to_string_lossy().into_owned();
        let modified = std::fs::metadata(path).map(|m| file_stamp(&m)).unwrap_or(0);
        if let Some(entry) = self.entries.lock().unwrap().get(&key)
            && entry.modified == modified
        {
            return Ok(entry.value.clone());
        }
        let meta = Arc::new(read_meta(path)?);
        self.entries.lock().unwrap().insert(key, CacheEntry { modified, value: meta.clone() });
        Ok(meta)
    }
}
