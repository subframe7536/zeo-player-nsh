use serde::{Deserialize, Serialize};

use std::path::Path;

use zeo_types::config::TrackEntry;

/// 条件可用的字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    // 元数据
    Title,
    Artist,
    Album,
    AlbumArtist,
    /// 标签：genre 按设置的分隔符拆开的值。文件里沿用旧名 `genre`。
    #[serde(rename = "genre")]
    Tag,
    Year,
    // 音频
    Format,
    Duration,
    SampleRate,
    BitRate,
    Channels,
    // 文件
    Folder,
    FileName,
    Size,
    Modified,
}

/// 字段的取值类型（决定可用的比较方式与值的解析）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Text,
    Number,
    /// 修改时间：按「天前」比较。
    Date,
}

impl Field {
    /// 选择器里的顺序，按组：元数据、音频、文件。
    pub const ALL: [Field; 15] = [
        Field::Title,
        Field::Artist,
        Field::Album,
        Field::AlbumArtist,
        Field::Tag,
        Field::Year,
        Field::Format,
        Field::Duration,
        Field::SampleRate,
        Field::BitRate,
        Field::Channels,
        Field::Folder,
        Field::FileName,
        Field::Size,
        Field::Modified,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::Title => "标题",
            Field::Artist => "歌手",
            Field::Album => "专辑",
            Field::AlbumArtist => "专辑歌手",
            Field::Tag => "标签",
            Field::Year => "年份",
            Field::Format => "格式",
            Field::Duration => "时长",
            Field::SampleRate => "采样率",
            Field::BitRate => "比特率",
            Field::Channels => "声道数",
            Field::Folder => "文件夹",
            Field::FileName => "文件名",
            Field::Size => "文件大小",
            Field::Modified => "修改时间",
        }
    }

    /// 分组名（选择器里的小标题）。
    pub fn group(self) -> &'static str {
        match self {
            Field::Title
            | Field::Artist
            | Field::Album
            | Field::AlbumArtist
            | Field::Tag
            | Field::Year => "元数据",
            Field::Format
            | Field::Duration
            | Field::SampleRate
            | Field::BitRate
            | Field::Channels => "音频",
            Field::Folder | Field::FileName | Field::Size | Field::Modified => "文件",
        }
    }

    pub fn kind(self) -> FieldType {
        match self {
            Field::Year
            | Field::Duration
            | Field::SampleRate
            | Field::BitRate
            | Field::Channels
            | Field::Size => FieldType::Number,
            Field::Modified => FieldType::Date,
            _ => FieldType::Text,
        }
    }

    /// 值的单位（输入框旁的提示）。
    pub fn unit(self) -> &'static str {
        match self {
            Field::Duration => "分:秒 或 秒",
            Field::SampleRate => "kHz",
            Field::BitRate => "kbps",
            Field::Size => "MB",
            Field::Modified => "天",
            _ => "",
        }
    }

    /// 可用的比较方式（第一个为默认）。
    pub fn ops(self) -> &'static [Op] {
        match self.kind() {
            // 格式与标签的值都从列表里选，只有「是 / 不是」有意义。
            FieldType::Text if matches!(self, Field::Format | Field::Tag) => &[Op::Is, Op::IsNot],
            FieldType::Text => &[
                Op::Contains,
                Op::NotContains,
                Op::Is,
                Op::IsNot,
                Op::StartsWith,
                Op::EndsWith,
            ],
            FieldType::Number => &[Op::Is, Op::IsNot, Op::Greater, Op::Less],
            FieldType::Date => &[Op::InLast, Op::NotInLast],
        }
    }

    /// 文本字段的取值（标签可能有多个）。
    pub(crate) fn texts(self, t: &TrackEntry) -> Vec<String> {
        match self {
            Field::Title => vec![t.title.clone()],
            Field::Artist => vec![t.artist.clone()],
            Field::Album => vec![t.album.clone()],
            Field::AlbumArtist => vec![t.album_artist.clone()],
            Field::Tag => t.genres.clone(),
            Field::Format => vec![extension(&t.path)],
            Field::Folder => vec![
                Path::new(&t.path)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ],
            Field::FileName => vec![t.file_name.clone()],
            _ => Vec::new(),
        }
    }

    /// 数值字段的取值，单位同 [`Field::unit`]（没有这项数据时为 `None`）。
    pub(crate) fn number(self, t: &TrackEntry) -> Option<f64> {
        match self {
            Field::Year => t.year.map(f64::from),
            Field::Duration => Some(t.duration),
            Field::SampleRate => t.sample_rate.map(|r| r as f64 / 1000.0),
            Field::BitRate => t.bit_rate.map(f64::from),
            Field::Channels => t.channels.map(f64::from),
            Field::Size => Some(t.size_bytes as f64 / 1_000_000.0),
            _ => None,
        }
    }
}

/// 比较方式。
/// 比较方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Contains,
    NotContains,
    Is,
    IsNot,
    StartsWith,
    EndsWith,
    Greater,
    Less,
    /// 最近 N 天内。
    InLast,
    /// N 天之前。
    NotInLast,
}

impl Op {
    pub fn label(self) -> &'static str {
        match self {
            Op::Contains => "包含",
            Op::NotContains => "不包含",
            Op::Is => "是",
            Op::IsNot => "不是",
            Op::StartsWith => "开头是",
            Op::EndsWith => "结尾是",
            Op::Greater => "大于",
            Op::Less => "小于",
            Op::InLast => "在最近",
            Op::NotInLast => "早于最近",
        }
    }
}

/// 智能歌单的排序。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    /// 曲库顺序（歌手、专辑、碟号、音轨号）。
    #[default]
    Library,
    Title,
    Artist,
    Album,
    /// 年份，新的在前。
    YearDesc,
    /// 最近修改的在前。
    ModifiedDesc,
    /// 最长的在前。
    DurationDesc,
    /// 比特率高的在前。
    BitRateDesc,
}

impl Sort {
    pub const ALL: [Sort; 8] = [
        Sort::Library,
        Sort::Title,
        Sort::Artist,
        Sort::Album,
        Sort::YearDesc,
        Sort::ModifiedDesc,
        Sort::DurationDesc,
        Sort::BitRateDesc,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Library => "曲库顺序",
            Sort::Title => "标题",
            Sort::Artist => "歌手",
            Sort::Album => "专辑",
            Sort::YearDesc => "年份（新到旧）",
            Sort::ModifiedDesc => "最近修改",
            Sort::DurationDesc => "时长（长到短）",
            Sort::BitRateDesc => "比特率（高到低）",
        }
    }
}

fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// 解析数值条件的值：时长接受 `m:ss` / `h:mm:ss` 或秒数，其余为十进制数。
pub fn parse_number(field: Field, value: &str) -> Option<f64> {
    let value = value.trim();
    if field == Field::Duration && value.contains(':') {
        let mut total = 0.0;
        for part in value.split(':') {
            total = total * 60.0 + part.trim().parse::<f64>().ok()?;
        }
        return Some(total);
    }
    value.parse::<f64>().ok().filter(|v| v.is_finite())
}
