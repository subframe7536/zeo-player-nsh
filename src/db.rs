//! SQLite 存储：曲目元数据（封面与歌词之外的全部：标签、技术参数，
//! 按文件指纹失效；歌词不入库——只缓存播放位置附近的几首）与播放状态
//! （上次播放的曲目、位置和队列）。文件是数据目录里的 `zeo-player.db`；
//! 打不开时什么也不存，应用照常只用内存。

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::metadata::{CacheEntry, MetaInfo};

/// 当前表结构版本（`PRAGMA user_version`）。改表时加一，并在 [`migrate`] 里迁移。
const SCHEMA_VERSION: i32 = 7;

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
    if version < 5 {
        // 每首曲目的手动歌词偏移（毫秒，正值 = 提前）。跟随文件移动；
        // 重建曲库不清。
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS lyrics_offsets (
                path TEXT PRIMARY KEY,
                offset_ms INTEGER NOT NULL
            );",
        )?;
    }
    if version < 6 {
        // 播放记录：越过统计阈值时写一行。跟随文件移动；删除文件、
        // 重建曲库都不清（历史保留）。
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS plays (
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL,
                played_at INTEGER NOT NULL,
                listened REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS plays_path ON plays(path);
            CREATE INDEX IF NOT EXISTS plays_time ON plays(played_at);",
        )?;
    }
    if version < 7 {
        // 收藏时间（首页「新增收藏」要它）：与「我喜欢」歌单内容对应，
        // 升级前的老收藏由启动回填补 `liked_at = 0`——不计新增。
        // 入库时间（首页「新添加」要它）：路径第一次见到时记 `added_at`，
        // 升级前的老曲目由启动回填补文件修改时间。
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS likes (
                path TEXT PRIMARY KEY,
                liked_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS added (
                path TEXT PRIMARY KEY,
                added_at INTEGER NOT NULL
            );",
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

/// 读出全部元数据（启动时一次，及隐藏丢弃后的 `entries` 整体回填）。
pub fn load_meta() -> Vec<(String, CacheEntry)> {
    with("读取元数据", |conn| load_meta_from(conn)).unwrap_or_default()
}

fn meta_row(r: &rusqlite::Row) -> rusqlite::Result<(String, CacheEntry)> {
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
}

fn load_meta_from(conn: &Connection) -> rusqlite::Result<Vec<(String, CacheEntry)>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM tracks"))?;
    let rows = stmt.query_map([], meta_row)?;
    rows.collect()
}

/// 单行元数据（内存缓存隐藏丢弃后 `entry` 的按需回填）：不存在或出错
/// 都回 `None`，调用方再落到现场读标签。
pub fn load_meta_row(path: &str) -> Option<CacheEntry> {
    with("读取元数据行", |conn| {
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM tracks WHERE path = ?1"),
            [path],
            meta_row,
        )
        .optional()
    })
    .flatten()
    .map(|(_, e)| e)
}

/// 已存的封面主色调（`art_hue` 在内存条目被隐藏丢掉后查库）。
pub fn meta_hue(path: &str) -> Option<(f32, f32)> {
    with("读取封面色调", |conn| {
        conn.query_row(
            "SELECT art_hue, art_sat FROM tracks WHERE path = ?1",
            [path],
            |r| Ok((r.get::<_, Option<f64>>(0)?, r.get::<_, Option<f64>>(1)?)),
        )
        .optional()
    })
    .flatten()
    .and_then(|(h, s)| h.zip(s))
    .map(|(h, s)| (h as f32, s as f32))
}

/// 只写封面主色调（内存条目被隐藏丢掉、`set_art_hue` 无处落时直写库）：
/// `WHERE stamp` 保证还是同一份文件（与 `set_art_hue` 的指纹检查同一
/// 语义），不存在的行 / 旧指纹静默不写。
pub fn set_meta_hue(path: &str, stamp: u64, hue: Option<(f32, f32)>) {
    let (h, s) = hue.map_or((None, None), |(h, s)| (Some(h as f64), Some(s as f64)));
    with("写入封面色调", |conn| {
        conn.execute(
            "UPDATE tracks SET art_hue = ?2, art_sat = ?3 WHERE path = ?1 AND stamp = ?4",
            params![path, h, s, stamp as i64],
        )
        .map(|_| ())
    });
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
        {
            // 「新添加」跟曲目走：文件没了，入库时间也不留（删掉再加算重新加入）。
            let mut delete = tx.prepare("DELETE FROM added WHERE path = ?1")?;
            for path in &gone {
                delete.execute([path])?;
            }
        }
        tx.commit()
    });
}

