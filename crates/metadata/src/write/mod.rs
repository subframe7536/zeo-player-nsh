//! 标签写回：先把原文件复制到同目录的临时文件 `.<文件名>.zeo-tmp`，在副本上
//! 用 lofty 改标签，成功后 `rename` 原子覆盖原文件；任何一步失败都删掉
//! 临时文件、原文件不动。
//!
//! 标签的选择与字段映射与 music-tag-native 一致：写主标签
//! （`primary_tag_type()`），没有主标签时先写读取用的那张（非 ID3v1，
//! 它存不下才落到新建的主标签——新标签先复制原标签的条目与图片）；
//! ID3v1 只同步、不直接接收写入。

mod apply;
mod atomic;
mod read;
mod sidecar;
#[cfg(test)]
mod tests_edge;
#[cfg(test)]
mod tests_tags;
#[cfg(test)]
mod testutil;

pub(crate) use apply::apply_tag_edits;
pub use apply::write_tags;
pub(crate) use atomic::*;
pub use read::{read_fields, read_row};
pub(crate) use sidecar::*;

use std::fs;
use std::path::Path;

use lofty::tag::ItemKey;

/// 可编辑的标签字段（「编辑元数据」对话框的每个输入对应一个）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TagField {
    Title,
    Artist,
    Album,
    AlbumArtist,
    Genre,
    Year,
    Track,
    TrackTotal,
    Disc,
    DiscTotal,
    Composer,
    Comment,
    Lyrics,
    Conductor,
    Lyricist,
    Publisher,
    Copyright,
    Rating,
    TrackGain,
    TrackPeak,
    AlbumGain,
    AlbumPeak,
}

impl TagField {
    /// 全部字段（[`read_fields`] 返回的键集合）。
    pub const ALL: [TagField; 22] = [
        TagField::Title,
        TagField::Artist,
        TagField::Album,
        TagField::AlbumArtist,
        TagField::Genre,
        TagField::Year,
        TagField::Track,
        TagField::TrackTotal,
        TagField::Disc,
        TagField::DiscTotal,
        TagField::Composer,
        TagField::Comment,
        TagField::Lyrics,
        TagField::Conductor,
        TagField::Lyricist,
        TagField::Publisher,
        TagField::Copyright,
        TagField::Rating,
        TagField::TrackGain,
        TagField::TrackPeak,
        TagField::AlbumGain,
        TagField::AlbumPeak,
    ];

    /// 界面与报错里的中文字段名。
    pub fn label(self) -> &'static str {
        match self {
            TagField::Title => "标题",
            TagField::Artist => "歌手",
            TagField::Album => "专辑",
            TagField::AlbumArtist => "专辑歌手",
            TagField::Genre => "流派",
            TagField::Year => "年份",
            TagField::Track => "音轨号",
            TagField::TrackTotal => "音轨总数",
            TagField::Disc => "碟号",
            TagField::DiscTotal => "碟总数",
            TagField::Composer => "作曲",
            TagField::Comment => "注释",
            TagField::Lyrics => "歌词",
            TagField::Conductor => "指挥",
            TagField::Lyricist => "作词",
            TagField::Publisher => "发行商",
            TagField::Copyright => "版权",
            TagField::Rating => "评分",
            TagField::TrackGain => "音轨增益",
            TagField::TrackPeak => "音轨峰值",
            TagField::AlbumGain => "专辑增益",
            TagField::AlbumPeak => "专辑峰值",
        }
    }

    /// 直接写文本的 `ItemKey`（Artist / AlbumArtist / Year 走特殊路径，
    /// 这里给 `Clear` 删除主键用）。
    fn key(self) -> ItemKey {
        match self {
            TagField::Title => ItemKey::TrackTitle,
            TagField::Artist => ItemKey::TrackArtist,
            TagField::Album => ItemKey::AlbumTitle,
            TagField::AlbumArtist => ItemKey::AlbumArtist,
            TagField::Genre => ItemKey::Genre,
            TagField::Year => ItemKey::Year,
            TagField::Track => ItemKey::TrackNumber,
            TagField::TrackTotal => ItemKey::TrackTotal,
            TagField::Disc => ItemKey::DiscNumber,
            TagField::DiscTotal => ItemKey::DiscTotal,
            TagField::Composer => ItemKey::Composer,
            TagField::Comment => ItemKey::Comment,
            TagField::Lyrics => ItemKey::Lyrics,
            TagField::Conductor => ItemKey::Conductor,
            TagField::Lyricist => ItemKey::Lyricist,
            TagField::Publisher => ItemKey::Publisher,
            TagField::Copyright => ItemKey::CopyrightMessage,
            TagField::Rating => ItemKey::Popularimeter,
            TagField::TrackGain => ItemKey::ReplayGainTrackGain,
            TagField::TrackPeak => ItemKey::ReplayGainTrackPeak,
            TagField::AlbumGain => ItemKey::ReplayGainAlbumGain,
            TagField::AlbumPeak => ItemKey::ReplayGainAlbumPeak,
        }
    }
}

