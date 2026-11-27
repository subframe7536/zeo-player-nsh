//! 本地文件源：根目录持久化、遍历目录建索引（带进度），`watcher` 监视根
//! 目录变化并触发重扫。

pub mod watcher;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::config::{
    LibraryConfig, LibraryStore, ProviderId, TrackEntry, TrackShared, split_artists, split_genres,
};
use zeo_metadata::{CacheEntry, MetaCache};

/// 支持的音频扩展名（不含 .mp4——可能是视频；opus 暂缓）。
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "m4a", "aac", "ogg", "oga", "aif", "aiff",
];

/// 用数据库里上次的元数据建初始索引，界面马上有曲库；随后的扫描只重读
/// 变了的文件。
pub(crate) fn initial_index(config: &LibraryConfig) -> Vec<TrackEntry> {
    let mut tracks: Vec<TrackEntry> = MetaCache::global()
        .entries()
        .into_iter()
        .filter(|(path, _)| {
            config
                .roots
                .iter()
                .any(|root| Path::new(path).starts_with(root))
        })
        .filter(|(_, entry)| passes(entry.value.duration, entry.size, config))
        .map(|(path, entry)| track_entry(path, &entry, config))
        .collect();
    sort_tracks(&mut tracks);
    tracks
}

/// 收集初始根目录下所有子目录。
pub(crate) fn initial_dirs(roots: &[String]) -> Vec<String> {
    let mut dirs = Vec::new();
    for root in roots {
        let root_path = Path::new(root);
        for entry in walkdir::WalkDir::new(root_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_dir() && entry.path() != root_path {
                dirs.push(entry.path().to_string_lossy().into_owned());
            }
        }
    }
    dirs.sort_unstable();
    dirs.dedup();
    dirs
}

/// 扫描所有根目录并重建索引：先遍历目录得到总数，再逐个读标签，
/// `store.progress()` 随之推进。
pub(crate) fn scan(store: &LibraryStore) -> usize {
    if store.scanning.swap(true, Ordering::AcqRel) {
        return store.tracks().len();
    }
    store.done.store(0, Ordering::Relaxed);
    store.total.store(0, Ordering::Relaxed);
    let config = store.config();
    let mut files: Vec<(PathBuf, Option<std::fs::Metadata>)> = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    let mut sidecar_stems: std::collections::HashSet<(PathBuf, String)> =
        std::collections::HashSet::new();
    for root in &config.roots {
        (*store.current.lock().unwrap()).clone_from(root);
        collect_audio_and_dirs(Path::new(root), &mut files, &mut dirs, &mut sidecar_stems);
    }
    dirs.sort_unstable();
    dirs.dedup();
    store.set_dirs(dirs);
    store.total.store(files.len(), Ordering::Relaxed);
    let cache = MetaCache::global();
    cache.begin_batch();

    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, 4)
        .min(files.len().max(1));
    let next_idx = std::sync::atomic::AtomicUsize::new(0);
    let collected: std::sync::Mutex<Vec<TrackEntry>> =
        std::sync::Mutex::new(Vec::with_capacity(files.len()));
    let sidecar_ref = &sidecar_stems;
    let files_ref = &files;
    let config_ref = &config;

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                let mut local_entries = Vec::new();
                loop {
                    let i = next_idx.fetch_add(1, Ordering::Relaxed);
                    if i >= files_ref.len() {
                        break;
                    }
                    let (path, meta) = &files_ref[i];
                    if i % 8 == 0
                        && let Some(name) = path.file_name()
                    {
                        *store.current.lock().unwrap() = name.to_string_lossy().into_owned();
                    }
                    if let Some(entry) = read_entry_with_meta(path, meta.clone(), config_ref)
                        && passes(entry.duration, entry.size_bytes, config_ref)
                    {
                        let has_sidecar = path
                            .parent()
                            .zip(path.file_stem())
                            .is_some_and(|(dir, stem)| {
                                sidecar_ref.contains(&(
                                    dir.to_path_buf(),
                                    stem.to_string_lossy().to_ascii_lowercase(),
                                ))
                            });
                        if entry.has_art || has_sidecar {
                            zeo_metadata::covers::request_low(&entry.path);
                        }
                        local_entries.push(entry);
                    }
                    store.done.fetch_add(1, Ordering::Relaxed);
                }
                if !local_entries.is_empty() {
                    collected.lock().unwrap().extend(local_entries);
                }
            });
        }
    });

    let mut entries = collected.into_inner().unwrap();
    cache.end_batch();
    // 已不存在的文件从缓存和数据库里去掉；被过滤的保留元数据（阈值改回
    // 就不用重读标签）。
    let keep: std::collections::HashSet<String> = files
        .iter()
        .map(|(p, _)| p.to_string_lossy().into_owned())
        .collect();
    cache.retain(&keep);
    sort_tracks(&mut entries);
    let count = entries.len();
    // 「新添加」对账：第一次见到路径才记入庫时间，已不在库的清掉。
    let paths: Vec<String> = entries.iter().map(|t| t.path.clone()).collect();
    let now = crate::config::unix_now();
    zeo_db::later(move || zeo_db::reconcile_added(&paths, now));
    *store.tracks.write().unwrap() = Arc::new(entries);
    store.current.lock().unwrap().clear();
    store.scanning.store(false, Ordering::Release);
    count
}