fn remove_meta_on(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM tracks WHERE path = ?1", [path])?;
    conn.execute("DELETE FROM lyrics_offsets WHERE path = ?1", [path])?;
    conn.execute("DELETE FROM added WHERE path = ?1", [path])?;
    Ok(())
}

fn remove_meta_under_on(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    let sep = std::path::MAIN_SEPARATOR;
    let trimmed = path.trim_end_matches(sep);
    let under = format!("{trimmed}{sep}");
    for table in ["tracks", "lyrics_offsets", "added"] {
        conn.execute(
            &format!("DELETE FROM {table} WHERE path = ?1 OR substr(path, 1, ?2) = ?3"),
            params![trimmed, under.len() as i64, under],
        )?;
    }
    Ok(())
}

/// 删掉 `path` 的元数据行（界面「删除记录」）。
pub fn remove_meta(path: &str) {
    with("删除元数据", |conn| remove_meta_on(conn, path));
}

/// 删掉 `path` 下（含自身）的元数据行（文件夹删除 / 根摘除）。
pub fn remove_meta_under(path: &str) {
    with("删除元数据", |conn| remove_meta_under_on(conn, path));
}

fn move_meta_on(conn: &Connection, rows: &[(String, String, u64, u64)]) -> rusqlite::Result<()> {
    {
        let mut stmt = conn.prepare_cached(
            "UPDATE tracks SET path = ?1, stamp = ?2, modified = ?3 WHERE path = ?4",
        )?;
        for (old, new, stamp, modified) in rows {
            stmt.execute(params![new, *stamp as i64, *modified as i64, old])?;
        }
    }
    {
        // 每曲歌词偏移跟随文件移动 / 改名。
        let mut stmt =
            conn.prepare_cached("UPDATE lyrics_offsets SET path = ?1 WHERE path = ?2")?;
        for (old, new, ..) in rows {
            stmt.execute(params![new, old])?;
        }
    }
    {
        // 播放记录跟随文件移动 / 改名（历史保留，只换路径键）。
        let mut stmt = conn.prepare_cached("UPDATE plays SET path = ?1 WHERE path = ?2")?;
        for (old, new, ..) in rows {
            stmt.execute(params![new, old])?;
        }
    }
    {
        // 收藏时间也跟随（同 plays）。
        let mut stmt = conn.prepare_cached("UPDATE likes SET path = ?1 WHERE path = ?2")?;
        for (old, new, ..) in rows {
            stmt.execute(params![new, old])?;
        }
    }
    {
        // 入库时间同样跟随。
        let mut stmt = conn.prepare_cached("UPDATE added SET path = ?1 WHERE path = ?2")?;
        for (old, new, ..) in rows {
            stmt.execute(params![new, old])?;
        }
    }
    Ok(())
}

/// `rows = (旧路径, 新路径, 新指纹, 新修改时间)`：文件 / 文件夹移动或改
/// 名后把元数据行换到新路径（内容没变，标签不重读）。
pub fn move_meta(rows: &[(String, String, u64, u64)]) {
    if rows.is_empty() {
        return;
    }
    with("换元数据路径", |conn| {
        let tx = conn.transaction()?;
        move_meta_on(&tx, rows)?;
        tx.commit()
    });
}

/// 删掉全部元数据（重建曲库）。
pub fn clear_meta() {
    with("清空元数据", |conn| {
        conn.execute("DELETE FROM tracks", []).map(|_| ())
    });
}

// --- 每曲歌词偏移 -------------------------------------------------------------

fn lyrics_offset_on(conn: &Connection, path: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT offset_ms FROM lyrics_offsets WHERE path = ?1",
        [path],
        |r| r.get(0),
    )
    .optional()
    .map(|v| v.unwrap_or(0))
}

