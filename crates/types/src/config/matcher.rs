use serde::{Deserialize, Serialize};

/// 在线元数据匹配的数据源。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchSource {
    /// 智能聚合：并发查询多个已启用平台并合并打分。
    #[default]
    Smart,
    QQMusic,
    NetEase,
    Kugou,
    Kuwo,
    MusicBrainz,
}

impl MatchSource {
    /// 全部可选平台（含智能聚合，用于单曲下拉选择）。
    pub const ALL: [MatchSource; 6] = [
        MatchSource::Smart,
        MatchSource::QQMusic,
        MatchSource::NetEase,
        MatchSource::Kugou,
        MatchSource::Kuwo,
        MatchSource::MusicBrainz,
    ];

    /// 单个具体平台（不含聚合，用于数据源优先级列表）。
    pub const PROVIDERS: [MatchSource; 5] = [
        MatchSource::QQMusic,
        MatchSource::NetEase,
        MatchSource::Kugou,
        MatchSource::Kuwo,
        MatchSource::MusicBrainz,
    ];

    /// 界面显示的平台名称（国内平台使用缩写）。
    pub fn label(self) -> &'static str {
        match self {
            MatchSource::Smart => "智能聚合",
            MatchSource::QQMusic => "QM",
            MatchSource::NetEase => "NCM",
            MatchSource::Kugou => "KGM",
            MatchSource::Kuwo => "KWM",
            MatchSource::MusicBrainz => "MusicBrainz",
        }
    }

    /// 候选行里紧凑显示的短标签。
    pub fn short_label(self) -> &'static str {
        match self {
            MatchSource::Smart => "聚合",
            MatchSource::QQMusic => "QM",
            MatchSource::NetEase => "NCM",
            MatchSource::Kugou => "KGM",
            MatchSource::Kuwo => "KWM",
            MatchSource::MusicBrainz => "MB",
        }
    }
}

/// 批量自动匹配的严格度。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStrictness {
    /// 标准模式：综合校验标题、歌手与时长，避免误匹配。
    #[default]
    Standard,
    /// 宽松模式：允许标题精确命中且时长未严重偏离时采纳。
    Relaxed,
}

impl MatchStrictness {
    pub fn label(self) -> &'static str {
        match self {
            MatchStrictness::Standard => "标准（推荐）",
            MatchStrictness::Relaxed => "宽松",
        }
    }
}

/// 批量写回时的字段覆盖策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WritePolicy {
    /// 仅补全当前为空的字段（安全默认）。
    #[default]
    FillEmpty,
    /// 用匹配结果覆盖已有字段。
    Overwrite,
}

impl WritePolicy {
    pub fn label(self) -> &'static str {
        match self {
            WritePolicy::FillEmpty => "仅补全空字段",
            WritePolicy::Overwrite => "覆盖已有字段",
        }
    }
}

/// 批量自动匹配需要写入的字段集合。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MatchFields {
    pub title: bool,
    pub artist: bool,
    pub album: bool,
    pub album_artist: bool,
    pub year: bool,
    pub track: bool,
    pub disc: bool,
    pub cover: bool,
    pub lyrics: bool,
    /// 封面写到同名侧车图片（true）还是内嵌标签（false）。
    pub cover_sidecar: bool,
    /// 歌词写到同名 `.lrc` 侧车文件（true）还是内嵌标签（false）。
    pub lyrics_sidecar: bool,
}

impl Default for MatchFields {
    fn default() -> Self {
        Self {
            title: true,
            artist: true,
            album: true,
            album_artist: true,
            year: true,
            track: true,
            disc: true,
            cover: true,
            lyrics: true,
            cover_sidecar: false,
            lyrics_sidecar: false,
        }
    }
}

/// 持久化的元数据自动匹配偏好（向后兼容写入 `library.json`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MatchPrefs {
    /// 单曲弹窗默认选中的平台。
    pub single_source: MatchSource,
    /// 批量匹配是否使用多源并发聚合（false = 按 `batch_sources` 顺序瀑布回退）。
    pub batch_smart: bool,
    /// 批量匹配启用的数据源及优先级顺序。
    pub batch_sources: Vec<MatchSource>,
    /// 批量匹配的严格度。
    pub strictness: MatchStrictness,
    /// 批量写回策略（仅补全空字段 / 覆盖已有）。
    pub write_policy: WritePolicy,
    /// 批量写回勾选的字段。
    pub fields: MatchFields,
}

impl Default for MatchPrefs {
    fn default() -> Self {
        Self {
            single_source: MatchSource::Smart,
            batch_smart: false,
            batch_sources: vec![
                MatchSource::QQMusic,
                MatchSource::NetEase,
                MatchSource::Kugou,
                MatchSource::Kuwo,
            ],
            strictness: MatchStrictness::Standard,
            write_policy: WritePolicy::FillEmpty,
            fields: MatchFields::default(),
        }
    }
}
