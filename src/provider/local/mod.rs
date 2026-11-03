//! 本地文件源：根目录持久化、遍历目录建索引（带进度），`watcher` 监视根
//! 目录变化并触发重扫。

pub(crate) mod watcher;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::config::{
    LibraryConfig, LibraryStore, ProviderId, TrackEntry, TrackShared, split_genres,
};
use crate::metadata::{CacheEntry, MetaCache};

/// 基线音频扩展名（opus/ogg 暂缓，恢复时加回）。
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "wav"];

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
        .map(|(path, entry)| track_entry(path, &entry, &config.genre_separators))
        .collect();
    sort_tracks(&mut tracks);
    tracks
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
    let mut files: Vec<PathBuf> = Vec::new();
    for root in &config.roots {
        *store.current.lock().unwrap() = root.clone();
        collect_audio_files(Path::new(root), &mut files);
    }
    store.total.store(files.len(), Ordering::Relaxed);
    let cache = MetaCache::global();
    cache.begin_batch();
    let mut entries: Vec<TrackEntry> = Vec::with_capacity(files.len());
    for (i, path) in files.iter().enumerate() {
        if let Some(name) = path.file_name() {
            *store.current.lock().unwrap() = name.to_string_lossy().into_owned();
        }
        if let Some(entry) = read_entry(path, &config.genre_separators)
            && passes(entry.duration, entry.size_bytes, &config)
        {
            // 封面缩略图交给后台线程池预生成（低优先级，界面可见的请求
            // 插队）：扫描完浏览列表不用再转圈。本地同名图片也算有封面。
            if entry.has_art
                || crate::metadata::covers::find_sidecar(Path::new(&entry.path)).is_some()
            {
                crate::metadata::covers::request_low(&entry.path);
            }
            entries.push(entry);
        }
        store.done.store(i + 1, Ordering::Relaxed);
    }
    cache.end_batch();
    // 已不存在的文件从缓存和数据库里去掉；被过滤的保留元数据（阈值改回
    // 就不用重读标签）。
    let keep: std::collections::HashSet<String> = files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    cache.retain(&keep);
    sort_tracks(&mut entries);
    let count = entries.len();
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
    crate::metadata::covers::clear_cache();
    scan(store)
}

fn collect_audio_files(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let ext = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        if ext.is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.as_str())) {
            out.push(entry.into_path());
        }
    }
}

/// 时长 / 大小阈值内的音频才进索引（`0` 为不过滤）。
fn passes(duration: f64, size_bytes: u64, config: &LibraryConfig) -> bool {
    duration >= f64::from(config.min_duration_secs)
        && size_bytes >= u64::from(config.min_size_kb) * 1024
}

/// 读一个文件的索引条目；读不出标签/属性的文件跳过。
fn read_entry(path: &Path, separators: &[String]) -> Option<TrackEntry> {
    let entry = MetaCache::global().entry(path).ok()?;
    Some(track_entry(
        path.to_string_lossy().into_owned(),
        &entry,
        separators,
    ))
}

/// 缓存条目 → 索引条目：标签 `Arc` 与元数据缓存共享，不复制字符串。
fn track_entry(path: String, entry: &CacheEntry, separators: &[String]) -> TrackEntry {
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
                meta.artist.clone()
            } else {
                aa.to_owned()
            }
        },
        genres: split_genres(&meta.genres, separators),
        size_bytes: entry.size,
        modified: entry.modified,
        stamp: entry.stamp,
        meta: meta.clone(),
    }))
}

/// 索引顺序：歌手、专辑、碟号、音轨号、标题。
fn sort_tracks(entries: &mut [TrackEntry]) {
    entries.sort_by(|a, b| {
        a.artist
            .cmp(&b.artist)
            .then(a.album.cmp(&b.album))
            .then(a.disk.cmp(&b.disk))
            .then(a.track.cmp(&b.track))
            .then(a.title.cmp(&b.title))
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
