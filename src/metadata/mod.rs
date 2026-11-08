//! 曲目派生数据：lofty 元数据读取（时长、标签、封面、技术参数，本文件）、
//! 封面缩略图缓存（`covers`）、LRC 歌词解析（`lyrics`）、波形条（`waveform`）。
//! 对齐 ap-spec 的 AudioMetadata 字段（title 缺省用文件名去扩展名、artist 缺省
//! Unknown Artist、album 缺省 Unknown Album；多 artist 用 `; ` 拼接）。

pub(crate) mod covers;
pub(crate) mod lyrics;
pub(crate) mod rename;
pub(crate) mod waveform;
pub(crate) mod write;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::ItemKey;
use lofty::tag::Tag;

/// ReplayGain 标签（dB 与线性峰值）；没有的项为 `None`。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ReplayGain {
    pub track_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<f32>,
    pub album_peak: Option<f32>,
}

impl ReplayGain {
    /// 四项全空（没读到任何 RG 标签，或迁移前的老行）。
    pub fn is_empty(&self) -> bool {
        self.track_gain.is_none()
            && self.track_peak.is_none()
            && self.album_gain.is_none()
            && self.album_peak.is_none()
    }
}

/// `-6.54 dB` / `+1.2dB` / `-6.54` → -6.54；解析失败 `None`。
pub(crate) fn parse_gain(text: &str) -> Option<f32> {
    let lower = text.trim().to_lowercase();
    let digits = lower.strip_suffix("db").unwrap_or(&lower).trim();
    let value: f32 = digits.parse().ok()?;
    (value.is_finite() && (-60.0..=60.0).contains(&value)).then_some(value)
}

/// `0.988525` → 0.988525；非正数或解析失败 `None`。
pub(crate) fn parse_peak(text: &str) -> Option<f32> {
    let value: f32 = text.trim().parse().ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

/// 先查主标签再查其余标签，取第一个能解析的值。
fn rg_from_tagged(tagged: &lofty::file::TaggedFile) -> ReplayGain {
    let find = |key: ItemKey, parse: fn(&str) -> Option<f32>| {
        tagged
            .primary_tag()
            .into_iter()
            .chain(tagged.tags().iter())
            .find_map(|tag| first(tag, key).and_then(|text| parse(&text)))
    };
    ReplayGain {
        track_gain: find(ItemKey::ReplayGainTrackGain, parse_gain),
        track_peak: find(ItemKey::ReplayGainTrackPeak, parse_peak),
        album_gain: find(ItemKey::ReplayGainAlbumGain, parse_gain),
        album_peak: find(ItemKey::ReplayGainAlbumPeak, parse_peak),
    }
}

/// 独立按路径读一次 ReplayGain（引擎对迁移前的老行回退时用）。
pub fn read_replaygain(path: &Path) -> ReplayGain {
    lofty::read_from_path(path)
        .map(|tagged| rg_from_tagged(&tagged))
        .unwrap_or_default()
}

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
    /// ReplayGain 标签（音量标准化；没有的项为 `None`）。
    pub replaygain: ReplayGain,
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

    let mut codec = codec_of(file_type).map(|s| s.to_owned());
    let quality = match file_type {
        lofty::file::FileType::Flac | lofty::file::FileType::Wav | lofty::file::FileType::Aiff => {
            Some("Lossless".to_owned())
        }
        lofty::file::FileType::Mpeg
        | lofty::file::FileType::Vorbis
        | lofty::file::FileType::Aac => Some("Lossy".to_owned()),
        // M4A 可能是 AAC 也可能是 ALAC：按位深推断（ALAC 有位深，AAC 没有）。
        lofty::file::FileType::Mp4 => {
            if props.bit_depth().is_some() {
                codec = Some("ALAC".to_owned());
                Some("Lossless".to_owned())
            } else {
                codec = Some("AAC".to_owned());
                Some("Lossy".to_owned())
            }
        }
        _ => None,
    };

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
        codec,
        quality,
        genres: Vec::new(),
        year: None,
        track: None,
        track_total: None,
        disk: None,
        disk_total: None,
        album_artists: Vec::new(),
        composers: Vec::new(),
        art_hue: None,
        // 文件已打开，顺手读（无额外 I/O）。
        replaygain: rg_from_tagged(&tagged),
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
        info.track_total = track_total.or_else(|| number(tag, ItemKey::TrackTotal));
        let (disk, disk_total) = pair(tag, ItemKey::DiscNumber);
        info.disk = disk;
        info.disk_total = disk_total.or_else(|| number(tag, ItemKey::DiscTotal));
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
    /// 隐藏到托盘时丢过内存条目（[`Self::drop_in_memory`]）：置位期间
    /// `entries` 全量需求从数据库整体回填，`entry` / `art_hue` /
    /// `set_art_hue` 按需走数据库单行读写。
    dropped: AtomicBool,
}

