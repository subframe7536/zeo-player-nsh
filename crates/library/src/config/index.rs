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

    /// 批量从索引里移除多条路径（合并为一次克隆与写锁更新）。
    pub fn remove_tracks(&self, paths: &[String]) {
        if paths.is_empty() {
            return;
        }
        let drop_set: std::collections::HashSet<&str> = paths.iter().map(String::as_str).collect();
        let current = self.tracks();
        if !current.iter().any(|t| drop_set.contains(t.path.as_str())) {
            return;
        }
        let kept: Vec<TrackEntry> = current
            .iter()
            .filter(|t| !drop_set.contains(t.path.as_str()))
            .cloned()
            .collect();
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
                    root.clone_from(&dest);
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

    /// 批量移动 / 重命名多对 `(src, dest)`：合并为一次索引与目录重建、单次缓存换键和单个 SQLite 事务。
    pub fn move_paths(&self, pairs: &[(String, String)]) {
        if pairs.is_empty() {
            return;
        }
        if pairs.len() == 1 {
            self.move_path(&pairs[0].0, &pairs[0].1);
            return;
        }
        let sep = std::path::MAIN_SEPARATOR;
        let normalized: Vec<(String, String, String)> = pairs
            .iter()
            .filter_map(|(s, d)| {
                let src = s.trim_end_matches(sep).to_owned();
                let dest = d.trim_end_matches(sep).to_owned();
                if src == dest {
                    return None;
                }
                let under = format!("{src}{sep}");
                Some((src, dest, under))
            })
            .collect();
        if normalized.is_empty() {
            return;
        }
        let exact_map: std::collections::HashMap<&str, &str> = normalized
            .iter()
            .map(|(s, d, _)| (s.as_str(), d.as_str()))
            .collect();
        let moved_root = {
            let mut guard = self.config.lock().unwrap();
            let mut config = (**guard).clone();
            let mut touched = false;
            for root in config.roots.iter_mut() {
                let trimmed = root.trim_end_matches(sep).to_owned();
                if let Some(&dest) = exact_map.get(trimmed.as_str()) {
                    *root = dest.to_owned();
                    touched = true;
                } else {
                    for (_, dest, under) in &normalized {
                        if let Some(rest) = trimmed.strip_prefix(under) {
                            *root = format!("{dest}{sep}{rest}");
                            touched = true;
                            break;
                        }
                    }
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
        let current = self.tracks();
        let mut rows: Vec<(String, String, u64, u64)> = Vec::new();
        let updated: Vec<TrackEntry> = current
            .iter()
            .map(|t| {
                let mapped = if let Some(&dest) = exact_map.get(t.path.as_str()) {
                    Some(dest.to_owned())
                } else {
                    normalized.iter().find_map(|(_, dest, under)| {
                        t.path
                            .strip_prefix(under)
                            .map(|rest| format!("{dest}{sep}{rest}"))
                    })
                };
                let Some(path) = mapped else {
                    return t.clone();
                };
                let (stamp, modified) = std::fs::metadata(&path)
                    .map(|m| {
                        (
                            zeo_metadata::file_stamp(&m),
                            m.modified()
                                .ok()
                                .and_then(|mt| mt.duration_since(std::time::UNIX_EPOCH).ok())
                                .map_or(t.modified, |d| d.as_secs()),
                        )
                    })
                    .unwrap_or((t.stamp, t.modified));
                rows.push((t.path.clone(), path.clone(), stamp, modified));
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
                if let Some(&dest) = exact_map.get(d.as_str()) {
                    dirs_touched = true;
                    dest.to_owned()
                } else if let Some(new_d) = normalized.iter().find_map(|(_, dest, under)| {
                    d.strip_prefix(under)
                        .map(|rest| format!("{dest}{sep}{rest}"))
                }) {
                    dirs_touched = true;
                    new_d
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
        if paths.is_empty() {
            return;
        }
        let mut tracks = (*self.tracks()).as_slice().to_vec();
        let index: std::collections::HashMap<String, usize> = tracks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.path.clone(), i))
            .collect();
        let config = self.config();
        let cache = MetaCache::global();
        cache.begin_batch();
        let mut removed = std::collections::HashSet::new();
        let mut touched = false;
        for path in paths {
            let Some(&idx) = index.get(path) else {
                continue;
            };
            match crate::provider::local::read_entry(std::path::Path::new(path), &config) {
                Some(entry) => tracks[idx] = entry,
                None => {
                    removed.insert(idx);
                }
            }
            touched = true;
        }
        cache.end_batch();
        if !touched {
            return;
        }
        if !removed.is_empty() {
            let mut i = 0;
            tracks.retain(|_| {
                let keep = !removed.contains(&i);
                i += 1;
                keep
            });
        }
        crate::provider::local::sort_tracks(&mut tracks);
        *self.tracks.write().unwrap() = Arc::new(tracks);
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning.load(Ordering::Relaxed)
    }

    pub fn progress(&self) -> ScanProgress {
        let scanning = self.is_scanning();
        ScanProgress {
            scanning,
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            current: if scanning {
                self.current.lock().unwrap().clone()
            } else {
                String::new()
            },
        }
    }

    /// 改流派分隔符：保存，并用已缓存的标签重新拆分（不重读文件、不触发 stat）。
    pub fn set_separators(&self, separators: &[String]) -> Vec<String> {
        let clean = clean_separators(separators);
        self.update_config(|c| c.genre_separators.clone_from(&clean));
        let current = self.tracks();
        let updated: Vec<TrackEntry> = current
            .iter()
            .map(|t| {
                let mut shared = (*t.0).clone();
                shared.genres = split_genres(&t.meta.genres, &clean);
                TrackEntry(Arc::new(shared))
            })
            .collect();
        *self.tracks.write().unwrap() = Arc::new(updated);
        clean
    }

    /// 改艺术家分隔符：保存，并用已缓存的标签重新拆分（不重读文件、不触发 stat）。
    pub fn set_artist_separators(&self, separators: &[String]) -> Vec<String> {
        let clean = clean_separators(separators);
        self.update_config(|c| c.artist_separators.clone_from(&clean));
        let current = self.tracks();
        let updated: Vec<TrackEntry> = current
            .iter()
            .map(|t| {
                let mut shared = (*t.0).clone();
                shared.artists = split_artists(&t.meta.artist, &clean);
                TrackEntry(Arc::new(shared))
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
