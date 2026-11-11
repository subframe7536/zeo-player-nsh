//! 曲目标签与派生数据的共享形态：lofty 读出的完整 [`MetaInfo`]、
//! [`ReplayGain`]、SQLite 缓存行 [`CacheEntry`]、文件指纹 [`file_stamp`]。
//! 读取与缓存逻辑在 `zeo-metadata` crate。

use std::sync::Arc;

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
    pub conductor: Option<String>,
    pub lyricist: Option<String>,
    pub publisher: Option<String>,
    pub copyright: Option<String>,
    pub comment: Option<String>,
    pub rating: Option<u32>,
    pub lyrics: Option<String>,
    /// 封面主色调（色相圈 0..1、饱和度 0..1）：不是标签，由 covers 工作线程
    /// 在解码封面时算好并写进同一行（`MetaCache::set_art_hue`）。
    pub art_hue: Option<(f32, f32)>,
    /// ReplayGain 标签（音量标准化；没有的项为 `None`）。
    pub replaygain: ReplayGain,
}

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
