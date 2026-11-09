//! 配置与全局库：所有提供方共享的模型（[`TrackEntry`] 索引行、
//! [`ScanProgress`] 同步进度）、全局库 [`LibraryStore`]（各源的曲目合成
//! 一份索引）与持久化设置 [`LibraryConfig`]（`library.json`），外加流派
//! 拆分等工具。
//!
//! 曲目来源（本地文件、jellyfin、YouTube Music…）的实现在
//! `crate::provider` 下；本模块的 `scan`/`rebuild` 经 `crate::provider`
//! 的门面把各源的曲目同步进索引。

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::metadata::MetaCache;

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
    /// 文件指纹（[`crate::metadata::file_stamp`]），内容一变就变。
    pub stamp: u64,
    /// 完整标签，与元数据缓存共享。
    pub meta: Arc<crate::metadata::MetaInfo>,
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
    type Target = crate::metadata::MetaInfo;

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

/// 持久化配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryConfig {
    pub roots: Vec<String>,
    /// 旧版的单个分隔符（只读：迁移到 `genre_separators`）。
    #[serde(default, skip_serializing)]
    genre_separator: Option<String>,
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
    pub loop_mode: crate::engine::audio::LoopMode,
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
    /// macOS 菜单栏在托盘图标旁显示当前曲名（只在该平台生效）。
    #[serde(default = "default_true")]
    pub show_tray_title: bool,
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
    pub replaygain: crate::engine::audio::ReplayGainConfig,
    /// 均衡器（开关 / 前级 / 10 段增益）。
    #[serde(default)]
    pub eq: crate::engine::eq::EqConfig,
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
    crate::engine::audio::DEFAULT_FADE_MS
}

fn default_min_duration_secs() -> u32 {
    DEFAULT_MIN_DURATION_SECS
}

fn default_min_size_kb() -> u32 {
    DEFAULT_MIN_SIZE_KB
}

fn default_stats_min_percent() -> u32 {
    crate::engine::audio::DEFAULT_STATS_MIN_PERCENT
}

fn default_stats_min_secs() -> u32 {
    crate::engine::audio::DEFAULT_STATS_MIN_SECS
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
            loop_mode: crate::engine::audio::LoopMode::default(),
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
            show_tray_title: default_true(),
            delete_permanently: false,
            pause_on_device_change: true,
            stats_min_percent: default_stats_min_percent(),
            stats_min_secs: default_stats_min_secs(),
            replaygain: crate::engine::audio::ReplayGainConfig::default(),
            eq: crate::engine::eq::EqConfig::default(),
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
    fn normalized(mut self) -> Self {
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

/// 现在（Unix 秒）。
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// 本地时区相对 UTC 的偏移（秒）。取现值——夏令时切换边界的历史时刻
/// 用现值近似即可（首页统计的粒度是天）。
pub fn local_offset_seconds() -> i64 {
    #[cfg(target_os = "macos")]
    {
        // NSTimeZone.localTimeZone.secondsFromGMT 是现成的偏移秒数。
        objc2_foundation::NSTimeZone::localTimeZone().secondsFromGMT() as i64
    }
    #[cfg(target_os = "windows")]
    {
        // GetTimeZoneInformation 的 Bias 是「本地 + N 分钟 = UTC」——
        // 偏移取负；夏令时时段叠 DaylightBias。返回值是裸 u32
        // （windows-sys 0.61 只导出 INVALID 常量）：1=STANDARD 2=DAYLIGHT。
        use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        unsafe {
            let mut info: TIME_ZONE_INFORMATION = std::mem::zeroed();
            let id = GetTimeZoneInformation(&mut info);
            let extra = match id {
                2 => info.DaylightBias,
                1 => info.StandardBias,
                _ => 0,
            };
            -((info.Bias + extra) as i64) * 60
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        0
    }
}

/// `unix` 秒的本地天数（本地历法从 `unix + offset` 切）。
pub fn local_day(unix: u64, offset: i64) -> i64 {
    (unix as i64 + offset).div_euclid(86_400)
}

/// `unix` 秒的本地小时（0–23）。
pub fn local_hour(unix: u64, offset: i64) -> u32 {
    ((unix as i64 + offset).rem_euclid(86_400) / 3600) as u32
}

/// `unix` 所在本地周（周一 00:00）的起始 Unix 秒。
pub fn local_week_start(unix: u64, offset: i64) -> u64 {
    let day = local_day(unix, offset);
    // 纪元日 1970-01-01 是周四：day+3 对 7 取模 = 周一为 0 的星期序。
    let monday = day - (day + 3).rem_euclid(7);
    (monday * 86_400 - offset).max(0) as u64
}

/// `unix` 秒 → 本地 "YYYY-MM-DD HH:MM"（元数据面板的修改 / 入库时间用）。
pub fn format_local_datetime(unix: u64) -> String {
    format_local_at(unix, local_offset_seconds())
}

/// `unix` 秒 + 偏移 → "YYYY-MM-DD HH:MM"（测试可注入偏移）。
fn format_local_at(unix: u64, offset: i64) -> String {
    let t = unix as i64 + offset;
    let (y, m, d) = civil_from_days(t.div_euclid(86_400));
    let secs = t.rem_euclid(86_400);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        y,
        m,
        d,
        secs / 3600,
        secs % 3600 / 60
    )
}

/// 纪元日起算的天数 → (年, 月, 日)（Hinnant 的 civil_from_days，公历）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
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

/// 扫描进度。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanProgress {
    pub scanning: bool,
    /// 已处理 / 总数（遍历目录阶段总数为 0）。
    pub done: usize,
    pub total: usize,
    /// 正在读取的文件名。
    pub current: String,
}

/// 全局库存储：配置 + 索引。锁粒度粗但只在扫描/读取列表时短暂持有。
pub struct LibraryStore {
    dir: PathBuf,
    /// `Arc` 共享快照：界面轮询按指针判变化——所有写路径都必须经
    /// `update_config` 整份换新 `Arc`（原地改会让读侧漏掉变化）。
    config: std::sync::Mutex<Arc<LibraryConfig>>,
    // 索引与扫描进度由提供方（`crate::provider`）写；按惯例只经方法访问。
    pub(crate) tracks: std::sync::RwLock<Arc<Vec<TrackEntry>>>,
    pub(crate) dirs: std::sync::RwLock<Arc<Vec<String>>>,
    pub(crate) scanning: std::sync::atomic::AtomicBool,
    pub(crate) done: std::sync::atomic::AtomicUsize,
    pub(crate) total: std::sync::atomic::AtomicUsize,
    pub(crate) current: std::sync::Mutex<String>,
}

/// 应用数据目录（`ZEO_PLAYER_DATA_DIR` 可覆盖；开发构建写到仓库的
/// `.nsh/dev-data`，不碰系统目录；正式包才用系统数据目录）。
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ZEO_PLAYER_DATA_DIR") {
        return PathBuf::from(dir);
    }
    if cfg!(debug_assertions) {
        return PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".nsh")
            .join("dev-data");
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ZeoPlayer")
}

