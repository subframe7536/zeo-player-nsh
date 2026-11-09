use serde::{Deserialize, Serialize};

use super::model::{
    Appearance, CloseAction, DEFAULT_ARTIST_SEPARATORS, DEFAULT_MIN_DURATION_SECS,
    DEFAULT_MIN_SIZE_KB, DEFAULT_SEPARATORS, LyricsAlign, SidebarSection, ThemeColor,
};

/// 持久化配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryConfig {
    pub roots: Vec<String>,
    /// 旧版的单个分隔符（只读：迁移到 `genre_separators`）。
    #[serde(default, skip_serializing)]
    pub genre_separator: Option<String>,
    /// 流派分隔符（任一出现即切开；空列表为不拆分）。
    #[serde(default = "default_separators")]
    pub genre_separators: Vec<String>,
    /// 艺术家分隔符（任一出现即切开；空列表为不拆分）。
    #[serde(default = "default_artist_separators")]
    pub artist_separators: Vec<String>,
    /// 监视根目录，文件变化后自动扫描。
    #[serde(default)]
    pub watch: bool,
    /// 界面字体家族（`None`：系统字体）。
    #[serde(default)]
    pub font: Option<String>,
    /// 关闭窗口时的行为。
    #[serde(default)]
    pub close_action: CloseAction,
    /// 播放 / 暂停 / 跳转 / 切歌的淡入淡出时长（毫秒；0 关闭）。
    #[serde(default = "default_fade_ms")]
    pub fade_ms: u32,
    /// 队列循环模式（顺序 / 单曲 / 随机）。
    #[serde(default)]
    pub loop_mode: crate::audio::LoopMode,
    /// 全屏歌词的对齐方式。
    #[serde(default)]
    pub lyrics_align: LyricsAlign,
    /// 界面外观（跟随系统 / 浅色 / 深色）。
    #[serde(default)]
    pub appearance: Appearance,
    /// 主题色（强调色）。
    #[serde(default)]
    pub theme_color: ThemeColor,
    /// 侧栏下半部当前显示的视图（歌单 / 标签 / 文件夹）。
    #[serde(default)]
    pub sidebar_section: SidebarSection,
    /// 过滤：短于此时长（秒）的音频不进曲库（0 不过滤）。
    #[serde(default = "default_min_duration_secs")]
    pub min_duration_secs: u32,
    /// 过滤：小于此大小（KB）的音频不进曲库（0 不过滤）。
    #[serde(default = "default_min_size_kb")]
    pub min_size_kb: u32,
    /// 本地同名 `.lrc` 优先于内嵌歌词。
    #[serde(default = "default_true")]
    pub prefer_local_lyrics: bool,
    /// 全局歌词偏移（毫秒，正值 = 歌词提前显示；与每首曲目的手动偏移相加生效）。
    #[serde(default)]
    pub global_lyrics_offset_ms: i64,
    /// 全屏歌词行悬停时显示该行时间戳（占位在行尾，长歌词盖不住）。
    #[serde(default = "default_true")]
    pub lyrics_timestamps: bool,
    /// 本地同名图片（jpg / jpeg / png / webp）优先于内嵌封面。
    #[serde(default = "default_true")]
    pub prefer_local_covers: bool,
    /// 列表里显示封面；关掉后行 / 网格 / 队列都只画占位图（省解码内存）。
    #[serde(default = "default_true")]
    pub show_covers: bool,
    /// 「删除文件」不经过回收站、直接删除（不可恢复）。
    #[serde(default)]
    pub delete_permanently: bool,
    /// 输出设备变化（拔耳机、切默认设备）时暂停播放。
    #[serde(default = "default_true")]
    pub pause_on_device_change: bool,
    /// 播放统计：听过整首的该百分比算一次播放（0 = 不用此条件）。
    #[serde(default = "default_stats_min_percent")]
    pub stats_min_percent: u32,
    /// 播放统计：实际播出该秒数算一次播放（0 = 不用此条件）。
    #[serde(default = "default_stats_min_secs")]
    pub stats_min_secs: u32,
    /// ReplayGain 音量标准化（模式 / 预放大 / 无标签增益 / 防削波）。
    #[serde(default)]
    pub replaygain: crate::audio::ReplayGainConfig,
    /// 均衡器（开关 / 前级 / 10 段增益）。
    #[serde(default)]
    pub eq: crate::audio::EqConfig,
    /// 重新打开时恢复上次的窗口大小与位置。
    #[serde(default = "default_true")]
    pub remember_frame: bool,
    /// 上次窗口的常规位置与大小（NSH 的 `WindowPlacement`：模式 + 常规
    /// 边界，点；恢复时经 `WindowRef::set_placement` 按现有显示器校正）。
    /// 旧版的 `window_frame` 字段读取时忽略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_placement: Option<nsh_core::desktop::WindowPlacement>,
}

fn default_fade_ms() -> u32 {
    crate::audio::DEFAULT_FADE_MS
}

fn default_min_duration_secs() -> u32 {
    DEFAULT_MIN_DURATION_SECS
}

fn default_min_size_kb() -> u32 {
    DEFAULT_MIN_SIZE_KB
}

fn default_stats_min_percent() -> u32 {
    crate::audio::DEFAULT_STATS_MIN_PERCENT
}

fn default_stats_min_secs() -> u32 {
    crate::audio::DEFAULT_STATS_MIN_SECS
}

fn default_true() -> bool {
    true
}

impl Default for LibraryConfig {
    fn default() -> Self {
        LibraryConfig {
            roots: Vec::new(),
            genre_separator: None,
            genre_separators: default_separators(),
            artist_separators: default_artist_separators(),
            watch: false,
            font: None,
            close_action: CloseAction::Ask,
            fade_ms: default_fade_ms(),
            loop_mode: crate::audio::LoopMode::default(),
            lyrics_align: LyricsAlign::default(),
            appearance: Appearance::default(),
            theme_color: ThemeColor::default(),
            sidebar_section: SidebarSection::default(),
            min_duration_secs: default_min_duration_secs(),
            min_size_kb: default_min_size_kb(),
            prefer_local_lyrics: default_true(),
            global_lyrics_offset_ms: 0,
            lyrics_timestamps: true,
            prefer_local_covers: default_true(),
            show_covers: default_true(),
            delete_permanently: false,
            pause_on_device_change: true,
            stats_min_percent: default_stats_min_percent(),
            stats_min_secs: default_stats_min_secs(),
            replaygain: crate::audio::ReplayGainConfig::default(),
            eq: crate::audio::EqConfig::default(),
            remember_frame: true,
            window_placement: None,
        }
    }
}

fn default_separators() -> Vec<String> {
    DEFAULT_SEPARATORS.iter().map(|s| (*s).to_owned()).collect()
}

fn default_artist_separators() -> Vec<String> {
    DEFAULT_ARTIST_SEPARATORS
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

impl LibraryConfig {
    /// 迁移旧字段、去空去重。
    pub fn normalized(mut self) -> Self {
        // 旧版文件只有单个 `genre_separator`。
        if let Some(old) = self.genre_separator.take()
            && !old.is_empty()
        {
            self.genre_separators = vec![old];
        }
        self.genre_separators = clean_separators(&self.genre_separators);
        self.artist_separators = clean_separators(&self.artist_separators);
        self
    }
}

/// 去空、去重（保持顺序）。分隔符可以为空列表：不拆分。
pub fn clean_separators(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in list {
        if !s.is_empty() && !out.contains(s) {
            out.push(s.clone());
        }
    }
    out
}