impl MetaCache {
    /// 进程内唯一的缓存；第一次用时从数据库读出上次保存的全部元数据。
    pub fn global() -> &'static MetaCache {
        static CACHE: OnceLock<MetaCache> = OnceLock::new();
        CACHE.get_or_init(|| MetaCache {
            entries: Mutex::new(crate::db::load_meta().into_iter().collect()),
            pending: Mutex::new(Vec::new()),
            batching: AtomicBool::new(false),
            dropped: AtomicBool::new(false),
        })
    }

    /// 全部条目（启动时据此先显示上次的曲库）。
    pub fn entries(&self) -> Vec<(String, CacheEntry)> {
        let mut entries = self.entries.lock().unwrap();
        if self.dropped.swap(false, Ordering::Relaxed) {
            // 隐藏期间丢过内存条目：曲库索引重建这类全量需求本就需要
            // 全部条目，从数据库整体回填（单行按需回填见 `entry`）。
            *entries = crate::db::load_meta().into_iter().collect();
        }
        entries
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

    /// 只丢内存条目（隐藏到托盘时调）：数据库行保留，`entry` /
    /// `art_hue` / `set_art_hue` 按需单行回填与直写——引擎后台换曲、
    /// 封面工作线程照常工作，但只有被碰到的条目重新占内存。
    pub fn drop_in_memory(&self) {
        self.flush();
        let mut entries = self.entries.lock().unwrap();
        // 标志在持锁状态下先置位再清表：否则「清空」与「置位」之间的
        // `entries` 读方会拿到空表且错过回填。
        self.dropped.store(true, Ordering::Relaxed);
        entries.clear();
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

    /// `path` 已存的封面主色调（同步读，界面用）。内存条目被隐藏丢
    /// 掉后回退到数据库单行查——色调本来就在库里，不该因丢缓存重算。
    pub fn art_hue(&self, path: &Path) -> Option<(f32, f32)> {
        let key = path.to_string_lossy().into_owned();
        if let Some(hue) = self
            .entries
            .lock()
            .unwrap()
            .get(key.as_str())
            .and_then(|e| e.value.art_hue)
        {
            return Some(hue);
        }
        if self.dropped.load(Ordering::Relaxed) {
            return crate::db::meta_hue(&key);
        }
        None
    }

    /// 写封面主色调（covers 工作线程）：条目在且文件指纹未变才写
    /// （指纹变了说明算的是旧封面，等下次读取重建）。写入走 `pending`
    /// 与元数据同一批落库；条目被隐藏丢掉时按同指纹直写库。
    pub fn set_art_hue(&self, path: &Path, hue: Option<(f32, f32)>) {
        let stamp = std::fs::metadata(path).map(|m| file_stamp(&m)).unwrap_or(0);
        let key = path.to_string_lossy().into_owned();
        let row = {
            let mut entries = self.entries.lock().unwrap();
            let Some(entry) = entries.get_mut(&key) else {
                drop(entries);
                // 内存条目被隐藏丢掉：不重建条目，同指纹直写库（`WHERE
                // stamp` 与上面的指纹检查同一语义）。未丢弃时条目缺失
                // = 库里也没有（表随启动装载且只增），保持原静默返回。
                if self.dropped.load(Ordering::Relaxed) {
                    crate::db::set_meta_hue(&key, stamp, hue);
                }
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
        // 内存条目可能被隐藏丢掉：先试数据库行（指纹一致直接用，免
        // 重读文件）；已在库的条目不进 `pending` 再写一遍。
        if let Some(entry) = crate::db::load_meta_row(&key)
            && entry.stamp == stamp
        {
            self.entries
                .lock()
                .unwrap()
                .insert(key.clone(), entry.clone());
            return Ok(entry);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// ReplayGain 标签的常见写法：带 / 不带 dB 后缀、前导加号、空白。
    #[test]
    fn replaygain_parses_common_forms() {
        assert_eq!(parse_gain("-6.54 dB"), Some(-6.54));
        assert_eq!(parse_gain("+1.2dB"), Some(1.2));
        assert_eq!(parse_gain(" -3 "), Some(-3.0));
        assert_eq!(parse_gain("abc"), None);
        assert_eq!(parse_gain("nan"), None);
        assert_eq!(parse_gain("-100 dB"), None, "超界");
        assert_eq!(parse_peak("0.988525"), Some(0.988525));
        assert_eq!(parse_peak("0"), None, "峰值必须为正");
        assert_eq!(parse_peak("abc"), None);
    }

    /// 带 RG 标签的语料样本（scripts/make-test-music.sh 生成）：
    /// read_meta 与 read_replaygain 都读出相同的值。
    #[test]
    fn replaygain_reads_flac_tags() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(".nsh/tmp/zeo-player-test-formats/rg.flac");
        if !path.is_file() {
            eprintln!("跳过：没有 zeo-player-test-formats 测试语料");
            return;
        }
        for rg in [
            read_meta(&path).expect("read_meta").replaygain,
            read_replaygain(&path),
        ] {
            assert_eq!(rg.track_gain, Some(-6.0));
            assert_eq!(rg.track_peak, Some(0.5));
            assert_eq!(rg.album_gain, Some(-3.0));
            assert_eq!(rg.album_peak, None);
        }
    }
}