fn set_lyrics_offset_on(conn: &Connection, path: &str, offset_ms: i64) -> rusqlite::Result<()> {
    if offset_ms == 0 {
        conn.execute("DELETE FROM lyrics_offsets WHERE path = ?1", [path])?;
    } else {
        conn.execute(
            "INSERT OR REPLACE INTO lyrics_offsets (path, offset_ms) VALUES (?1, ?2)",
            params![path, offset_ms],
        )?;
    }
    Ok(())
}

/// `path` 的手动歌词偏移（毫秒）；没有为 0。
pub fn lyrics_offset(path: &str) -> i64 {
    with("读歌词偏移", |conn| lyrics_offset_on(conn, path)).unwrap_or(0)
}

/// 设置 `path` 的手动歌词偏移（毫秒，正值 = 提前）；0 删除该行。
pub fn set_lyrics_offset(path: &str, offset_ms: i64) {
    with("写歌词偏移", |conn| {
        set_lyrics_offset_on(conn, path, offset_ms)
    });
}

// --- 播放记录 -----------------------------------------------------------------

fn record_play_on(
    conn: &Connection,
    path: &str,
    played_at: u64,
    listened: f64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO plays (path, played_at, listened) VALUES (?1, ?2, ?3)",
        params![path, played_at as i64, listened],
    )?;
    Ok(())
}

/// 记一次播放（`played_at` 为 Unix 秒，`listened` 为实际播出的秒数）。
/// 引擎在越过统计阈值的那一刻写一行；文件移动 / 改名跟随 `move_meta`，
/// 删除与重建曲库不清（历史保留）。
pub fn record_play(path: &str, played_at: u64, listened: f64) {
    with("写播放记录", |conn| {
        record_play_on(conn, path, played_at, listened)
    });
}

// --- 收藏时间 -----------------------------------------------------------------

/// 记一次喜欢（`liked_at` 为 Unix 秒；重新收藏视为新喜欢）。
pub fn record_like(path: &str, liked_at: u64) {
    with("写收藏时间", |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO likes (path, liked_at) VALUES (?1, ?2)",
            params![path, liked_at as i64],
        )?;
        Ok(())
    });
}

/// 取消喜欢。
pub fn remove_like(path: &str) {
    with("删收藏时间", |conn| {
        conn.execute("DELETE FROM likes WHERE path = ?1", params![path])?;
        Ok(())
    });
}

/// 把升级前已存在的收藏路径补进表（`liked_at = 0`：有记录但不计新增）。
pub fn backfill_likes(paths: &[String]) {
    with("补收藏时间", |conn| {
        let mut stmt =
            conn.prepare_cached("INSERT OR IGNORE INTO likes (path, liked_at) VALUES (?1, 0)")?;
        for p in paths {
            stmt.execute(params![p])?;
        }
        Ok(())
    });
}

// --- 入库时间 -----------------------------------------------------------------

/// `added` 表的代次：首页据它重读「新添加」（写是 `db::later` 异步的）。
static ADDED_GEN: AtomicU64 = AtomicU64::new(0);

/// `added` 表变过几次。
pub fn added_generation() -> u64 {
    ADDED_GEN.load(Ordering::Relaxed)
}

/// 扫描结束对账：表里没有的路径以 `now` 记（第一次见到才算入库），不在
/// `paths` 里的行清掉（文件删了再加算重新加入）。在 `db::later` 上跑。
pub fn reconcile_added(paths: &[String], now: u64) {
    let done = with("对账入库时间", |conn| {
        let tx = conn.transaction()?;
        {
            let mut ins =
                tx.prepare_cached("INSERT OR IGNORE INTO added (path, added_at) VALUES (?1, ?2)")?;
            for p in paths {
                ins.execute(params![p, now as i64])?;
            }
        }
        {
            tx.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS added_keep (path TEXT PRIMARY KEY);
                 DELETE FROM added_keep;",
            )?;
            let mut keep =
                tx.prepare_cached("INSERT OR IGNORE INTO added_keep (path) VALUES (?1)")?;
            for p in paths {
                keep.execute(params![p])?;
            }
            tx.execute(
                "DELETE FROM added WHERE path NOT IN (SELECT path FROM added_keep)",
                [],
            )?;
        }
        tx.commit()
    });
    if done.is_some() {
        ADDED_GEN.fetch_add(1, Ordering::Relaxed);
    }
}

