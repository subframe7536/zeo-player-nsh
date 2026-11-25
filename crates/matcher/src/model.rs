use serde::{Deserialize, Serialize};
use zeo_types::config::{MatchSource, MatchStrictness};

/// 单次搜索的上下文：搜索关键词 + 本地文件的参考标签与时长。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchQuery {
    /// 实际发送给平台的搜索关键词。
    pub keyword: String,
    /// 本地标题（用于打分）。
    pub title: String,
    /// 本地艺术家（用于打分）。
    pub artist: String,
    /// 本地专辑（用于打分）。
    pub album: String,
    /// 本地文件时长（毫秒，最强版本判别字段）。
    pub duration_ms: Option<u64>,
}

/// 平台返回的单条候选元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchCandidate {
    pub source: MatchSource,
    /// 平台曲目 ID（如 QQ mid、网易云 id、酷狗 hash、酷我 rid、MB recording id）。
    pub source_id: String,
    /// 辅助 ID（如 QQ 数字 songid，用于歌词或详情兜底）。
    pub ext_id: Option<String>,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_ms: Option<u64>,
    /// 列表预览用缩略图 URL（~300px）。
    pub cover_thumb_url: Option<String>,
    /// 写盘用高清封面 URL（800-1024px）。
    pub cover_original_url: Option<String>,
}

impl MatchCandidate {
    /// 按指定分隔符拼接歌手列表（默认 `; `，与 ZeoPlayer 艺术家分隔习惯一致）。
    pub fn joined_artists(&self, sep: &str) -> String {
        self.artists
            .iter()
            .map(|a| a.trim())
            .filter(|a| !a.is_empty())
            .collect::<Vec<_>>()
            .join(sep)
    }

    /// 优先取高清封面，缺省回落缩略图。
    pub fn best_cover_url(&self) -> Option<&str> {
        self.cover_original_url
            .as_deref()
            .filter(|u| !u.is_empty())
            .or_else(|| self.cover_thumb_url.as_deref().filter(|u| !u.is_empty()))
    }
}

/// 候选项命中的维度标记（用于界面徽章展示）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchedTags {
    pub title_exact: bool,
    pub artist_match: bool,
    pub duration_close: bool,
    pub best: bool,
}

/// 经评分与排序后的候选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedCandidate {
    pub candidate: MatchCandidate,
    pub score: i32,
    /// 候选时长 − 本地时长（毫秒）；任一缺失时为 `None`。
    pub duration_diff_ms: Option<i64>,
    /// 与本地时长差超过 20 秒，基本可判定为不同版本。
    pub duration_far: bool,
    pub tags: MatchedTags,
}

/// 批量匹配的单条输入曲目。
#[derive(Debug, Clone)]
pub struct BatchTrackInput {
    pub path: String,
    pub file_name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<u64>,
}

/// 批量匹配任务配置。
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// true：并发查询 `sources` 合并打分；false：按 `sources` 顺序瀑布回退。
    pub smart_aggregate: bool,
    /// 参与匹配的具体平台列表（按优先级排序）。
    pub sources: Vec<MatchSource>,
    pub strictness: MatchStrictness,
    /// 最大并发工作线程数（默认 2，防风控）。
    pub concurrency: usize,
    /// 每次请求后的最小礼貌间隔（毫秒，默认 180）。
    pub min_delay_ms: u64,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            smart_aggregate: false,
            sources: vec![
                MatchSource::QQMusic,
                MatchSource::NetEase,
                MatchSource::Kugou,
                MatchSource::Kuwo,
            ],
            strictness: MatchStrictness::Standard,
            concurrency: 2,
            min_delay_ms: 180,
        }
    }
}

/// 批量匹配单首曲目的判定状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchMatchStatus {
    /// 达到自动采纳阈值。
    Matched,
    /// 有候选但置信度不足，需人工确认。
    NeedsReview,
    /// 未搜索到候选。
    NotFound,
    /// 所有数据源均发生网络/解析错误。
    Failed,
}

/// 批量匹配单首曲目的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchItemResult {
    pub path: String,
    pub status: BatchMatchStatus,
    /// 按分数降序的前 5 条候选（供用户在预览阶段切换）。
    pub candidates: Vec<RankedCandidate>,
    /// 默认选中的候选索引（`Matched` 时为 `Some(0)`）。
    pub selected: Option<usize>,
    /// 若出错，记录最后一条报错摘要。
    pub error: Option<String>,
}

/// 批量匹配实时进度汇报。
#[derive(Debug, Clone)]
pub struct BatchProgress {
    pub done: usize,
    pub total: usize,
    pub matched: usize,
    pub needs_review: usize,
    pub not_found: usize,
    pub failed: usize,
    pub latest: BatchItemResult,
}