/// 重建：丢掉标签缓存与封面缩略图，从文件重新读取全部曲目。
pub(crate) fn rebuild(store: &LibraryStore) -> usize {
    if store.is_scanning() {
        return store.tracks().len();
    }
    MetaCache::global().clear();
    zeo_metadata::covers::clear_cache();
    scan(store)
}

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

fn collect_audio_and_dirs(
    root: &Path,
    files: &mut Vec<(PathBuf, Option<std::fs::Metadata>)>,
    dirs: &mut Vec<String>,
    sidecar_stems: &mut std::collections::HashSet<(PathBuf, String)>,
) {
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir() {
            if entry.path() != root {
                dirs.push(entry.path().to_string_lossy().into_owned());
            }
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let ext = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let Some(ext) = ext else {
            continue;
        };
        if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
            let meta = entry.metadata().ok();
            files.push((entry.into_path(), meta));
        } else if IMAGE_EXTENSIONS.contains(&ext.as_str())
            && let (Some(dir), Some(stem)) = (entry.path().parent(), entry.path().file_stem())
        {
            sidecar_stems.insert((
                dir.to_path_buf(),
                stem.to_string_lossy().to_ascii_lowercase(),
            ));
        }
    }
}

/// 时长 / 大小阈值内的音频才进索引（`0` 为不过滤）。
fn passes(duration: f64, size_bytes: u64, config: &LibraryConfig) -> bool {
    duration >= f64::from(config.min_duration_secs)
        && size_bytes >= u64::from(config.min_size_kb) * 1024
}

/// 读一个文件的索引条目；读不出标签/属性的文件跳过。
pub(crate) fn read_entry(path: &Path, config: &LibraryConfig) -> Option<TrackEntry> {
    let entry = MetaCache::global().entry(path).ok()?;
    Some(track_entry(
        path.to_string_lossy().into_owned(),
        &entry,
        config,
    ))
}

pub(crate) fn read_entry_with_meta(
    path: &Path,
    meta: Option<std::fs::Metadata>,
    config: &LibraryConfig,
) -> Option<TrackEntry> {
    let entry = MetaCache::global().entry_with_meta(path, meta).ok()?;
    Some(track_entry(
        path.to_string_lossy().into_owned(),
        &entry,
        config,
    ))
}

/// 缓存条目 → 索引条目：标签 `Arc` 与元数据缓存共享，不复制字符串。
fn track_entry(path: String, entry: &CacheEntry, config: &LibraryConfig) -> TrackEntry {
    let meta = &entry.value;
    TrackEntry(Arc::new(TrackShared {
        provider: ProviderId::Local,
        file_name: Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path,
        album_artist: {
            let aa = meta.album_artist.trim();
            if aa.is_empty() {
                let art = meta.artist.trim();
                if art.is_empty() {
                    "Unknown Artist".to_owned()
                } else {
                    art.to_owned()
                }
            } else {
                aa.to_owned()
            }
        },
        genres: split_genres(&meta.genres, &config.genre_separators),
        artists: split_artists(&meta.artist, &config.artist_separators),
        size_bytes: entry.size,
        modified: entry.modified,
        stamp: entry.stamp,
        meta: meta.clone(),
    }))
}

