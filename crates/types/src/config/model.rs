use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 曲目来自哪个提供方。新源实现时加一个变体，并各自产出带自己 id 的
/// `TrackEntry`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    /// 本地文件夹。
    #[default]
    Local,
}

/// 一条曲目各处共享的数据：`Arc` 一份，克隆只加引用计数——分组 /
/// 搜索 / 歌单 / 队列持有的都是同一份。`meta` 与元数据缓存共享同一个
/// `Arc<MetaInfo>`，标签字符串不再复制两遍；标签字段（`title`、
/// `artist`、`duration`、`has_art`…）经 `Deref` 直读。
#[derive(Debug, Clone, PartialEq)]
pub struct TrackShared {
    /// 来源提供方。
    pub provider: ProviderId,
    pub path: String,
    pub file_name: String,
    /// 专辑歌手（标签缺省回退曲目歌手；拆组与排序用）。
    pub album_artist: String,
    /// 按流派分隔符拆好的流派列表（`split_genres`，设置改了会重算）。
    pub genres: Vec<String>,
    pub size_bytes: u64,
    pub modified: u64,
    /// 文件指纹（[`crate::meta::file_stamp`]），内容一变就变。
    pub stamp: u64,
    /// 完整标签，与元数据缓存共享。
    pub meta: Arc<crate::meta::MetaInfo>,
}

impl TrackShared {
    /// 显示用标题：若标签无标题，回落文件名主干。
    pub fn display_title(&self) -> &str {
        if !self.meta.title.is_empty() {
            &self.meta.title
        } else {
            Path::new(&self.file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&self.file_name)
        }
    }

    /// 显示用歌手：若标签无歌手，回落 "Unknown Artist"。
    pub fn display_artist(&self) -> &str {
        if !self.meta.artist.is_empty() {
            &self.meta.artist
        } else {
            "Unknown Artist"
        }
    }

    /// 显示用专辑：若标签无专辑，回落 "Unknown Album"。
    pub fn display_album(&self) -> &str {
        if !self.meta.album.is_empty() {
            &self.meta.album
        } else {
            "Unknown Album"
        }
    }
}

impl std::ops::Deref for TrackShared {
    type Target = crate::meta::MetaInfo;

    fn deref(&self) -> &Self::Target {
        &self.meta
    }
}

/// 库索引里的一条曲目（TSX TrackInfo 的 Rust 侧形态）。
#[derive(Debug, Clone, PartialEq)]
pub struct TrackEntry(pub Arc<TrackShared>);

impl std::ops::Deref for TrackEntry {
    type Target = TrackShared;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// 点窗口的关闭按钮时做什么。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CloseAction {
    /// 第一次关闭时询问（之后记住选择）。
    #[default]
    Ask,
    /// 隐藏到托盘，继续播放。
    Tray,
    /// 退出。
    Quit,
}

/// 全屏歌词的对齐方式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LyricsAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// 界面外观（浅色 / 深色）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    /// 跟随系统。
    #[default]
    System,
    /// 固定浅色。
    Light,
    /// 固定深色。
    Dark,
}

/// 侧栏下半部当前显示的视图（歌单 / 标签 / 文件夹）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarSection {
    /// 歌单列表。
    #[default]
    Playlists,
    /// 标签（流派）列表。
    Tags,
    /// 文件夹树。
    Folders,
}

/// 出厂流派分隔符。
pub const DEFAULT_SEPARATORS: &[&str] = &[";"];

/// 出厂艺术家分隔符。
pub const DEFAULT_ARTIST_SEPARATORS: &[&str] = &[";"];

/// 出厂过滤：短于 10 秒的音频不进曲库。
pub const DEFAULT_MIN_DURATION_SECS: u32 = 10;
/// 出厂过滤：小于 100 KB 的音频不进曲库。
pub const DEFAULT_MIN_SIZE_KB: u32 = 100;

/// 主题色（强调色）：默认跟随当前曲目封面的主色调（没有封面或封面
/// 不饱和时回黑白），也可钉住黑白或自定义强调色（0xRRGGBB）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeColor {
    /// 黑白（主题默认）。
    Neutral,
    /// 当前曲目封面的主色调。
    Cover,
    /// 自定义颜色（24-bit RGB，如 0x16a34a）。
    Custom(u32),
}

impl Default for ThemeColor {
    fn default() -> Self {
        Self::Cover
    }
}

impl Serialize for ThemeColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Neutral => serializer.serialize_str("neutral"),
            Self::Cover => serializer.serialize_str("cover"),
            Self::Custom(rgb) => match *rgb {
                0xe5263f => serializer.serialize_str("red"),
                0xea580c => serializer.serialize_str("orange"),
                0x16a34a => serializer.serialize_str("green"),
                0x2563eb => serializer.serialize_str("blue"),
                0x9333ea => serializer.serialize_str("purple"),
                other => serializer.serialize_str(&format!("#{:06x}", other)),
            },
        }
    }
}

impl<'de> Deserialize<'de> for ThemeColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ThemeColorVisitor;

        impl<'de> serde::de::Visitor<'de> for ThemeColorVisitor {
            type Value = ThemeColor;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a theme color string or custom hex")
            }

            fn visit_str<E>(self, v: &str) -> Result<ThemeColor, E>
            where
                E: serde::de::Error,
            {
                let s = v.trim();
                let lower = s.to_lowercase();
                match lower.as_str() {
                    "neutral" => Ok(ThemeColor::Neutral),
                    "cover" => Ok(ThemeColor::Cover),
                    "red" => Ok(ThemeColor::Custom(0xe5263f)),
                    "orange" => Ok(ThemeColor::Custom(0xea580c)),
                    "green" => Ok(ThemeColor::Custom(0x16a34a)),
                    "blue" => Ok(ThemeColor::Custom(0x2563eb)),
                    "purple" => Ok(ThemeColor::Custom(0x9333ea)),
                    hex => {
                        let hex = hex.trim_start_matches('#');
                        if hex.len() == 6 {
                            if let Ok(rgb) = u32::from_str_radix(hex, 16) {
                                return Ok(ThemeColor::Custom(rgb));
                            }
                        } else if hex.len() == 3 {
                            let mut expanded = String::new();
                            for c in hex.chars() {
                                expanded.push(c);
                                expanded.push(c);
                            }
                            if let Ok(rgb) = u32::from_str_radix(&expanded, 16) {
                                return Ok(ThemeColor::Custom(rgb));
                            }
                        }
                        Ok(ThemeColor::Cover)
                    }
                }
            }

            fn visit_u64<E>(self, v: u64) -> Result<ThemeColor, E>
            where
                E: serde::de::Error,
            {
                Ok(ThemeColor::Custom(v as u32))
            }

            fn visit_i64<E>(self, v: i64) -> Result<ThemeColor, E>
            where
                E: serde::de::Error,
            {
                Ok(ThemeColor::Custom(v as u32))
            }
        }

        deserializer.deserialize_any(ThemeColorVisitor)
    }
}
