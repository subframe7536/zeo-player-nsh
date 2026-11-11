use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::{CacheEntry, MetaInfo, file_stamp, read_meta};

/// 新读出、还没写进数据库的条目超过这个数就写一次（扫描中）。
const FLUSH_AT: usize = 256;

pub struct MetaCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
    /// 新读出、还没写进数据库的条目。
    pending: Mutex<Vec<(String, CacheEntry)>>,
    /// 扫描中：攒一批再写（见 [`Self::begin_batch`]）。
    batching: AtomicBool,
    /// 隐藏到托盘时丢过内存条目（[`Self::drop_in_memory`]）：置位期间
    /// `entries` 全量需求从数据库整体回填，`entry` / `art_hue` /
    /// `set_art_hue` 按需走数据库单行读写。
    dropped: AtomicBool,
}

impl MetaCache {
    /// 进程内唯一的缓存；第一次用时从数据库读出上次保存的全部元数据。
    pub fn global() -> &'static MetaCache {
        static CACHE: OnceLock<MetaCache> = OnceLock::new();
        CACHE.get_or_init(|| MetaCache {
            entries: Mutex::new(zeo_db::load_meta().into_iter().collect()),
            pending: Mutex::new(Vec::new()),
            batching: AtomicBool::new(false),
            dropped: AtomicBool::new(false),
        })
    }

    /// 全部条目（启动时据此先显示上次的曲库）。
    pub fn entries(&self) -> Vec<(String, CacheEntry)> {
        let mut entries = self.entries.lock().unwrap();
        if self.dropped.swap(false, Ordering::Relaxed) {
            // 隐藏期间丢过内存条目：曲库索引重建这类全量需求本就需要
            // 全部条目，从数据库整体回填（单行按需回填见 `entry`）。
            *entries = zeo_db::load_meta().into_iter().collect();
        }
        entries
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// 丢掉全部缓存，数据库里的也删掉（重建曲库）。
    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
        self.pending.lock().unwrap().clear();
        zeo_db::clear_meta();
    }

    /// 只丢内存条目（隐藏到托盘时调）：数据库行保留，`entry` /
    /// `art_hue` / `set_art_hue` 按需单行回填与直写——引擎后台换曲、
    /// 封面工作线程照常工作，但只有被碰到的条目重新占内存。
    pub fn drop_in_memory(&self) {
        self.flush();
        let mut entries = self.entries.lock().unwrap();
        // 标志在持锁状态下先置位再清表：否则「清空」与「置位」之间的
        // `entries` 读方会拿到空表且错过回填。
        self.dropped.store(true, Ordering::Relaxed);
        entries.clear();
    }

    /// 移除单条缓存（界面「删除记录」）：连待写队列里同一路径的条目
    /// 一起丢，否则下次 flush 的 `INSERT OR REPLACE` 会把它写回。
    pub fn remove(&self, path: &Path) {
        let key = path.to_string_lossy().into_owned();
        self.entries.lock().unwrap().remove(&key);
        self.pending.lock().unwrap().retain(|(p, _)| *p != key);
    }

    /// 移除 `path` 下（含自身）的全部条目（文件夹删除 / 根摘除）：连
    /// 待写队列与数据库行一起清，文件还在时下次扫描会重新入库。
    pub fn remove_under(&self, path: &str) {
        // 先把待写的旧键落库：否则删完它们还会以旧路径写回一条死行。
        self.flush();
        let sep = std::path::MAIN_SEPARATOR;
        let trimmed = path.trim_end_matches(sep);
        let under = format!("{trimmed}{sep}");
        self.entries
            .lock()
            .unwrap()
            .retain(|p, _| p != trimmed && !p.starts_with(&under));
        self.pending
            .lock()
            .unwrap()
            .retain(|(p, _)| p != trimmed && !p.starts_with(&under));
        zeo_db::remove_meta_under(trimmed);
    }

    /// `rows = (旧路径, 新路径, 新指纹, 新修改时间)`：文件 / 文件夹移
    /// 动或改名后把缓存键换到新路径并带上新指纹（改名改了 ctime，不换
    /// 指纹下次扫描会误判内容变了重读标签）。数据库更新由调用方排。
    pub fn move_path(&self, rows: &[(String, String, u64, u64)]) {
        if rows.is_empty() {
            return;
        }
        // 先把待写的旧键落库：数据库随后整体 UPDATE 到新键；不然
        // pending 里的旧键会在换键之后被 flush 写回一条死行。
        self.flush();
        {
            let mut entries = self.entries.lock().unwrap();
            for (old, new, stamp, modified) in rows {
                if let Some(mut e) = entries.remove(old) {
                    e.stamp = *stamp;
                    e.modified = *modified;
                    entries.insert(new.clone(), e);
                }
            }
        }
        // 待写队列同一路径的也换：不然下次 flush 会按旧路径写回一条死行。
        let mut pending = self.pending.lock().unwrap();
        for (old, new, stamp, modified) in rows {
            for (p, e) in pending.iter_mut() {
                if p == old {
                    *p = new.clone();
                    e.stamp = *stamp;
                    e.modified = *modified;
                }
            }
        }
    }

    /// 只留下 `keep` 里的路径（扫描后去掉已不存在的文件）。
    pub fn retain(&self, keep: &HashSet<String>) {
        self.flush();
        self.entries
            .lock()
            .unwrap()
            .retain(|path, _| keep.contains(path));
        zeo_db::retain_meta(keep);
    }

    /// 开始批量读取：新条目攒够 [`FLUSH_AT`] 条才写一次数据库。
    pub fn begin_batch(&self) {
        self.batching.store(true, Ordering::Relaxed);
    }

    /// 结束批量读取，写入剩下的条目。
    pub fn end_batch(&self) {
        self.batching.store(false, Ordering::Relaxed);
        self.flush();
    }

    fn flush(&self) {
        let rows = std::mem::take(&mut *self.pending.lock().unwrap());
        zeo_db::save_meta(&rows);
    }

    /// 读取（带缓存）：文件未变时直接返回缓存。
    pub fn get(&self, path: &Path) -> Result<Arc<MetaInfo>, String> {
        self.entry(path).map(|e| e.value)
    }

    /// `path` 已存的封面主色调（同步读，界面用）。内存条目被隐藏丢
    /// 掉后回退到数据库单行查——色调本来就在库里，不该因丢缓存重算。
    pub fn art_hue(&self, path: &Path) -> Option<(f32, f32)> {
        let key = path.to_string_lossy().into_owned();
        if let Some(hue) = self
            .entries
            .lock()
            .unwrap()
            .get(key.as_str())
            .and_then(|e| e.value.art_hue)
        {
            return Some(hue);
        }
        if self.dropped.load(Ordering::Relaxed) {
            return zeo_db::meta_hue(&key);
        }
        None
    }

    /// 写封面主色调（covers 工作线程）：条目在且文件指纹未变才写
    /// （指纹变了说明算的是旧封面，等下次读取重建）。写入走 `pending`
    /// 与元数据同一批落库；条目被隐藏丢掉时按同指纹直写库。
    pub fn set_art_hue(&self, path: &Path, hue: Option<(f32, f32)>) {
        let stamp = std::fs::metadata(path).map(|m| file_stamp(&m)).unwrap_or(0);
        let key = path.to_string_lossy().into_owned();
        let row = {
            let mut entries = self.entries.lock().unwrap();
            let Some(entry) = entries.get_mut(&key) else {
                drop(entries);
                // 内存条目被隐藏丢掉：不重建条目，同指纹直写库（`WHERE
                // stamp` 与上面的指纹检查同一语义）。未丢弃时条目缺失
                // = 库里也没有（表随启动装载且只增），保持原静默返回。
                if self.dropped.load(Ordering::Relaxed) {
                    zeo_db::set_meta_hue(&key, stamp, hue);
                }
                return;
            };
            if entry.stamp != stamp || entry.value.art_hue == hue {
                return;
            }
            let mut info = (*entry.value).clone();
            info.art_hue = hue;
            entry.value = Arc::new(info);
            (key, entry.clone())
        };
        let flush = {
            let mut pending = self.pending.lock().unwrap();
            pending.push(row);
            !self.batching.load(Ordering::Relaxed) || pending.len() >= FLUSH_AT
        };
        if flush {
            self.flush();
        }
    }

    /// 同 [`Self::get`]，连同文件指纹、大小与修改时间。
    pub fn entry(&self, path: &Path) -> Result<CacheEntry, String> {
        let key = path.to_string_lossy().into_owned();
        let file = std::fs::metadata(path).ok();
        let stamp = file.as_ref().map_or(0, file_stamp);
        if let Some(entry) = self.entries.lock().unwrap().get(&key)
            && entry.stamp == stamp
        {
            return Ok(entry.clone());
        }
        // 内存条目可能被隐藏丢掉：先试数据库行（指纹一致直接用，免
        // 重读文件）；已在库的条目不进 `pending` 再写一遍。
        if let Some(entry) = zeo_db::load_meta_row(&key)
            && entry.stamp == stamp
        {
            self.entries
                .lock()
                .unwrap()
                .insert(key.clone(), entry.clone());
            return Ok(entry);
        }
        let entry = CacheEntry {
            stamp,
            size: file.as_ref().map_or(0, |m| m.len()),
            modified: file
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs()),
            value: Arc::new(read_meta(path)?),
        };
        self.entries
            .lock()
            .unwrap()
            .insert(key.clone(), entry.clone());
        let flush = {
            let mut pending = self.pending.lock().unwrap();
            pending.push((key, entry.clone()));
            !self.batching.load(Ordering::Relaxed) || pending.len() >= FLUSH_AT
        };
        if flush {
            self.flush();
        }
        Ok(entry)
    }
}