/// 索引顺序：歌手、专辑、碟号、音轨号、标题。
pub(crate) fn sort_tracks(entries: &mut [TrackEntry]) {
    entries.sort_by(|a, b| {
        a.display_artist()
            .cmp(b.display_artist())
            .then_with(|| a.display_album().cmp(b.display_album()))
            .then_with(|| a.disk.cmp(&b.disk))
            .then_with(|| a.track.cmp(&b.track))
            .then_with(|| a.display_title().cmp(b.display_title()))
    });
}

/// 把 `path` 移到回收站；`permanent` 时直接删除（不可恢复）。在调用方
/// 的工作线程上运行（回收站 API 可能慢）。
pub fn delete_file(path: &Path, permanent: bool) -> std::io::Result<()> {
    if permanent {
        std::fs::remove_file(path)
    } else {
        trash_one(path)
    }
}

/// 把文件夹（含其中全部内容）移到回收站；`permanent` 时直接删除（不可
/// 恢复）。在调用方的工作线程上运行（回收站 API 可能慢）。
pub fn delete_dir(path: &Path, permanent: bool) -> std::io::Result<()> {
    if permanent {
        std::fs::remove_dir_all(path)
    } else {
        trash_one(path)
    }
}

/// 回收站删除一项：macOS 用 `NSFileManager.trashItemAtURL`——不走
/// `trash` 默认的 osascript / Finder 脚本（要辅助功能权限、起子进程、
/// 有提示音）。代价是 Finder 的「放回原处」在部分系统上没有。
#[cfg(target_os = "macos")]
fn trash_one(path: &Path) -> std::io::Result<()> {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut ctx = trash::TrashContext::new();
    ctx.set_delete_method(DeleteMethod::NsFileManager);
    ctx.delete(path)
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// 回收站删除一项（Windows）：`trash` 默认就走 `IFileOperation`，原生 API。
#[cfg(not(target_os = "macos"))]
fn trash_one(path: &Path) -> std::io::Result<()> {
    trash::delete(path).map_err(|e| std::io::Error::other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LibraryConfig;

    #[test]
    fn filter_thresholds() {
        let cfg = LibraryConfig::default(); // 10 秒 / 100 KB
        assert!(passes(30.0, 500_000, &cfg));
        assert!(!passes(5.0, 500_000, &cfg), "短于时长阈值");
        assert!(!passes(30.0, 50_000, &cfg), "小于大小阈值");
        assert!(!passes(5.0, 50_000, &cfg));
        assert!(passes(10.0, 100 * 1024, &cfg), "恰好等于阈值放行");
        let mut open = cfg;
        open.min_duration_secs = 0;
        open.min_size_kb = 0;
        assert!(passes(0.1, 1, &open), "阈值为 0 时全放行");
    }
}

#[cfg(test)]
mod real_library {
    use crate::config::library_store;

    /// `cargo test -- --ignored scan_reports_progress`：用 ~/Music 真实曲库检查进度推进。
    #[test]
    #[ignore = "reads the real ~/Music library"]
    fn scan_reports_progress() {
        let music = dirs::audio_dir().expect("~/Music");
        // SAFETY(测试): 只有本测试读写这个变量。
        unsafe { std::env::set_var("ZEO_PLAYER_DATA_DIR", "/tmp/zeo-player-test-data-progress") };
        let _ = std::fs::remove_dir_all("/tmp/zeo-player-test-data-progress");
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
        assert!(
            !p.scanning && p.done == p.total && p.total >= count,
            "{p:?} count={count}"
        );
        assert!(
            seen.windows(2).all(|w| w[0] <= w[1]) && seen.len() >= 2,
            "进度单调推进：{} 次采样",
            seen.len()
        );
        let started = std::time::Instant::now();
        assert_eq!(store.rebuild(), count, "重建后曲目数不变");
        eprintln!(
            "{count} 首，进度采样 {} 次，首个 {:?} 末个 {:?}，重建 {:?}",
            seen.len(),
            seen.first(),
            seen.last(),
            started.elapsed()
        );
        store.set_separators(&["/".into()]);
        assert_eq!(store.config().genre_separators, vec!["/"]);
    }
}
