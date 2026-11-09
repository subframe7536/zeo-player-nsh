use std::collections::HashSet;
use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension, params};

use zeo_types::meta::{CacheEntry, MetaInfo};

use crate::conn::{COLUMNS, json, list, with};

// --- 曲目元数据 ---------------------------------------------------------------

/// 读出全部元数据（启动时一次，及隐藏丢弃后的 `entries` 整体回填）。
pub fn load_meta() -> Vec<(String, CacheEntry)> {
    with("读取元数据", |conn| load_meta_from(conn)).unwrap_or_default()
}

pub(crate) fn meta_row(r: &rusqlite::Row) -> rusqlite::Result<(String, CacheEntry)> {
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
        replaygain: zeo_types::meta::ReplayGain {
            track_gain: r.get::<_, Option<f64>>(26)?.map(|v| v as f32),
            track_peak: r.get::<_, Option<f64>>(27)?.map(|v| v as f32),
            album_gain: r.get::<_, Option<f64>>(28)?.map(|v| v as f32),
            album_peak: r.get::<_, Option<f64>>(29)?.map(|v| v as f32),
        },
        conductor: r.get(30)?,
        lyricist: r.get(31)?,
        publisher: r.get(32)?,
        copyright: r.get(33)?,
        comment: r.get(34)?,
        rating: r.get(35)?,
        lyrics: r.get(36)?,
    };
    let entry = CacheEntry {
        stamp: r.get::<_, i64>(1)? as u64,
        size: r.get::<_, i64>(2)? as u64,
        modified: r.get::<_, i64>(3)? as u64,
        value: Arc::new(meta),
    };
    Ok((r.get::<_, String>(0)?, entry))
}

pub(crate) fn load_meta_from(conn: &Connection) -> rusqlite::Result<Vec<(String, CacheEntry)>> {
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

pub(crate) fn save_meta_to(
    conn: &mut Connection,
    rows: &[(String, CacheEntry)],
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(&format!(
            "INSERT OR REPLACE INTO tracks ({COLUMNS}) VALUES \
             (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37)"
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
                m.conductor,
                m.lyricist,
                m.publisher,
                m.copyright,
                m.comment,
                m.rating,
                m.lyrics,
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

pub(crate) fn remove_meta_on(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM tracks WHERE path = ?1", [path])?;
    conn.execute("DELETE FROM lyrics_offsets WHERE path = ?1", [path])?;
    conn.execute("DELETE FROM added WHERE path = ?1", [path])?;
    Ok(())
}

pub(crate) fn remove_meta_under_on(conn: &Connection, path: &str) -> rusqlite::Result<()> {
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

pub(crate) fn move_meta_on(
    conn: &Connection,
    rows: &[(String, String, u64, u64)],
) -> rusqlite::Result<()> {
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