pub fn library_store() -> &'static LibraryStore {
    static STORE: OnceLock<LibraryStore> = OnceLock::new();
    STORE.get_or_init(|| {
        let dir = data_dir();
        // 单元测试不碰真实数据目录（自己设了 ZEO_PLAYER_DATA_DIR 的测试除外，
        // 同 db() 的护栏）：谁先调谁就绑，把全局绑到 dev-data 会让别人的
        // 断言吃到真实配置。
        #[cfg(test)]
        let dir = if std::env::var_os("ZEO_PLAYER_DATA_DIR").is_none() {
            std::env::temp_dir().join(format!("zeo-player-store-{}", std::process::id()))
        } else {
            dir
        };
        let _ = std::fs::create_dir_all(&dir);
        let config = load_config(&dir);
        // 先用数据库里上次的元数据建索引，界面马上有曲库；随后的同步只重读变了的内容。
        let tracks = crate::provider::initial_index(&config);
        let dirs = crate::provider::initial_dirs(&config.roots);
        LibraryStore {
            dir,
            config: std::sync::Mutex::new(Arc::new(config)),
            tracks: std::sync::RwLock::new(Arc::new(tracks)),
            dirs: std::sync::RwLock::new(Arc::new(dirs)),
            scanning: std::sync::atomic::AtomicBool::new(false),
            done: std::sync::atomic::AtomicUsize::new(0),
            total: std::sync::atomic::AtomicUsize::new(0),
            current: std::sync::Mutex::new(String::new()),
        }
    })
}

fn config_path(dir: &Path) -> PathBuf {
    dir.join("library.json")
}

