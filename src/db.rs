//! SQLite 存储：曲目元数据（封面与歌词之外的全部：标签、技术参数，
//! 按文件指纹失效；歌词不入库——只缓存播放位置附近的几首）与播放状态
//! （上次播放的曲目、位置和队列）。文件是数据目录里的 `zeo-player.db`；
//! 打不开时什么也不存，应用照常只用内存。

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::metadata::{CacheEntry, MetaInfo};

/// 当前表结构版本（`PRAGMA user_version`）。改表时加一，并在 [`migrate`] 里迁移。
const SCHEMA_VERSION: i32 = 4;

const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS tracks (
    path TEXT PRIMARY KEY,
    stamp INTEGER NOT NULL,
    size INTEGER NOT NULL,
    modified INTEGER NOT NULL,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    album TEXT NOT NULL,
    album_artist TEXT NOT NULL,
    duration REAL NOT NULL,
    has_art INTEGER NOT NULL,
    lyrics TEXT,
    bit_rate INTEGER,
    bit_depth INTEGER,
    sample_rate INTEGER,
    channels INTEGER,
    codec TEXT,
    quality TEXT,
    genres TEXT NOT NULL,
    year INTEGER,
    track INTEGER,
    track_total INTEGER,
    disk INTEGER,
    disk_total INTEGER,
    album_artists TEXT NOT NULL,
    composers TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

const COLUMNS: &str = "path, stamp, size, modified, title, artist, album, album_artist, duration, has_art, \
    bit_rate, bit_depth, sample_rate, channels, codec, quality, genres, year, track, track_total, disk, disk_total, \
    album_artists, composers, art_hue, art_sat, rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak";

fn db() -> Option<&'static Mutex<Connection>> {
    static DB: OnceLock<Option<Mutex<Connection>>> = OnceLock::new();
    DB.get_or_init(|| {
        // 单元测试不碰真实数据目录（自己设了 ZEO_PLAYER_DATA_DIR 的测试除外）。
        if cfg!(test) && std::env::var_os("ZEO_PLAYER_DATA_DIR").is_none() {
            return None;
        }
        let dir = crate::config::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        match open(&dir.join("zeo-player.db")) {
            Ok(conn) => Some(Mutex::new(conn)),
            Err(err) => {
                eprintln!("zeo-player: 数据库打开失败: {err}");
                None
            }
        }
    })
    .as_ref()
}

fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(2))?;
    // WAL + NORMAL：写入不逐条 fsync，崩溃最多丢最后几次提交，库不会坏。
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 1 {
        conn.execute_batch(SCHEMA_V1)?;
    }
    if version < 2 {
        // 封面主色调（扫描时 covers 工作线程算好写入；老行 NULL = 未算）。
        conn.execute_batch(
            "ALTER TABLE tracks ADD COLUMN art_hue REAL;
             ALTER TABLE tracks ADD COLUMN art_sat REAL;",
        )?;
    }
    if version < 3 {
        // 歌词不再入全局缓存 / 数据库（内嵌歌词按需读，只缓存播放位置
        // 附近的几首）：清掉老行里存的歌词文本。列保留，新行写 NULL。
        conn.execute_batch("UPDATE tracks SET lyrics = NULL")?;
    }
    if version < 4 {
        // ReplayGain 标签随扫描入库；迁移前的老行为 NULL，装载时由
        // 引擎回退到现场读标签（metadata::read_replaygain）。
        conn.execute_batch(
            "ALTER TABLE tracks ADD COLUMN rg_track_gain REAL;
             ALTER TABLE tracks ADD COLUMN rg_track_peak REAL;
             ALTER TABLE tracks ADD COLUMN rg_album_gain REAL;
             ALTER TABLE tracks ADD COLUMN rg_album_peak REAL;",
        )?;
    }
    if version < SCHEMA_VERSION {
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}

/// 在连接上做 `f`；没有数据库或出错时返回 `None`（错误打到 stderr）。
fn with<T>(what: &str, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Option<T> {
    let mut conn = db()?.lock().unwrap_or_else(|e| e.into_inner());
    match f(&mut conn) {
        Ok(value) => Some(value),
        Err(err) => {
            eprintln!("zeo-player: 数据库{what}失败: {err}");
            None
        }
    }
}

/// 在数据库写线程上做 `f`（按提交顺序，不阻塞调用方）。
pub fn later(f: impl FnOnce() + Send + 'static) {
    type Job = Box<dyn FnOnce() + Send>;
    static WRITER: OnceLock<Mutex<std::sync::mpsc::Sender<Job>>> = OnceLock::new();
    let tx = WRITER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let _ = std::thread::Builder::new()
            .name("zeo-player-db".into())
            .spawn(move || {
                for job in rx {
                    job();
                }
            });
        Mutex::new(tx)
    });
    let _ = tx
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send(Box::new(f));
}

