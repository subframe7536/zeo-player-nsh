use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, OptionalExtension, params};

use crate::conn::with;

// --- 每曲歌词偏移 -------------------------------------------------------------

pub(crate) fn lyrics_offset_on(conn: &Connection, path: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT offset_ms FROM lyrics_offsets WHERE path = ?1",
        [path],
        |r| r.get(0),
    )
    .optional()
    .map(|v| v.unwrap_or(0))
}

pub(crate) fn set_lyrics_offset_on(
    conn: &Connection,
    path: &str,
    offset_ms: i64,
) -> rusqlite::Result<()> {
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

pub(crate) fn record_play_on(
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
pub(crate) fn streak_days(days: &[i64], today: i64) -> u64 {
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

pub(crate) fn play_stats_on(
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
    let today = zeo_types::config::local_day(zeo_types::config::unix_now(), offset);
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