/// 升级前的老曲目回填：用文件修改时间当入库时间的近似（`INSERT OR
/// IGNORE`——已经记过的不动）。`entries = (路径, 文件修改时间)`。
pub fn backfill_added(entries: &[(String, u64)]) {
    let done = with("补入库时间", |conn| {
        let mut stmt =
            conn.prepare_cached("INSERT OR IGNORE INTO added (path, added_at) VALUES (?1, ?2)")?;
        for (p, modified) in entries {
            stmt.execute(params![p, *modified as i64])?;
        }
        Ok(())
    });
    if done.is_some() {
        ADDED_GEN.fetch_add(1, Ordering::Relaxed);
    }
}

/// 单条的入库时间（元数据面板「音频参数」用；没记过返回 `None`——
/// `optional` 让无行不进错误日志）。
pub fn added_at(path: &str) -> Option<u64> {
    with("读入库时间", |conn| {
        conn.query_row("SELECT added_at FROM added WHERE path = ?1", [path], |r| {
            r.get::<_, i64>(0)
        })
        .optional()
    })
    .flatten()
    .map(|v| v.max(0) as u64)
}

/// 播放统计摘要（首页用）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayStats {
    /// 全部播放次数（含曲库已没有的历史记录）。
    pub total: u64,
    /// `played_at` 大于 `since` 的播放次数。
    pub since: u64,
    /// （路径，次数）按次数降序，只统计曲库里还在的曲目；零播放的
    /// 按入库时间排在末位（0 次也上榜）。
    pub top_tracks: Vec<(String, u64)>,
    /// （歌手标签原文，次数）按次数降序：`artist` 是标签原文，可能是
    /// `A; B` 这样的组合——按原文分组，不拆分。含零播放的歌手（排在末位），
    /// 首页「热门艺术家」在没听过歌时也有内容。
    pub top_artists: Vec<(String, u64)>,
    /// 最近播放的路径（去重，最新在前），只统计曲库里还在的曲目；
    /// 零播放的曲目按入库时间排在末位。
    pub recent: Vec<String>,
    /// `played_at >= week_start` 的实际播出总秒数。
    pub week_secs: f64,
    /// `liked_at >= week_start` 的收藏数（`liked_at = 0` 的老收藏不算）。
    pub week_likes: u64,
    /// 连续收听天数（本地日）：今天没听时算到昨天为止的连续段。
    pub streak: u64,
    /// 最新入库的路径（`added_at` 倒序，只统计曲库里还在的曲目）。
    pub new_added: Vec<String>,
}

/// 连续天数：本地日序号倒序（去重）从 `today`（或它没听时的昨天）往回
/// 数，断档即停。`days` 为 DESC 序。
fn streak_days(days: &[i64], today: i64) -> u64 {
    let mut expected = if days.first().is_some_and(|&d| d >= today) {
        today
    } else {
        today - 1
    };
    let mut n = 0u64;
    for &d in days {
        if d > expected {
            continue; // 未来时间戳（时钟回拨）跳过
        }
        if d < expected {
            break;
        }
        n += 1;
        expected -= 1;
    }
    n
}

