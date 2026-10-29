//! 多根目录曲库：根目录持久化、扫盘建索引（带进度）、流派按自定义分隔符
//! 切开并 trim。

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

use crate::metadata::MetaCache;

/// 基线音频扩展名（opus/ogg 暂缓，恢复时加回）。
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "wav"];

/// 库索引里的一条曲目（TSX TrackInfo 的 Rust 侧形态）。
#[derive(Debug, Clone, PartialEq)]
pub struct TrackEntry {
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

/// 出厂流派分隔符。
pub const DEFAULT_SEPARATORS: &[&str] = &[";"];

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
}

impl Default for LibraryConfig {
    fn default() -> Self {
        LibraryConfig {
            roots: Vec::new(),
            genre_separator: None,
            genre_separators: default_separators(),
            watch: false,
            font: None,
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
    tracks: std::sync::RwLock<Arc<Vec<TrackEntry>>>,
    scanning: std::sync::atomic::AtomicBool,
    done: std::sync::atomic::AtomicUsize,
    total: std::sync::atomic::AtomicUsize,
    current: std::sync::Mutex<String>,
}

/// 应用数据目录（`ZEO_DATA_DIR` 可覆盖）。
pub fn data_dir() -> PathBuf {
    std::env::var_os("ZEO_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("Zeo"))
}

pub fn library_store() -> &'static LibraryStore {
    static STORE: OnceLock<LibraryStore> = OnceLock::new();
    STORE.get_or_init(|| {
        let dir = data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let config = load_config(&dir);
        LibraryStore {
            dir,
            config: std::sync::Mutex::new(config),
            tracks: std::sync::RwLock::new(Arc::new(Vec::new())),
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
        let canonical = std::fs::canonicalize(path).map_err(|e| format!("无法访问 {path}: {e}"))?;
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
        crate::watcher::sync();
        Ok(roots)
    }

    /// 移除音乐根目录。
    pub fn remove_root(&self, path: &str) -> Vec<String> {
        let mut config = self.config.lock().unwrap();
        config.roots.retain(|r| r != path);
        let roots = config.roots.clone();
        self.save_config(&config);
        drop(config);
        crate::watcher::sync();
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

    pub fn watching(&self) -> bool {
        self.config().watch
    }

    /// 打开/关闭文件夹监视（保存设置）。
    pub fn set_watch(&self, on: bool) {
        {
            let mut config = self.config.lock().unwrap();
            config.watch = on;
            self.save_config(&config);
        }
        crate::watcher::sync();
    }

    pub fn tracks(&self) -> Arc<Vec<TrackEntry>> {
        self.tracks.read().unwrap().clone()
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

    /// 扫描所有根目录并重建索引（在调用方的工作线程上运行）：先遍历目录
    /// 得到总数，再逐个读标签，[`Self::progress`] 随之推进。
    pub fn scan(&self) -> usize {
        if self.scanning.swap(true, Ordering::AcqRel) {
            return self.tracks().len();
        }
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        let config = self.config();
        let mut files: Vec<PathBuf> = Vec::new();
        for root in &config.roots {
            *self.current.lock().unwrap() = root.clone();
            collect_audio_files(Path::new(root), &mut files);
        }
        self.total.store(files.len(), Ordering::Relaxed);
        let mut entries: Vec<TrackEntry> = Vec::with_capacity(files.len());
        for (i, path) in files.iter().enumerate() {
            if let Some(name) = path.file_name() {
                *self.current.lock().unwrap() = name.to_string_lossy().into_owned();
            }
            if let Some(entry) = read_entry(path, &config.genre_separators) {
                entries.push(entry);
            }
            self.done.store(i + 1, Ordering::Relaxed);
        }
        entries.sort_by(|a, b| {
            a.artist
                .cmp(&b.artist)
                .then(a.album.cmp(&b.album))
                .then(a.disk.cmp(&b.disk))
                .then(a.track.cmp(&b.track))
                .then(a.title.cmp(&b.title))
        });
        let count = entries.len();
        *self.tracks.write().unwrap() = Arc::new(entries);
        self.current.lock().unwrap().clear();
        self.scanning.store(false, Ordering::Release);
        count
    }

    /// 重建：丢掉标签缓存与封面缩略图，从文件重新读取全部曲目。
    pub fn rebuild(&self) -> usize {
        if self.is_scanning() {
            return self.tracks().len();
        }
        MetaCache::global().clear();
        crate::covers::clear_cache();
        self.scan()
    }
}

fn collect_audio_files(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let ext = entry.path().extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
        if ext.is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.as_str())) {
            out.push(entry.into_path());
        }
    }
}

/// 读一个文件的索引条目；读不出标签/属性的文件跳过。
fn read_entry(path: &Path, separators: &[String]) -> Option<TrackEntry> {
    let file_meta = std::fs::metadata(path).ok()?;
    let modified = file_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let meta = MetaCache::global().get(path).ok()?;
    Some(TrackEntry {
        path: path.to_string_lossy().into_owned(),
        file_name: path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        title: meta.title.clone(),
        artist: meta.artist.clone(),
        album: meta.album.clone(),
        album_artist: {
            let aa = meta.album_artist.trim();
            if aa.is_empty() { meta.artist.clone() } else { aa.to_owned() }
        },
        genres: split_genres(&meta.genres, separators),
        year: meta.year,
        track: meta.track,
        disk: meta.disk,
        duration: meta.duration,
        bit_rate: meta.bit_rate,
        sample_rate: meta.sample_rate,
        channels: meta.channels,
        has_art: meta.artwork.is_some(),
        size_bytes: file_meta.len(),
        modified,
        stamp: crate::metadata::file_stamp(&file_meta),
    })
}

/// 流派切分：任一分隔符处切开 → trim 每段 → 去空段、去重。
pub fn split_genres(raw: &[String], separators: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in raw {
        let mut parts = vec![value.as_str()];
        for sep in separators.iter().filter(|s| !s.is_empty()) {
            parts = parts.into_iter().flat_map(|p| p.split(sep.as_str())).collect();
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
        assert_eq!(split_genres(&raw, &[";".into()]), vec!["Rock", "Indie", "Dream Pop"]);
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
        assert_eq!(split_genres(&raw, &[]), vec!["J-Pop / Anime, Game"], "没有分隔符时整段保留");
    }

    #[test]
    fn old_config_migrates() {
        let old: LibraryConfig = serde_json::from_str(r#"{"roots":["/m"],"genre_separator":"/"}"#).unwrap();
        let config = old.normalized();
        assert_eq!(config.genre_separators, vec!["/"]);
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("genre_separator\""), "旧字段不再写出：{json}");
        let fresh: LibraryConfig = serde_json::from_str(r#"{"roots":[]}"#).unwrap();
        assert_eq!(fresh.normalized().genre_separators, vec![";"], "缺字段用出厂值");
        let none: LibraryConfig = serde_json::from_str(r#"{"roots":[],"genre_separators":[]}"#).unwrap();
        assert!(none.normalized().genre_separators.is_empty(), "显式清空时不拆分");
    }
}

#[cfg(test)]
mod real_library {
    use super::*;

    /// `cargo test -- --ignored scan_reports_progress`：用 ~/Music 真实曲库检查进度推进。
    #[test]
    #[ignore = "reads the real ~/Music library"]
    fn scan_reports_progress() {
        let music = dirs::audio_dir().expect("~/Music");
        // SAFETY(测试): 只有本测试读写这个变量。
        unsafe { std::env::set_var("ZEO_DATA_DIR", "/tmp/zeo-test-data-progress") };
        let _ = std::fs::remove_dir_all("/tmp/zeo-test-data-progress");
        let store = library_store();
        store.add_root(&music.to_string_lossy()).unwrap();
        let worker = std::thread::spawn(|| library_store().scan());
        let mut seen = Vec::new();
        while !worker.is_finished() {
            let p = store.progress();
            if p.scanning && p.total > 0 {
                seen.push(p.done);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let count = worker.join().unwrap();
        let p = store.progress();
        assert!(!p.scanning && p.done == p.total && p.total >= count, "{p:?} count={count}");
        assert!(seen.windows(2).all(|w| w[0] <= w[1]) && seen.len() >= 2, "进度单调推进：{} 次采样", seen.len());
        let started = std::time::Instant::now();
        assert_eq!(store.rebuild(), count, "重建后曲目数不变");
        eprintln!("{count} 首，进度采样 {} 次，首个 {:?} 末个 {:?}，重建 {:?}", seen.len(), seen.first(), seen.last(), started.elapsed());
        store.set_separators(&["/".into()]);
        assert_eq!(store.separators(), vec!["/"]);
    }
}
