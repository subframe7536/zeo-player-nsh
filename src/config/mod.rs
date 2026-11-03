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

use serde::{Deserialize, Serialize};

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

/// 出厂流派分隔符。
pub const DEFAULT_SEPARATORS: &[&str] = &[";"];

/// 出厂过滤：短于 10 秒的音频不进曲库。
pub const DEFAULT_MIN_DURATION_SECS: u32 = 10;
/// 出厂过滤：小于 100 KB 的音频不进曲库。
pub const DEFAULT_MIN_SIZE_KB: u32 = 100;

/// 主题色（强调色）：默认跟随当前曲目封面的主色调（没有封面或封面
/// 不饱和时回黑白），也可钉住黑白或预设色。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeColor {
    /// 黑白。
    Neutral,
    Red,
    Orange,
    Green,
    Blue,
    Purple,
    /// 当前曲目封面的主色调。
    #[default]
    Cover,
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
    /// ReplayGain 音量标准化（模式 / 预放大 / 无标签增益 / 防削波）。
    #[serde(default)]
    pub replaygain: crate::engine::audio::ReplayGainConfig,
    /// 均衡器（开关 / 前级 / 10 段增益）。
    #[serde(default)]
    pub eq: crate::engine::eq::EqConfig,
    /// 重新打开时恢复上次的窗口大小与位置。
    #[serde(default = "default_true")]
    pub remember_frame: bool,
    /// 上次窗口的常规位置与大小（Windows 的 `SetWindowPlacement` 用；
    /// macOS 走系统自己的 frame 自动保存，不写字段）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_frame: Option<SavedFrame>,
}

/// 持久化的窗口位置（屏幕像素）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedFrame {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// 关闭时是最大化的。
    #[serde(default)]
    pub maximized: bool,
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

fn default_true() -> bool {
    true
}

impl Default for LibraryConfig {
    fn default() -> Self {
        LibraryConfig {
            roots: Vec::new(),
            genre_separator: None,
            genre_separators: default_separators(),
            watch: false,
            font: None,
            close_action: CloseAction::Ask,
            fade_ms: default_fade_ms(),
            loop_mode: crate::engine::audio::LoopMode::default(),
            lyrics_align: LyricsAlign::default(),
            appearance: Appearance::default(),
            theme_color: ThemeColor::default(),
            min_duration_secs: default_min_duration_secs(),
            min_size_kb: default_min_size_kb(),
            prefer_local_lyrics: default_true(),
            global_lyrics_offset_ms: 0,
            prefer_local_covers: default_true(),
            show_covers: default_true(),
            delete_permanently: false,
            pause_on_device_change: true,
            replaygain: crate::engine::audio::ReplayGainConfig::default(),
            eq: crate::engine::eq::EqConfig::default(),
            remember_frame: true,
            window_frame: None,
        }
    }
}

fn default_separators() -> Vec<String> {
    DEFAULT_SEPARATORS.iter().map(|s| (*s).to_owned()).collect()
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
        self
    }
}

/// 现在（Unix 秒）。
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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
        let _ = std::fs::create_dir_all(&dir);
        let config = load_config(&dir);
        // 先用数据库里上次的元数据建索引，界面马上有曲库；随后的同步只重读变了的内容。
        let tracks = crate::provider::initial_index(&config);
        LibraryStore {
            dir,
            config: std::sync::Mutex::new(Arc::new(config)),
            tracks: std::sync::RwLock::new(Arc::new(tracks)),
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
                c.window_frame = None;
            }
        });
        crate::window::set_remember_frame(on);
    }

    /// 上次的窗口位置（`remember_frame` 开着时才有意义）。
    #[cfg(target_os = "windows")]
    pub fn window_frame(&self) -> Option<SavedFrame> {
        self.config().window_frame
    }

    /// 记录当前窗口位置（Windows 的 `WM_EXITSIZEMOVE` / 退出时调用）。
    #[cfg(target_os = "windows")]
    pub fn set_window_frame(&self, frame: SavedFrame) {
        let mut guard = self.config.lock().unwrap();
        if !guard.remember_frame {
            return;
        }
        let mut config = (**guard).clone();
        config.window_frame = Some(frame);
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
        if rows.is_empty() {
            return;
        }
        *self.tracks.write().unwrap() = Arc::new(updated);
        let cache = MetaCache::global();
        cache.move_path(&rows);
        crate::db::later(move || crate::db::move_meta(&rows));
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
            scanning: AtomicBool::new(false),
            done: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            current: Mutex::new(String::new()),
        }
    }

    #[test]
    fn move_path_rekeys_index_and_roots() {
        let sep = std::path::MAIN_SEPARATOR;
        let base = std::env::temp_dir().join(format!("zeo-move-test-{}", std::process::id()));
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
}