fn play_stats_on(
    conn: &Connection,
    since: u64,
    week_start: u64,
    offset: i64,
    limit: usize,
) -> rusqlite::Result<PlayStats> {
    let limit = limit as i64;
    let total = conn.query_row("SELECT COUNT(*) FROM plays", [], |r| r.get::<_, i64>(0))? as u64;
    let recent_n = conn.query_row(
        "SELECT COUNT(*) FROM plays WHERE played_at > ?1",
        [since as i64],
        |r| r.get::<_, i64>(0),
    )? as u64;
    let week_secs = conn.query_row(
        "SELECT COALESCE(SUM(listened), 0) FROM plays WHERE played_at >= ?1",
        [week_start as i64],
        |r| r.get::<_, f64>(0),
    )?;
    let week_likes = conn.query_row(
        "SELECT COUNT(*) FROM likes WHERE liked_at >= ?1",
        [week_start as i64],
        |r| r.get::<_, i64>(0),
    )? as u64;
    // 本地日序号（去重倒序）：offset 是本地时区相对 UTC 的秒数。
    let days: Vec<i64> = conn
        .prepare(
            "SELECT DISTINCT CAST((played_at + ?1) / 86400 AS INTEGER) d \
             FROM plays ORDER BY d DESC",
        )?
        .query_map([offset], |r| r.get::<_, i64>(0))?
        .collect::<Result<_, _>>()?;
    let today = crate::config::local_day(crate::config::unix_now(), offset);
    let streak = streak_days(&days, today);
    let ranked = |sql: &str| -> rusqlite::Result<Vec<(String, u64)>> {
        conn.prepare(sql)?
            .query_map([limit], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
            })?
            .collect()
    };
    // 三个榜都含零播放的（LEFT JOIN）：播放过的按次数 / 时间在前，
    // 零播放的按入库时间在末。
    let top_tracks = ranked(
        "SELECT t.path, COUNT(p.id) c FROM tracks t \
         LEFT JOIN plays p ON p.path = t.path \
         LEFT JOIN added a ON a.path = t.path \
         GROUP BY t.path \
         ORDER BY c DESC, MAX(p.played_at) DESC, COALESCE(a.added_at, 0) DESC, t.path \
         LIMIT ?1",
    )?;
    let top_artists = ranked(
        "SELECT t.artist, COUNT(p.id) c FROM tracks t \
         LEFT JOIN plays p ON p.path = t.path \
         GROUP BY t.artist ORDER BY c DESC, MAX(p.played_at) DESC, t.artist LIMIT ?1",
    )?;
    let recent = conn
        .prepare(
            "SELECT t.path FROM tracks t \
             LEFT JOIN plays p ON p.path = t.path \
             LEFT JOIN added a ON a.path = t.path \
             GROUP BY t.path \
             ORDER BY MAX(p.played_at) DESC, COALESCE(a.added_at, 0) DESC, t.path \
             LIMIT ?1",
        )?
        .query_map([limit], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let new_added = conn
        .prepare(
            "SELECT a.path FROM added a JOIN tracks t ON t.path = a.path \
             ORDER BY a.added_at DESC, a.path LIMIT ?1",
        )?
        .query_map([limit], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PlayStats {
        total,
        since: recent_n,
        top_tracks,
        top_artists,
        recent,
        week_secs,
        week_likes,
        streak,
        new_added,
    })
}