fn load_config(dir: &Path) -> LibraryConfig {
    std::fs::read(config_path(dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LibraryConfig>(&bytes).ok())
        .unwrap_or_default()
        .normalized()
}

impl LibraryStore {
    /// 配置的共享快照（`Arc` 克隆只加引用计数；设置写入才换新）。
    pub fn config(&self) -> Arc<LibraryConfig> {
        self.config.lock().unwrap().clone()
    }

    pub fn roots(&self) -> Vec<String> {
        self.config().roots.clone()
    }

    /// 克隆当前配置、用 `f` 改、写盘并换新 `Arc`（返回新快照）。
    fn update_config(&self, f: impl FnOnce(&mut LibraryConfig)) -> Arc<LibraryConfig> {
        let mut guard = self.config.lock().unwrap();
        let mut config = (**guard).clone();
        f(&mut config);
        self.save_config(&config);
        let config = Arc::new(config);
        *guard = config.clone();
        config
    }

    fn save_config(&self, config: &LibraryConfig) {
        if let Ok(bytes) = serde_json::to_vec_pretty(config) {
            let path = config_path(&self.dir);
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, &bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    /// 新增音乐根目录（必须存在且为目录）；返回新的根列表。
    pub fn add_root(&self, path: &str) -> Result<Vec<String>, String> {
        // dunce：Windows 上不返回 `\\?\C:\…` 这种逐字路径，和扫描出的曲目路径一致。
        let canonical = dunce::canonicalize(path).map_err(|e| format!("无法访问 {path}: {e}"))?;
        if !canonical.is_dir() {
            return Err(format!("{path} 不是目录"));
        }
        let as_string = canonical.to_string_lossy().into_owned();
        let config = self.update_config(|c| {
            if !c.roots.iter().any(|r| r == &as_string) {
                c.roots.push(as_string.clone());
            }
        });
        crate::provider::local::watcher::sync();
        Ok(config.roots.clone())
    }

    /// 移除音乐根目录。
    pub fn remove_root(&self, path: &str) -> Vec<String> {
        let config = self.update_config(|c| c.roots.retain(|r| r != path));
        crate::provider::local::watcher::sync();
        config.roots.clone()
    }

    /// 设置界面字体（`None`：系统字体），保存。
    pub fn set_font(&self, font: Option<String>) {
        self.update_config(|c| c.font = font.filter(|f| !f.trim().is_empty()));
    }

    pub fn fade_ms(&self) -> u32 {
        self.config().fade_ms
    }

    pub fn set_fade_ms(&self, ms: u32) {
        self.update_config(|c| c.fade_ms = ms);
    }

    pub fn set_lyrics_align(&self, align: LyricsAlign) {
        self.update_config(|c| c.lyrics_align = align);
    }

    pub fn appearance(&self) -> Appearance {
        self.config().appearance
    }

    /// 设置界面外观（保存）。
    pub fn set_appearance(&self, appearance: Appearance) {
        self.update_config(|c| c.appearance = appearance);
    }

    #[allow(dead_code)]
    pub fn sidebar_section(&self) -> SidebarSection {
        self.config().sidebar_section
    }

    /// 设置侧栏下半部显示的视图（保存）。
    pub fn set_sidebar_section(&self, section: SidebarSection) {
        self.update_config(|c| c.sidebar_section = section);
    }

    pub fn loop_mode(&self) -> crate::engine::audio::LoopMode {
        self.config().loop_mode
    }

    pub fn set_loop_mode(&self, mode: crate::engine::audio::LoopMode) {
        self.update_config(|c| c.loop_mode = mode);
    }

    /// 本地同名歌词文件是否优先于内嵌歌词。
    pub fn prefer_local_lyrics(&self) -> bool {
        self.config().prefer_local_lyrics
    }

    pub fn set_prefer_local_lyrics(&self, on: bool) {
        self.update_config(|c| c.prefer_local_lyrics = on);
    }

    /// 设置全局歌词偏移（保存）。
    pub fn set_global_lyrics_offset_ms(&self, ms: i64) {
        self.update_config(|c| c.global_lyrics_offset_ms = ms);
    }

    /// 全屏歌词行时间戳（悬停显示）开关。
    pub fn set_lyrics_timestamps(&self, on: bool) {
        self.update_config(|c| c.lyrics_timestamps = on);
    }

    /// 本地同名图片文件是否优先于内嵌封面。
    pub fn prefer_local_covers(&self) -> bool {
        self.config().prefer_local_covers
    }

    pub fn set_prefer_local_covers(&self, on: bool) {
        self.update_config(|c| c.prefer_local_covers = on);
    }

    /// 设置列表是否显示封面（关闭后行 / 网格 / 队列只画占位图）。
    pub fn set_show_covers(&self, on: bool) {
        self.update_config(|c| c.show_covers = on);
    }

    /// macOS 菜单栏是否在托盘图标旁显示当前曲名。
    pub fn set_show_tray_title(&self, on: bool) {
        self.update_config(|c| c.show_tray_title = on);
    }

    /// 「删除文件」是否不经过回收站、直接删除。
    pub fn delete_permanently(&self) -> bool {
        self.config().delete_permanently
    }

    pub fn set_delete_permanently(&self, on: bool) {
        self.update_config(|c| c.delete_permanently = on);
    }

    /// 输出设备变化时是否暂停播放。
    pub fn pause_on_device_change(&self) -> bool {
        self.config().pause_on_device_change
    }

    pub fn set_pause_on_device_change(&self, on: bool) {
        self.update_config(|c| c.pause_on_device_change = on);
    }

    /// 播放统计阈值（听过多少算一次播放；0 = 不用该条件）。
    pub fn stats_min_percent(&self) -> u32 {
        self.config().stats_min_percent
    }

    pub fn stats_min_secs(&self) -> u32 {
        self.config().stats_min_secs
    }

    /// 两个阈值一起写（保存）。
    pub fn set_stats_thresholds(&self, percent: u32, secs: u32) {
        self.update_config(|c| {
            c.stats_min_percent = percent.min(100);
            c.stats_min_secs = secs.min(3600);
        });
    }

    /// ReplayGain 设置。
    pub fn replaygain(&self) -> crate::engine::audio::ReplayGainConfig {
        self.config().replaygain
    }

    pub fn set_replaygain(&self, cfg: crate::engine::audio::ReplayGainConfig) {
        self.update_config(|c| c.replaygain = cfg);
    }

    /// 均衡器设置。
    pub fn eq(&self) -> crate::engine::eq::EqConfig {
        self.config().eq
    }

    /// 只更新内存里的配置快照、不落盘：`library.json` 的写盘由调用方
    /// （`ui::store::set_eq`，400ms 防抖）调 [`LibraryStore::save`] 收尾。
    /// 内存立即更新是为了不让 120ms 轮询在防抖窗口内把界面值弹回旧档。
    pub fn set_eq(&self, cfg: crate::engine::eq::EqConfig) {
        let mut guard = self.config.lock().unwrap();
        let mut config = (**guard).clone();
        config.eq = cfg;
        *guard = Arc::new(config);
    }

    /// 把当前内存配置写回 `library.json`（防抖写盘的收尾用）。
    pub fn save(&self) {
        self.save_config(&self.config());
    }

    pub fn close_action(&self) -> CloseAction {
        self.config().close_action
    }

    pub fn set_close_action(&self, action: CloseAction) {
        self.update_config(|c| c.close_action = action);
        crate::window::set_close_action(action);
    }

    /// 重新打开时是否恢复上次的窗口大小与位置。
    pub fn remember_frame(&self) -> bool {
        self.config().remember_frame
    }

    pub fn set_remember_frame(&self, on: bool) {
        self.update_config(|c| {
            c.remember_frame = on;
            if !on {
                c.window_placement = None;
            }
        });
        crate::window::set_remember_frame(on);
    }

    /// 上次的窗口位置（`remember_frame` 开着时才有意义）。
    pub fn window_placement(&self) -> Option<nsh_core::desktop::WindowPlacement> {
        self.config().window_placement
    }

    /// 记录当前窗口位置（界面侧的窗口状态效果在上报变化时调用）。
    pub fn set_window_placement(&self, placement: nsh_core::desktop::WindowPlacement) {
        let mut guard = self.config.lock().unwrap();
        if !guard.remember_frame {
            return;
        }
        let mut config = (**guard).clone();
        config.window_placement = Some(placement);
        self.save_config(&config);
        *guard = Arc::new(config);
    }

    pub fn watching(&self) -> bool {
        self.config().watch
    }

    /// 设置主题色（保存）。
    pub fn set_theme_color(&self, color: ThemeColor) {
        self.update_config(|c| c.theme_color = color);
    }

    /// 设置时长过滤阈值（保存；0 为不过滤）。
    pub fn set_min_duration_secs(&self, secs: u32) {
        self.update_config(|c| c.min_duration_secs = secs);
    }

    /// 设置大小过滤阈值（保存；0 为不过滤）。
    pub fn set_min_size_kb(&self, kb: u32) {
        self.update_config(|c| c.min_size_kb = kb);
    }

    /// 打开/关闭文件夹监视（保存设置）。
    pub fn set_watch(&self, on: bool) {
        self.update_config(|c| c.watch = on);
        crate::provider::local::watcher::sync();
    }

    pub fn tracks(&self) -> Arc<Vec<TrackEntry>> {
        self.tracks.read().unwrap().clone()
    }

    pub fn dirs(&self) -> Arc<Vec<String>> {
        self.dirs.read().unwrap().clone()
    }

    pub fn set_dirs(&self, dirs: Vec<String>) {
        *self.dirs.write().unwrap() = Arc::new(dirs);
    }

    /// 从索引里移除 `path`（界面「删除记录」）；元数据行与界面信号由
    /// 调用方清，文件不碰。文件仍在时下次扫描会重新入库。
    pub fn remove_track(&self, path: &str) {
        let current = self.tracks();
        if !current.iter().any(|t| t.path == path) {
            return;
        }
        let kept: Vec<TrackEntry> = current.iter().filter(|t| t.path != path).cloned().collect();
        *self.tracks.write().unwrap() = Arc::new(kept);
    }

    /// 移除 `path` 下（含自身）的全部索引与元数据记录（文件夹已删，
    /// 或根目录被摘掉）：索引、标签缓存、数据库、待写队列一起清；歌
    /// 单条目与根目录配置由调用方按语义处理。
    pub fn remove_under(&self, path: &str) {
        let sep = std::path::MAIN_SEPARATOR;
        let trimmed = path.trim_end_matches(sep);
        let under = format!("{trimmed}{sep}");
        let current = self.tracks();
        if current
            .iter()
            .any(|t| t.path == trimmed || t.path.starts_with(&under))
        {
            let kept: Vec<TrackEntry> = current
                .iter()
                .filter(|t| t.path != trimmed && !t.path.starts_with(&under))
                .cloned()
                .collect();
            *self.tracks.write().unwrap() = Arc::new(kept);
        }
        let current_dirs = self.dirs();
        if current_dirs
            .iter()
            .any(|d| d == trimmed || d.starts_with(&under))
        {
            let kept_dirs: Vec<String> = current_dirs
                .iter()
                .filter(|d| *d != trimmed && !d.starts_with(&under))
                .cloned()
                .collect();
            *self.dirs.write().unwrap() = Arc::new(kept_dirs);
        }
        MetaCache::global().remove_under(trimmed);
    }

    /// `src`（文件或文件夹）移 / 改名到 `dest`：索引里命中它或以
    /// `src/` 为前缀的曲目路径换前缀，根目录配置、标签缓存与数据库
    /// 跟着换键（内容没变，指纹只按新路径重算——不重读标签）。
    /// 在调用方的线程上同步执行（一条曲目一次 stat）。
    pub fn move_path(&self, src: &str, dest: &str) {
        let sep = std::path::MAIN_SEPARATOR;
        let (src, dest) = (
            src.trim_end_matches(sep).to_owned(),
            dest.trim_end_matches(sep).to_owned(),
        );
        if src == dest {
            return;
        }
        let under = format!("{src}{sep}");
        // 曲库根本身（或装着根的上级目录）被移走：根配置换前缀、
        // 保存并让监视器重新对齐。
        let moved_root = {
            let mut guard = self.config.lock().unwrap();
            let mut config = (**guard).clone();
            let mut touched = false;
            for root in config.roots.iter_mut() {
                let trimmed = root.trim_end_matches(sep).to_owned();
                if trimmed == src {
                    *root = dest.clone();
                    touched = true;
                } else if let Some(rest) = trimmed.strip_prefix(&under) {
                    *root = format!("{dest}{sep}{rest}");
                    touched = true;
                }
            }
            if touched {
                self.save_config(&config);
                *guard = Arc::new(config);
            }
            touched
        };
        if moved_root {
            crate::provider::local::watcher::sync();
        }
        // 索引：每条命中路径换新前缀，指纹 / 修改时间按新路径重算
        // （rename 改了 ctime，不重算会让下次扫描误判内容变了）。
        let current = self.tracks();
        let mut rows: Vec<(String, String, u64, u64)> = Vec::new();
        let updated: Vec<TrackEntry> = current
            .iter()
            .map(|t| {
                let rest = if t.path == src {
                    Some("")
                } else {
                    t.path.strip_prefix(&under)
                };
                let Some(rest) = rest else {
                    return t.clone();
                };
                let path = if rest.is_empty() {
                    dest.clone()
                } else {
                    format!("{dest}{sep}{rest}")
                };
                let (stamp, modified) = std::fs::metadata(&path)
                    .map(|m| {
                        (
                            crate::metadata::file_stamp(&m),
                            m.modified()
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map_or(t.modified, |d| d.as_secs()),
                        )
                    })
                    .unwrap_or((t.stamp, t.modified));
                rows.push((t.path.clone(), path.clone(), stamp, modified));
                // 文件改名后 file_name 也要换（排序 / 搜索用它）。
                let file_name = std::path::Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| t.file_name.clone());
                TrackEntry(Arc::new(TrackShared {
                    path,
                    file_name,
                    stamp,
                    modified,
                    ..(*t.0).clone()
                }))
            })
            .collect();

        let current_dirs = self.dirs();
        let mut dirs_touched = false;
        let updated_dirs: Vec<String> = current_dirs
            .iter()
            .map(|d| {
                if d == &src {
                    dirs_touched = true;
                    dest.clone()
                } else if let Some(rest) = d.strip_prefix(&under) {
                    dirs_touched = true;
                    format!("{dest}{sep}{rest}")
                } else {
                    d.clone()
                }
            })
            .collect();
        if dirs_touched {
            *self.dirs.write().unwrap() = Arc::new(updated_dirs);
        }

        if rows.is_empty() {
            return;
        }
        *self.tracks.write().unwrap() = Arc::new(updated);
        let cache = MetaCache::global();
        cache.move_path(&rows);
        crate::db::later(move || crate::db::move_meta(&rows));
    }

    /// 标签刚被写过的路径：按新文件指纹重读元数据（`MetaCache::entry`
    /// 指纹变了自动重读并把新条目排进数据库待写队列）、原地换索引
    /// 条目并重排。读不出来 / 文件没了的路径从索引里摘走（同扫描
    /// 语义）；不在索引里的路径不加（可能在根目录外或被过滤）。
    /// 不触发整库重扫。
    pub fn refresh_paths(&self, paths: &[String]) {
        let mut tracks = (*self.tracks()).as_slice().to_vec();
        let mut touched = false;
        for path in paths {
            if !tracks.iter().any(|t| t.path == *path) {
                continue;
            }
            match crate::provider::local::read_entry(
                std::path::Path::new(path),
                &self.config().genre_separators,
            ) {
                Some(entry) => {
                    if let Some(slot) = tracks.iter_mut().find(|t| t.path == *path) {
                        *slot = entry;
                    }
                }
                None => tracks.retain(|t| t.path != *path),
            }
            touched = true;
        }
        if !touched {
            return;
        }
        crate::provider::local::sort_tracks(&mut tracks);
        *self.tracks.write().unwrap() = Arc::new(tracks);
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning.load(Ordering::Relaxed)
    }

    pub fn progress(&self) -> ScanProgress {
        ScanProgress {
            scanning: self.is_scanning(),
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            current: self.current.lock().unwrap().clone(),
        }
    }

    /// 改流派分隔符：保存，并用已缓存的标签重新拆分（不重读文件）。
    pub fn set_separators(&self, separators: &[String]) -> Vec<String> {
        let clean = clean_separators(separators);
        self.update_config(|c| c.genre_separators = clean.clone());
        let current = self.tracks();
        let updated: Vec<TrackEntry> = current
            .iter()
            .map(|t| {
                if let Ok(meta) = MetaCache::global().get(Path::new(&t.path)) {
                    let mut shared = (*t.0).clone();
                    shared.genres = split_genres(&meta.genres, &clean);
                    TrackEntry(Arc::new(shared))
                } else {
                    t.clone()
                }
            })
            .collect();
        *self.tracks.write().unwrap() = Arc::new(updated);
        clean
    }

    /// 改艺术家分隔符：保存。
    pub fn set_artist_separators(&self, separators: &[String]) -> Vec<String> {
        let clean = clean_separators(separators);
        self.update_config(|c| c.artist_separators = clean.clone());
        clean
    }

    /// 同步所有源的曲目并重建索引（在调用方的工作线程上运行）。目前只有
    /// `local`；多源时在这里逐个同步、合并写回，`progress` 随之推进。
    pub fn scan(&self) -> usize {
        crate::provider::sync(self)
    }

    /// 重建：丢掉标签缓存与封面缩略图，从文件重新读取全部曲目。
    pub fn rebuild(&self) -> usize {
        crate::provider::rebuild(self)
    }
}

/// 流派切分：任一分隔符处切开 → trim 每段 → 去空段、去重。
pub fn split_genres(raw: &[String], separators: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in raw {
        let mut parts = vec![value.as_str()];
        for sep in separators.iter().filter(|s| !s.is_empty()) {
            parts = parts
                .into_iter()
                .flat_map(|p| p.split(sep.as_str()))
                .collect();
        }
        for part in parts {
            let part = part.trim();
            if !part.is_empty() && !out.iter().any(|g| g == part) {
                out.push(part.to_owned());
            }
        }
    }
    out
}

/// 艺术家切分：任一分隔符处切开 → trim 每段 → 去空段、去重。
pub fn split_artists(raw: &str, separators: &[String]) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }
    let mut parts = vec![raw];
    for sep in separators.iter().filter(|s| !s.is_empty()) {
        parts = parts
            .into_iter()
            .flat_map(|p| p.split(sep.as_str()))
            .collect();
    }
    let mut out: Vec<String> = Vec::new();
    for part in parts {
        let part = part.trim();
        if !part.is_empty() && !out.iter().any(|a| a == part) {
            out.push(part.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use std::sync::{Mutex, RwLock};

    /// 造一条只带路径 / 大小 / 修改时间的曲目。
    fn entry(path: &str, size: u64) -> TrackEntry {
        TrackEntry(Arc::new(TrackShared {
            provider: ProviderId::Local,
            path: path.to_owned(),
            file_name: std::path::Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            album_artist: String::new(),
            genres: Vec::new(),
            size_bytes: size,
            modified: 0,
            stamp: 0,
            meta: Arc::new(crate::metadata::MetaInfo::default()),
        }))
    }

    fn test_store(dir: PathBuf, roots: Vec<String>, tracks: Vec<TrackEntry>) -> LibraryStore {
        LibraryStore {
            dir,
            config: Mutex::new(Arc::new(LibraryConfig {
                roots,
                ..Default::default()
            })),
            tracks: RwLock::new(Arc::new(tracks)),
            dirs: RwLock::new(Arc::new(Vec::new())),
            scanning: AtomicBool::new(false),
            done: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            current: Mutex::new(String::new()),
        }
    }

    #[test]
    fn move_path_rekeys_index_and_roots() {
        let sep = std::path::MAIN_SEPARATOR;
        let base = std::env::temp_dir().join(format!("zeo-move-path-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let sub = base.join("music/sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("a.flac"), b"x").unwrap();
        let music = base.join("music").to_string_lossy().into_owned();
        let renamed = base.join("renamed").to_string_lossy().into_owned();
        let old_sub = format!("{music}{sep}sub");
        let new_sub = format!("{music}{sep}renamed");
        let store = test_store(
            base.clone(),
            vec![music.clone()],
            vec![entry(&format!("{old_sub}{sep}a.flac"), 1)],
        );
        // 文件夹已移走：索引换前缀，不在其下的根不动，指纹按新路径重算。
        std::fs::rename(&old_sub, &new_sub).unwrap();
        store.move_path(&old_sub, &new_sub);
        assert_eq!(store.tracks()[0].path, format!("{new_sub}{sep}a.flac"));
        assert_eq!(store.roots(), vec![music.clone()]);
        assert_ne!(store.tracks()[0].stamp, 0, "指纹按新路径重算");
        // 根本身被改名：根配置跟着换。
        std::fs::rename(&music, &renamed).unwrap();
        store.move_path(&music, &renamed);
        assert_eq!(store.roots(), vec![renamed.clone()]);
        assert_eq!(
            store.tracks()[0].path,
            format!("{renamed}{sep}renamed{sep}a.flac")
        );
        // 摘除：索引清掉。
        store.remove_under(&format!("{renamed}{sep}renamed"));
        assert!(store.tracks().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn genre_split() {
        let raw = vec!["Rock; Indie; Dream Pop".to_owned(), " Rock ".to_owned()];
        assert_eq!(
            split_genres(&raw, &[";".into()]),
            vec!["Rock", "Indie", "Dream Pop"]
        );
    }

    #[test]
    fn artist_split() {
        let raw = "Jay Chou; Mayday / Eason Chan";
        let seps = vec![";".to_owned(), "/".to_owned()];
        assert_eq!(
            split_artists(raw, &seps),
            vec!["Jay Chou", "Mayday", "Eason Chan"]
        );
        assert_eq!(split_artists("  ;;  ", &[";".into()]), Vec::<String>::new());
        assert_eq!(split_artists("Solo", &[]), vec!["Solo"]);
    }

    #[test]
    fn genre_split_drops_empty() {
        let raw = vec![";;".to_owned(), " ; Jazz ;".to_owned()];
        assert_eq!(split_genres(&raw, &[";".into()]), vec!["Jazz"]);
    }

    #[test]
    fn genre_split_several_separators() {
        let raw = vec!["J-Pop / Anime, Game".to_owned()];
        let seps = vec!["/".to_owned(), ",".to_owned()];
        assert_eq!(split_genres(&raw, &seps), vec!["J-Pop", "Anime", "Game"]);
        assert_eq!(
            split_genres(&raw, &[]),
            vec!["J-Pop / Anime, Game"],
            "没有分隔符时整段保留"
        );
    }

    #[test]
    fn old_config_migrates() {
        let old: LibraryConfig =
            serde_json::from_str(r#"{"roots":["/m"],"genre_separator":"/"}"#).unwrap();
        let config = old.normalized();
        assert_eq!(config.genre_separators, vec!["/"]);
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            !json.contains("genre_separator\""),
            "旧字段不再写出：{json}"
        );
        let fresh: LibraryConfig = serde_json::from_str(r#"{"roots":[]}"#).unwrap();
        assert_eq!(
            fresh.theme_color,
            ThemeColor::Cover,
            "缺主题色字段时出厂为跟随封面"
        );
        assert_eq!(
            fresh.sidebar_section,
            SidebarSection::Playlists,
            "缺侧栏视图字段时出厂为歌单"
        );
        assert_eq!(
            fresh.normalized().genre_separators,
            vec![";"],
            "缺字段用出厂值"
        );
        let none: LibraryConfig =
            serde_json::from_str(r#"{"roots":[],"genre_separators":[]}"#).unwrap();
        assert!(
            none.normalized().genre_separators.is_empty(),
            "显式清空时不拆分"
        );
    }

    #[test]
    fn theme_color_serde_roundtrip() {
        assert_eq!(
            serde_json::from_str::<ThemeColor>(r#""neutral""#).unwrap(),
            ThemeColor::Neutral
        );
        assert_eq!(
            serde_json::from_str::<ThemeColor>(r#""cover""#).unwrap(),
            ThemeColor::Cover
        );
        assert_eq!(
            serde_json::from_str::<ThemeColor>(r#""red""#).unwrap(),
            ThemeColor::Custom(0xe5263f)
        );
        assert_eq!(
            serde_json::from_str::<ThemeColor>(r#""blue""#).unwrap(),
            ThemeColor::Custom(0x2563eb)
        );
        assert_eq!(
            serde_json::from_str::<ThemeColor>(r##""#123456""##).unwrap(),
            ThemeColor::Custom(0x123456)
        );
        assert_eq!(
            serde_json::from_str::<ThemeColor>(r#""123456""#).unwrap(),
            ThemeColor::Custom(0x123456)
        );
        assert_eq!(
            serde_json::to_string(&ThemeColor::Neutral).unwrap(),
            r#""neutral""#
        );
        assert_eq!(
            serde_json::to_string(&ThemeColor::Cover).unwrap(),
            r#""cover""#
        );
        assert_eq!(
            serde_json::to_string(&ThemeColor::Custom(0xe5263f)).unwrap(),
            r#""red""#
        );
        assert_eq!(
            serde_json::to_string(&ThemeColor::Custom(0x123456)).unwrap(),
            r##""#123456""##
        );
    }

    #[test]
    fn sidebar_section_serde_roundtrip() {
        assert_eq!(
            serde_json::from_str::<SidebarSection>(r#""playlists""#).unwrap(),
            SidebarSection::Playlists
        );
        assert_eq!(
            serde_json::from_str::<SidebarSection>(r#""tags""#).unwrap(),
            SidebarSection::Tags
        );
        assert_eq!(
            serde_json::from_str::<SidebarSection>(r#""folders""#).unwrap(),
            SidebarSection::Folders
        );
        assert_eq!(
            serde_json::to_string(&SidebarSection::Playlists).unwrap(),
            r#""playlists""#
        );
        assert_eq!(
            serde_json::to_string(&SidebarSection::Tags).unwrap(),
            r#""tags""#
        );
        assert_eq!(
            serde_json::to_string(&SidebarSection::Folders).unwrap(),
            r#""folders""#
        );
    }

    /// 本地天数 / 小时 / 周起点：东八区（+8h）与负偏移都验。
    #[test]
    fn local_time_math() {
        const CST: i64 = 8 * 3600;
        // 2026-11-04 00:00:00+08:00 = 2026-11-03T16:00:00Z（日序号 20760）。
        let utc = |h: u64, m: u64| 1_793_664_000 + h * 3600 + m * 60;
        // UTC 16:00 → 本地 11-04 00:00。
        assert_eq!(local_hour(utc(16, 0), CST), 0);
        assert_eq!(local_hour(utc(17, 30), CST), 1);
        // UTC 15:59:59 还在前一天。
        assert_eq!(local_hour(utc(15, 59), CST), 23);
        // 11-04 是周三：本周起点 = 11-02 周一 00:00+08:00 = UTC 11-01 16:00。
        let monday = local_week_start(utc(16, 0), CST);
        assert_eq!(local_day(monday, CST) - local_day(utc(16, 0), CST), -2);
        assert_eq!(local_hour(monday, CST), 0, "周起点落在本地周一 00:00");
        // 周日（11-08 本地 12:00）仍归本周一。
        let sunday = monday + 5 * 86400 + 12 * 3600;
        assert_eq!(local_week_start(sunday, CST), monday);
        // 下周一零点后起点前移。
        let next = monday + 7 * 86400;
        assert_eq!(local_week_start(next, CST), next);
        // 负偏移（UTC-5）：本地时间比 UTC 慢。
        assert_eq!(local_hour(utc(16, 0), -5 * 3600), 11);
        let m = local_week_start(utc(16, 0), -5 * 3600);
        assert_eq!(local_hour(m, -5 * 3600), 0);
    }

    /// 本地日期时间格式化：跨日（+8h）、负偏移、纪元与闰日边界。
    #[test]
    fn format_local_datetime_math() {
        // 2026-11-03T16:00:00Z +8h → 次日 00:00。
        assert_eq!(format_local_at(1_793_721_600, 8 * 3600), "2026-11-04 00:00");
        // 同一时刻 UTC-5 → 当天上午 11:00。
        assert_eq!(
            format_local_at(1_793_721_600, -5 * 3600),
            "2026-11-03 11:00"
        );
        // 纪元零秒；负偏移回前一天。
        assert_eq!(format_local_at(0, 0), "1970-01-01 00:00");
        assert_eq!(format_local_at(0, -3600), "1969-12-31 23:00");
        // 闰日：2024-02-29T12:34:00Z。
        assert_eq!(format_local_at(1_709_210_040, 0), "2024-02-29 12:34");
    }
}
