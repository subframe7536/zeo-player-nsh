//! SQLite 存储：曲目元数据（封面之外的全部：标签、技术参数、内嵌歌词，
//! 按文件指纹失效）与播放状态（上次播放的曲目、位置和队列）。文件是数据
//! 目录里的 `zeo-player.db`；打不开时什么也不存，应用照常只用内存。

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::metadata::{CacheEntry, MetaInfo};

/// 当前表结构版本（`PRAGMA user_version`）。改表时加一，并在 [`migrate`] 里迁移。
const SCHEMA_VERSION: i32 = 1;

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

const COLUMNS: &str = "path, stamp, size, modified, title, artist, album, album_artist, duration, has_art, lyrics, \
    bit_rate, bit_depth, sample_rate, channels, codec, quality, genres, year, track, track_total, disk, disk_total, \
    album_artists, composers";

fn db() -> Option<&'static Mutex<Connection>> {
    static DB: OnceLock<Option<Mutex<Connection>>> = OnceLock::new();
    DB.get_or_init(|| {
        // 单元测试不碰真实数据目录（自己设了 ZEO_PLAYER_DATA_DIR 的测试除外）。
        if cfg!(test) && std::env::var_os("ZEO_PLAYER_DATA_DIR").is_none() {
            return None;
        }
        let dir = crate::library::data_dir();
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

fn list(raw: String) -> Vec<String> {
    serde_json::from_str(&raw).unwrap_or_default()
}

fn json(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_owned())
}

// --- 曲目元数据 ---------------------------------------------------------------

/// 读出全部元数据（启动时一次）。
pub fn load_meta() -> Vec<(String, CacheEntry)> {
    with("读取元数据", |conn| {
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM tracks"))?;
        let rows = stmt.query_map([], |r| {
            let meta = MetaInfo {
                title: r.get(4)?,
                artist: r.get(5)?,
                album: r.get(6)?,
                album_artist: r.get(7)?,
                duration: r.get(8)?,
                has_art: r.get(9)?,
                lyrics: r.get(10)?,
                bit_rate: r.get(11)?,
                bit_depth: r.get(12)?,
                sample_rate: r.get(13)?,
                channels: r.get(14)?,
                codec: r.get(15)?,
                quality: r.get(16)?,
                genres: list(r.get(17)?),
                year: r.get(18)?,
                track: r.get(19)?,
                track_total: r.get(20)?,
                disk: r.get(21)?,
                disk_total: r.get(22)?,
                album_artists: list(r.get(23)?),
                composers: list(r.get(24)?),
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
    })
    .unwrap_or_default()
}

/// 写入（同一路径覆盖），一个事务。
pub fn save_meta(rows: &[(String, CacheEntry)]) {
    if rows.is_empty() {
        return;
    }
    with("写入元数据", |conn| {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(&format!(
                "INSERT OR REPLACE INTO tracks ({COLUMNS}) VALUES \
                 (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)"
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
                    m.lyrics,
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
                ])?;
            }
        }
        tx.commit()
    });
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