/// 播放统计摘要（`since` / `week_start` 为 Unix 秒，`offset` 为本地时区
/// 偏移秒，`limit` 为各排行条数）；数据库不可用时全零。
pub fn play_stats(since: u64, week_start: u64, offset: i64, limit: usize) -> PlayStats {
    with("读播放统计", |conn| {
        play_stats_on(conn, since, week_start, offset, limit)
    })
    .unwrap_or_default()
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

    /// 每曲歌词偏移：写入读回；归零删行。
    #[test]
    fn lyric_offset_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        set_lyrics_offset_on(&conn, "/m/a.flac", 500).unwrap();
        assert_eq!(lyrics_offset_on(&conn, "/m/a.flac").unwrap(), 500);
        set_lyrics_offset_on(&conn, "/m/a.flac", 0).unwrap();
        assert_eq!(lyrics_offset_on(&conn, "/m/a.flac").unwrap(), 0);
        let n: i64 = conn
            .query_row("SELECT count(*) FROM lyrics_offsets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    /// 文件移动 / 改名把偏移换到新路径。
    #[test]
    fn lyric_offset_follows_move() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        set_lyrics_offset_on(&conn, "/m/a.flac", 500).unwrap();
        move_meta_on(
            &conn,
            &[("/m/a.flac".to_owned(), "/n/a.flac".to_owned(), 1, 1)],
        )
        .unwrap();
        assert_eq!(lyrics_offset_on(&conn, "/m/a.flac").unwrap(), 0);
        assert_eq!(lyrics_offset_on(&conn, "/n/a.flac").unwrap(), 500);
    }

    /// 文件夹删除清掉下面的偏移，外面的不动。
    #[test]
    fn lyric_offset_removed_under_folder() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let dir = format!(
            "{}m{}x",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );
        let inside = format!("{dir}{}a.flac", std::path::MAIN_SEPARATOR);
        let outside = format!(
            "{}m{}y.flac",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );
        set_lyrics_offset_on(&conn, &inside, 300).unwrap();
        set_lyrics_offset_on(&conn, &outside, 400).unwrap();
        remove_meta_under_on(&conn, &dir).unwrap();
        assert_eq!(lyrics_offset_on(&conn, &inside).unwrap(), 0);
        assert_eq!(lyrics_offset_on(&conn, &outside).unwrap(), 400);
    }

    /// v4 库迁到 v5：lyrics_offsets 表出现、老行还在。
    #[test]
    fn migrate_from_v4_keeps_tracks() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(
            "ALTER TABLE tracks ADD COLUMN art_hue REAL;
             ALTER TABLE tracks ADD COLUMN art_sat REAL;
             UPDATE tracks SET lyrics = NULL;
             ALTER TABLE tracks ADD COLUMN rg_track_gain REAL;
             ALTER TABLE tracks ADD COLUMN rg_track_peak REAL;
             ALTER TABLE tracks ADD COLUMN rg_album_gain REAL;
             ALTER TABLE tracks ADD COLUMN rg_album_peak REAL;
             PRAGMA user_version = 4;",
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
        let n: i64 = conn
            .query_row("SELECT count(*) FROM tracks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        // 新表可写。
        set_lyrics_offset_on(&conn, "a.flac", 100).unwrap();
        assert_eq!(lyrics_offset_on(&conn, "a.flac").unwrap(), 100);
    }

    /// 往 tracks 里插一行最小曲目。
    fn track(conn: &Connection, path: &str, artist: &str) {
        conn.execute(
            "INSERT INTO tracks (path, stamp, size, modified, title, artist, album, \
             album_artist, duration, has_art, genres, album_artists, composers) \
             VALUES (?1, 1, 1, 1, 't', ?2, 'al', 'aa', 1.0, 0, '[]', '[]', '[]')",
            params![path, artist],
        )
        .unwrap();
    }

    /// 播放统计：计数与排序；曲库里没有的历史路径算进总数、不进排行。
    /// 歌手榜与「最近」含零播放的（补在末位），「最常播放」仍只听过的。
    #[test]
    fn play_stats_counts_and_orders() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        track(&conn, "a", "X");
        track(&conn, "b", "X");
        track(&conn, "c", "Y");
        track(&conn, "d", "Z"); // 零播放
        // a×3、c×2、b×1，外加一条孤儿记录（曲库已没有该文件）。
        for (p, at) in [
            ("a", 1),
            ("c", 2),
            ("a", 3),
            ("b", 4),
            ("c", 5),
            ("a", 6),
            ("gone", 7),
        ] {
            record_play_on(&conn, p, at, 1.0).unwrap();
        }
        let s = play_stats_on(&conn, 4, 0, 0, 10).unwrap();
        assert_eq!(s.total, 7);
        assert_eq!(s.since, 3); // played_at 5、6、7
        assert_eq!(s.week_secs, 7.0); // 全部 played_at >= 0 都计
        // 零播放的 d 计 0 也上榜（排末位）。
        assert_eq!(
            s.top_tracks,
            vec![
                ("a".to_owned(), 3),
                ("c".to_owned(), 2),
                ("b".to_owned(), 1),
                ("d".to_owned(), 0)
            ]
        );
        // 歌手排行把孤儿排除：X = a×3 + b×1；零播放的 Z 计 0 排末位。
        assert_eq!(
            s.top_artists,
            vec![
                ("X".to_owned(), 4),
                ("Y".to_owned(), 2),
                ("Z".to_owned(), 0)
            ]
        );
        // 去重，最新在前；零播放的 d 排在最后。
        assert_eq!(s.recent, vec!["a", "c", "b", "d"]);
    }

    /// 文件移动 / 改名：播放记录换到新路径。
    #[test]
    fn play_stats_follow_move() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        track(&conn, "/m/a.flac", "X");
        record_play_on(&conn, "/m/a.flac", 1, 10.0).unwrap();
        move_meta_on(
            &conn,
            &[("/m/a.flac".to_owned(), "/n/a.flac".to_owned(), 1, 1)],
        )
        .unwrap();
        let s = play_stats_on(&conn, 0, 0, 0, 10).unwrap();
        assert_eq!(s.total, 1);
        // 元数据行与播放记录一起换到新路径。
        assert_eq!(s.top_tracks, vec![("/n/a.flac".to_owned(), 1)]);
        assert_eq!(s.recent, vec!["/n/a.flac"]);
    }

    /// 收藏时间：周新增计数、取消、回填补 0、移动跟随。
    #[test]
    fn likes_week_and_follow_move() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO likes (path, liked_at) VALUES
             ('a', 100), ('b', 200), ('old', 0)",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM likes WHERE path = 'b'", [])
            .unwrap();
        // week_start=150：a(100) 不计、old(0) 不计。
        let s = play_stats_on(&conn, 0, 150, 0, 10).unwrap();
        assert_eq!(s.week_likes, 0);
        let s = play_stats_on(&conn, 0, 50, 0, 10).unwrap();
        assert_eq!(s.week_likes, 1, "只有 liked_at>=50 的 a 算新增");
        // 收藏路径跟随 move_meta。
        move_meta_on(&conn, &[("a".to_owned(), "/n/a".to_owned(), 1, 1)]).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM likes WHERE path = '/n/a'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    /// 「新添加」：added_at 倒序、不在曲库的不进榜、移动跟随。
    #[test]
    fn new_added_orders_and_follows_move() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        track(&conn, "old", "X");
        track(&conn, "new", "X");
        conn.execute(
            "INSERT INTO added (path, added_at) VALUES ('new', 200), ('old', 100), ('gone', 300)",
            [],
        )
        .unwrap();
        let s = play_stats_on(&conn, 0, 0, 0, 10).unwrap();
        assert_eq!(s.new_added, vec!["new", "old"], "gone 不在曲库不进榜");
        move_meta_on(&conn, &[("new".to_owned(), "/n/new".to_owned(), 1, 1)]).unwrap();
        let s = play_stats_on(&conn, 0, 0, 0, 10).unwrap();
        assert_eq!(s.new_added[0], "/n/new", "入库时间跟随 move_meta");
    }

    /// 连续收听：本地日序去重；今天没听算到昨天，断档归零。
    #[test]
    fn streak_counts_consecutive_days() {
        // days 是 DESC 序的本地日序号。
        assert_eq!(streak_days(&[], 10), 0);
        assert_eq!(streak_days(&[10, 9, 8], 10), 3);
        assert_eq!(streak_days(&[9, 8, 7], 10), 3, "今天没听：昨天的连段仍算");
        assert_eq!(streak_days(&[8, 7], 10), 0, "昨天也没听：连击已断");
        assert_eq!(streak_days(&[10, 8], 10), 1, "中间断档只算最近一段");
        assert_eq!(streak_days(&[12, 11, 10, 9], 10), 2, "未来日跳过");
    }

    /// 清元数据（重建曲库 / 删除文件）不动播放记录。
    #[test]
    fn play_stats_survive_clear() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        track(&conn, "a", "X");
        record_play_on(&conn, "a", 1, 10.0).unwrap();
        conn.execute("DELETE FROM tracks", []).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM plays", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let s = play_stats_on(&conn, 0, 0, 0, 10).unwrap();
        assert_eq!(s.total, 1); // 历史还在
        assert!(s.top_tracks.is_empty()); // 排行只显示曲库里还在的
    }

    /// v5 库迁到 v6：plays 表出现、老行还在。
    #[test]
    fn play_stats_migrate_from_v5() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(
            "ALTER TABLE tracks ADD COLUMN art_hue REAL;
             ALTER TABLE tracks ADD COLUMN art_sat REAL;
             UPDATE tracks SET lyrics = NULL;
             ALTER TABLE tracks ADD COLUMN rg_track_gain REAL;
             ALTER TABLE tracks ADD COLUMN rg_track_peak REAL;
             ALTER TABLE tracks ADD COLUMN rg_album_gain REAL;
             ALTER TABLE tracks ADD COLUMN rg_album_peak REAL;
             CREATE TABLE lyrics_offsets (path TEXT PRIMARY KEY, offset_ms INTEGER NOT NULL);
             PRAGMA user_version = 5;",
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
        let n: i64 = conn
            .query_row("SELECT count(*) FROM tracks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        // 新表可写。
        record_play_on(&conn, "a.flac", 1, 10.0).unwrap();
        assert_eq!(play_stats_on(&conn, 0, 0, 0, 10).unwrap().total, 1);
    }
}