/// 收缩 SQLite 的页缓存与预分配（隐藏到托盘时调）：还掉连接持有的
/// 空闲页，真降提交内存。在写线程上做，不阻塞界面。
pub fn shrink_memory() {
    later(|| {
        with("收缩内存", |conn| {
            conn.execute_batch("PRAGMA shrink_memory")
        });
    });
}

fn list(raw: String) -> Vec<String> {
    serde_json::from_str(&raw).unwrap_or_default()
}

fn json(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_owned())
}

// --- 曲目元数据 ---------------------------------------------------------------

/// 读出全部元数据（启动时一次）。
pub fn load_meta() -> Vec<(String, CacheEntry)> {
    with("读取元数据", |conn| load_meta_from(conn)).unwrap_or_default()
}

fn load_meta_from(conn: &Connection) -> rusqlite::Result<Vec<(String, CacheEntry)>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM tracks"))?;
    let rows = stmt.query_map([], |r| {
        let meta = MetaInfo {
            title: r.get(4)?,
            artist: r.get(5)?,
            album: r.get(6)?,
            album_artist: r.get(7)?,
            duration: r.get(8)?,
            has_art: r.get(9)?,
            bit_rate: r.get(10)?,
            bit_depth: r.get(11)?,
            sample_rate: r.get(12)?,
            channels: r.get(13)?,
            codec: r.get(14)?,
            quality: r.get(15)?,
            genres: list(r.get(16)?),
            year: r.get(17)?,
            track: r.get(18)?,
            track_total: r.get(19)?,
            disk: r.get(20)?,
            disk_total: r.get(21)?,
            album_artists: list(r.get(22)?),
            composers: list(r.get(23)?),
            art_hue: match (r.get::<_, Option<f64>>(24)?, r.get::<_, Option<f64>>(25)?) {
                (Some(h), Some(s)) => Some((h as f32, s as f32)),
                _ => None,
            },
            replaygain: crate::metadata::ReplayGain {
                track_gain: r.get::<_, Option<f64>>(26)?.map(|v| v as f32),
                track_peak: r.get::<_, Option<f64>>(27)?.map(|v| v as f32),
                album_gain: r.get::<_, Option<f64>>(28)?.map(|v| v as f32),
                album_peak: r.get::<_, Option<f64>>(29)?.map(|v| v as f32),
            },
        };
        let entry = CacheEntry {
            stamp: r.get::<_, i64>(1)? as u64,
            size: r.get::<_, i64>(2)? as u64,
            modified: r.get::<_, i64>(3)? as u64,
            value: Arc::new(meta),
        };
        Ok((r.get::<_, String>(0)?, entry))
    })?;
    rows.collect()
}

/// 写入（同一路径覆盖），一个事务。
pub fn save_meta(rows: &[(String, CacheEntry)]) {
    if rows.is_empty() {
        return;
    }
    with("写入元数据", |conn| save_meta_to(conn, rows));
}

fn save_meta_to(conn: &mut Connection, rows: &[(String, CacheEntry)]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(&format!(
            "INSERT OR REPLACE INTO tracks ({COLUMNS}) VALUES \
             (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30)"
        ))?;
        for (path, entry) in rows {
            let m = &entry.value;
            stmt.execute(params![
                path,
                entry.stamp as i64,
                entry.size as i64,
                entry.modified as i64,
                m.title,
                m.artist,
                m.album,
                m.album_artist,
                m.duration,
                m.has_art,
                m.bit_rate,
                m.bit_depth,
                m.sample_rate,
                m.channels,
                m.codec,
                m.quality,
                json(&m.genres),
                m.year,
                m.track,
                m.track_total,
                m.disk,
                m.disk_total,
                json(&m.album_artists),
                json(&m.composers),
                m.art_hue.map(|(h, _)| h as f64),
                m.art_hue.map(|(_, s)| s as f64),
                m.replaygain.track_gain,
                m.replaygain.track_peak,
                m.replaygain.album_gain,
                m.replaygain.album_peak,
            ])?;
        }
    }
    tx.commit()
}

/// 只留下 `keep` 里的路径（扫描后删掉已不存在的文件）。
pub fn retain_meta(keep: &HashSet<String>) {
    with("清理元数据", |conn| {
        let tx = conn.transaction()?;
        let gone: Vec<String> = {
            let mut stmt = tx.prepare("SELECT path FROM tracks")?;
            let paths = stmt.query_map([], |r| r.get::<_, String>(0))?;
            paths
                .filter_map(Result::ok)
                .filter(|p| !keep.contains(p))
                .collect()
        };
        {
            let mut delete = tx.prepare("DELETE FROM tracks WHERE path = ?1")?;
            for path in &gone {
                delete.execute([path])?;
            }
        }
        tx.commit()
    });
}

/// 删掉 `path` 的元数据行（界面「删除记录」）。
pub fn remove_meta(path: &str) {
    with("删除元数据", |conn| {
        conn.execute("DELETE FROM tracks WHERE path = ?1", [path])
            .map(|_| ())
    });
}

