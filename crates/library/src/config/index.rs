use std::sync::Arc;

use super::*;

impl LibraryStore {
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
                            zeo_metadata::file_stamp(&m),
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
        zeo_db::later(move || zeo_db::move_meta(&rows));
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