/// 一个字段的改动。
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// 不动（单编辑对话框不产生；批量编辑「保持原值」的占位用它）。
    Keep,
    /// 设成新值；空白串与 `Clear` 等价。
    Set(String),
    /// 删掉该字段。
    Clear,
}

/// 封面的改动（`TagEdits::cover` 为 `None` 即不动）。
#[derive(Debug, Clone, PartialEq)]
pub enum CoverEdit {
    /// 换成新的封面（原始字节 + MIME 类型）；会替换掉文件里的全部内嵌图片——
    /// 界面只把第一张当封面，留着旧的多类型图片换完还显示旧图。
    Replace { data: Vec<u8>, mime: String },
    /// 删掉全部内嵌图片。
    Remove,
}

/// 一次写回的改动集合；批量编辑（plan 012）对每首复用同一结构。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TagEdits {
    pub fields: Vec<(TagField, Edit)>,
    /// 内嵌封面的改动（`None` 不动）。
    pub cover: Option<CoverEdit>,
    /// 同名图片侧车文件的改动（`None` 不动；它不是标签，是旁边的
    /// 图片文件，跟标签分开写——`.lrc` 同款）。`Replace` 的 `mime`
    /// 决定后缀，bmp / gif 转码成 PNG 再写。
    pub cover_file: Option<CoverEdit>,
    /// 同名 `.lrc` 侧车歌词文件的改动（`None` 不动；`.lrc` 不是标签，
    /// 是旁边的文本文件，跟标签分开写）。
    pub lrc: Option<Edit>,
}

impl TagEdits {
    /// 有没有要动音频文件的部分（没有就只写侧车，不复制原文件）。
    fn has_tag_changes(&self) -> bool {
        self.fields.iter().any(|(_, e)| *e != Edit::Keep) || self.cover.is_some()
    }

    /// 没有任何改动（批量编辑逐条跳过空改动用）。
    pub fn is_empty(&self) -> bool {
        !self.has_tag_changes() && self.cover_file.is_none() && self.lrc.is_none()
    }
}

/// 用户选的封面图片 → MIME 类型：先看扩展名，不认得的按文件头猜。
/// 认不出来的返回 `None`（调用方拒绝这张图片）。
pub fn cover_mime(path: &Path) -> Option<String> {
    let by_ext = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("bmp") => Some("image/bmp"),
        Some("webp") => Some("image/webp"),
        _ => None,
    };
    by_ext.map(str::to_owned).or_else(|| {
        use std::io::Read;
        let mut head = [0u8; 12];
        let n = fs::File::open(path).ok()?.read(&mut head).ok()?;
        let head = &head[..n];
        let mime = if head.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if head.starts_with(b"\xff\xd8\xff") {
            "image/jpeg"
        } else if head.starts_with(b"GIF8") {
            "image/gif"
        } else if head.starts_with(b"BM") {
            "image/bmp"
        } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
            "image/webp"
        } else {
            return None;
        };
        Some(mime.to_owned())
    })
}
