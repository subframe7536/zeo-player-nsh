use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use rusqlite::Connection;

/// 当前表结构版本（`PRAGMA user_version`）。改表时加一，并在 [`migrate`] 里迁移。
pub(crate) const SCHEMA_VERSION: i32 = 8;

pub(crate) const SCHEMA_V1: &str = "
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

pub(crate) const COLUMNS: &str = "path, stamp, size, modified, title, artist, album, album_artist, duration, has_art, \
    bit_rate, bit_depth, sample_rate, channels, codec, quality, genres, year, track, track_total, disk, disk_total, \
    album_artists, composers, art_hue, art_sat, rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak, \
    conductor, lyricist, publisher, copyright, comment, rating, lyrics";

pub(crate) fn db() -> Option<&'static Mutex<Connection>> {
    static DB: OnceLock<Option<Mutex<Connection>>> = OnceLock::new();
    DB.get_or_init(|| {
        // 单元测试不碰真实数据目录（自己设了 ZEO_PLAYER_DATA_DIR 的测试除外）。
        if cfg!(test) && std::env::var_os("ZEO_PLAYER_DATA_DIR").is_none() {
            return None;
        }
        let dir = zeo_types::config::data_dir();
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

pub(crate) fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(2))?;
    // WAL + NORMAL：写入不逐条 fsync，崩溃最多丢最后几次提交，库不会坏。
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    migrate(&conn)?;
    Ok(conn)
}

pub(crate) fn migrate(conn: &Connection) -> rusqlite::Result<()> {
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
    if version < 8 {
        // 指挥、作词、发行商、版权、注释、评分入库，
        // 元数据视图可直读 SQLite；老行 NULL = 缺失。
        conn.execute_batch(
            "ALTER TABLE tracks ADD COLUMN conductor TEXT;
             ALTER TABLE tracks ADD COLUMN lyricist TEXT;
             ALTER TABLE tracks ADD COLUMN publisher TEXT;
             ALTER TABLE tracks ADD COLUMN copyright TEXT;
             ALTER TABLE tracks ADD COLUMN comment TEXT;
             ALTER TABLE tracks ADD COLUMN rating INTEGER;",
        )?;
    }
    if version < SCHEMA_VERSION {
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}

/// 在连接上做 `f`；没有数据库或出错时返回 `None`（错误打到 stderr）。
pub(crate) fn with<T>(
    what: &str,
    f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>,
) -> Option<T> {
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

pub(crate) fn list(raw: String) -> Vec<String> {
    serde_json::from_str(&raw).unwrap_or_default()
}

pub(crate) fn json(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_owned())
}