/// 删掉 `path` 下（含自身）的元数据行（文件夹删除 / 根摘除）。
pub fn remove_meta_under(path: &str) {
    let sep = std::path::MAIN_SEPARATOR;
    let trimmed = path.trim_end_matches(sep);
    let under = format!("{trimmed}{sep}");
    with("删除元数据", |conn| {
        conn.execute(
            "DELETE FROM tracks WHERE path = ?1 OR substr(path, 1, ?2) = ?3",
            params![trimmed, under.len() as i64, under],
        )
        .map(|_| ())
    });
}

/// `rows = (旧路径, 新路径, 新指纹, 新修改时间)`：文件 / 文件夹移动或改
/// 名后把元数据行换到新路径（内容没变，标签不重读）。
pub fn move_meta(rows: &[(String, String, u64, u64)]) {
    if rows.is_empty() {
        return;
    }
    with("换元数据路径", |conn| {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE tracks SET path = ?1, stamp = ?2, modified = ?3 WHERE path = ?4",
            )?;
            for (old, new, stamp, modified) in rows {
                stmt.execute(params![new, *stamp as i64, *modified as i64, old])?;
            }
        }
        tx.commit()
    });
}

/// 删掉全部元数据（重建曲库）。
pub fn clear_meta() {
    with("清空元数据", |conn| {
        conn.execute("DELETE FROM tracks", []).map(|_| ())
    });
}

// --- 播放状态 -----------------------------------------------------------------

/// 上次播放的曲目与位置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LastPlayed {
    pub path: String,
    /// 位置（秒）。
    pub time: f64,
}

const LAST_PLAYED: &str = "last_played";
const LAST_QUEUE: &str = "last_queue";

fn get_state(key: &str) -> Option<String> {
    with("读取播放状态", |conn| {
        conn.query_row("SELECT value FROM state WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()
    })
    .flatten()
}

fn set_state(key: &str, value: &str) {
    with("写入播放状态", |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO state (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map(|_| ())
    });
}

/// 上次播放的曲目与它所在的队列。
pub fn last_played() -> Option<(LastPlayed, Vec<String>)> {
    let last: LastPlayed = serde_json::from_str(&get_state(LAST_PLAYED)?).ok()?;
    let queue = get_state(LAST_QUEUE).map(list).unwrap_or_default();
    Some((last, queue))
}

pub fn save_last_played(last: &LastPlayed) {
    if let Ok(value) = serde_json::to_string(last) {
        set_state(LAST_PLAYED, &value);
    }
}

/// 队列可能有上万条，只在它变了时写。
pub fn save_queue(queue: &[String]) {
    set_state(LAST_QUEUE, &json(queue));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{MetaInfo, ReplayGain};

    /// 内存库 + 全部迁移；tracks 表 `name` 列存在性。
    fn columns(conn: &Connection) -> Vec<String> {
        conn.prepare("PRAGMA table_info(tracks)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// v3 库迁到 v4：四个 rg_ 列出现、老行还在且 rg 列为 NULL。
    #[test]
    fn replaygain_migrate_adds_columns() {
        let conn = Connection::open_in_memory().unwrap();
        // 造一个 v3 的库：v1 建表 + v2/v3 的迁移语句 + user_version=3。
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(
            "ALTER TABLE tracks ADD COLUMN art_hue REAL;
             ALTER TABLE tracks ADD COLUMN art_sat REAL;
             UPDATE tracks SET lyrics = NULL;
             PRAGMA user_version = 3;",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tracks (path, stamp, size, modified, title, artist, album, \
             album_artist, duration, has_art, genres, album_artists, composers) \
             VALUES ('a.flac', 1, 1, 1, 't', 'a', 'al', 'aa', 1.0, 0, '[]', '[]', '[]')",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap();
        let version: i32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let cols = columns(&conn);
        for col in [
            "rg_track_gain",
            "rg_track_peak",
            "rg_album_gain",
            "rg_album_peak",
        ] {
            assert!(cols.iter().any(|c| c == col), "缺列 {col}");
        }
        // 老行还在，rg 列为 NULL。
        let (n, tg): (i64, Option<f64>) = conn
            .query_row(
                "SELECT count(*), (SELECT rg_track_gain FROM tracks WHERE path='a.flac') \
                 FROM tracks",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(tg, None);
    }

    /// 带 ReplayGain 的 CacheEntry 写入后读回相等。
    #[test]
    fn replaygain_db_roundtrip() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let rg = ReplayGain {
            track_gain: Some(-6.0),
            track_peak: Some(0.5),
            album_gain: Some(-3.0),
            album_peak: None,
        };
        let entry = CacheEntry {
            stamp: 1,
            size: 1,
            modified: 1,
            value: Arc::new(MetaInfo {
                replaygain: rg,
                ..Default::default()
            }),
        };
        save_meta_to(&mut conn, &[("a.flac".to_owned(), entry)]).unwrap();
        let rows = load_meta_from(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "a.flac");
        assert_eq!(rows[0].1.value.replaygain, rg);
    }
}
