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

/// 库索引里的一条曲目（TSX TrackInfo 的 Rust 侧形态）。
#[derive(Debug, Clone, PartialEq)]
pub struct TrackEntry {
    /// 来源提供方。
    pub provider: ProviderId,
    pub path: String,
    pub file_name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genres: Vec<String>,
    pub year: Option<u32>,
    pub track: Option<u32>,
    pub disk: Option<u32>,
    pub duration: f64,
    pub bit_rate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub has_art: bool,
    pub size_bytes: u64,
    pub modified: u64,
    /// 文件指纹（[`crate::metadata::file_stamp`]），内容一变就变。
    pub stamp: u64,
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
    /// 本地同名图片（jpg / jpeg / png / webp）优先于内嵌封面。
    #[serde(default = "default_true")]
    pub prefer_local_covers: bool,
    /// 「删除文件」不经过回收站、直接删除（不可恢复）。
    #[serde(default)]
    pub delete_permanently: bool,
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
            prefer_local_covers: default_true(),
            delete_permanently: false,
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
    config: std::sync::Mutex<LibraryConfig>,
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
            config: std::sync::Mutex::new(config),
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
    pub fn config(&self) -> LibraryConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn roots(&self) -> Vec<String> {
        self.config().roots
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
        let mut config = self.config.lock().unwrap();
        if !config.roots.iter().any(|r| r == &as_string) {
            config.roots.push(as_string);
        }
        let roots = config.roots.clone();
        self.save_config(&config);
        drop(config);
        crate::provider::local::watcher::sync();
        Ok(roots)
    }

    /// 移除音乐根目录。
    pub fn remove_root(&self, path: &str) -> Vec<String> {
        let mut config = self.config.lock().unwrap();
        config.roots.retain(|r| r != path);
        let roots = config.roots.clone();
        self.save_config(&config);
        drop(config);
        crate::provider::local::watcher::sync();
        roots
    }

    pub fn font(&self) -> Option<String> {
        self.config().font
    }

    /// 设置界面字体（`None`：系统字体），保存。
    pub fn set_font(&self, font: Option<String>) {
        let mut config = self.config.lock().unwrap();
        config.font = font.filter(|f| !f.trim().is_empty());
        self.save_config(&config);
    }

    pub fn fade_ms(&self) -> u32 {
        self.config().fade_ms
    }

    pub fn set_fade_ms(&self, ms: u32) {
        let mut config = self.config.lock().unwrap();
        config.fade_ms = ms;
        self.save_config(&config);
    }

    pub fn lyrics_align(&self) -> LyricsAlign {
        self.config().lyrics_align
    }

    pub fn set_lyrics_align(&self, align: LyricsAlign) {
        let mut config = self.config.lock().unwrap();
        config.lyrics_align = align;
        self.save_config(&config);
    }

    pub fn appearance(&self) -> Appearance {
        self.config().appearance
    }

    /// 设置界面外观（保存）。
    pub fn set_appearance(&self, appearance: Appearance) {
        let mut config = self.config.lock().unwrap();
        config.appearance = appearance;
        self.save_config(&config);
    }

    pub fn loop_mode(&self) -> crate::engine::audio::LoopMode {
        self.config().loop_mode
    }

    pub fn set_loop_mode(&self, mode: crate::engine::audio::LoopMode) {
        let mut config = self.config.lock().unwrap();
        config.loop_mode = mode;
        self.save_config(&config);
    }

    /// 本地同名歌词文件是否优先于内嵌歌词。
    pub fn prefer_local_lyrics(&self) -> bool {
        self.config().prefer_local_lyrics
    }

    pub fn set_prefer_local_lyrics(&self, on: bool) {
        let mut config = self.config.lock().unwrap();
        config.prefer_local_lyrics = on;
        self.save_config(&config);
    }

    /// 本地同名图片文件是否优先于内嵌封面。
    pub fn prefer_local_covers(&self) -> bool {
        self.config().prefer_local_covers
    }

    pub fn set_prefer_local_covers(&self, on: bool) {
        let mut config = self.config.lock().unwrap();
        config.prefer_local_covers = on;
        self.save_config(&config);
    }

    /// 「删除文件」是否不经过回收站、直接删除。
    pub fn delete_permanently(&self) -> bool {
        self.config().delete_permanently
    }

    pub fn set_delete_permanently(&self, on: bool) {
        let mut config = self.config.lock().unwrap();
        config.delete_permanently = on;
        self.save_config(&config);
    }

    pub fn close_action(&self) -> CloseAction {
        self.config().close_action
    }

    pub fn set_close_action(&self, action: CloseAction) {
        let mut config = self.config.lock().unwrap();
        config.close_action = action;
        self.save_config(&config);
        drop(config);
        crate::window::set_close_action(action);
    }

    /// 重新打开时是否恢复上次的窗口大小与位置。
    pub fn remember_frame(&self) -> bool {
        self.config().remember_frame
    }

    pub fn set_remember_frame(&self, on: bool) {
        let mut config = self.config.lock().unwrap();
        config.remember_frame = on;
        if !on {
            config.window_frame = None;
        }
        self.save_config(&config);
        drop(config);
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
        let mut config = self.config.lock().unwrap();
        if !config.remember_frame {
            return;
        }
        config.window_frame = Some(frame);
        self.save_config(&config);
    }

    pub fn watching(&self) -> bool {
        self.config().watch
    }

    /// 主题色（强调色）。
    pub fn theme_color(&self) -> ThemeColor {
        self.config().theme_color
    }

    /// 设置主题色（保存）。
    pub fn set_theme_color(&self, color: ThemeColor) {
        let mut config = self.config.lock().unwrap();
        config.theme_color = color;
        self.save_config(&config);
    }

    /// 过滤：短于此时长（秒）的音频不进索引。
    pub fn min_duration_secs(&self) -> u32 {
        self.config().min_duration_secs
    }

    /// 设置时长过滤阈值（保存；0 为不过滤）。
    pub fn set_min_duration_secs(&self, secs: u32) {
        let mut config = self.config.lock().unwrap();
        config.min_duration_secs = secs;
        self.save_config(&config);
    }

    /// 过滤：小于此大小（KB）的音频不进索引。
    pub fn min_size_kb(&self) -> u32 {
        self.config().min_size_kb
    }

    /// 设置大小过滤阈值（保存；0 为不过滤）。
    pub fn set_min_size_kb(&self, kb: u32) {
        let mut config = self.config.lock().unwrap();
        config.min_size_kb = kb;
        self.save_config(&config);
    }

    /// 打开/关闭文件夹监视（保存设置）。
    pub fn set_watch(&self, on: bool) {
        {
            let mut config = self.config.lock().unwrap();
            config.watch = on;
            self.save_config(&config);
        }
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

    pub fn separators(&self) -> Vec<String> {
        self.config().genre_separators
    }

    /// 改流派分隔符：保存，并用已缓存的标签重新拆分（不重读文件）。
    pub fn set_separators(&self, separators: &[String]) -> Vec<String> {
        let clean = clean_separators(separators);
        {
            let mut config = self.config.lock().unwrap();
            config.genre_separators = clean.clone();
            self.save_config(&config);
        }
        let current = self.tracks();
        let updated: Vec<TrackEntry> = current
            .iter()
            .map(|t| {
                let mut t = t.clone();
                if let Ok(meta) = MetaCache::global().get(Path::new(&t.path)) {
                    t.genres = split_genres(&meta.genres, &clean);
                }
                t
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
