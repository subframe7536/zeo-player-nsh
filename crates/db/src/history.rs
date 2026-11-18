use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::conn::with;

/// 保留最近多少个写回批次（更早的连同快照一起删）。封面快照可能不小，
/// 所以不留无限历史。
pub const KEEP_BATCHES: usize = 30;

/// 一个文件在某次写回之前的标签快照：只含这次动过的字段的旧值（空串 =
/// 原本没有）；封面只在这次动过封面时才存。字段键是调用方自定的稳定字符串
/// （库不认 `TagField`）。
#[derive(Debug, Clone, PartialEq)]
pub struct TagSnapshot {
    pub path: String,
    pub fields: Vec<(String, String)>,
    /// `None` = 这次没动封面；`Some(None)` = 动了、原本没有内嵌封面；
    /// `Some(Some((字节, MIME)))` = 动了、原来的内嵌封面。
    pub cover: Option<Option<(Vec<u8>, String)>>,
}

/// 一个写回批次（历史列表的一行）。
#[derive(Debug, Clone, PartialEq)]
pub struct TagBatch {
    pub id: i64,
    /// Unix 秒。
    pub created_at: u64,
    pub summary: String,
    /// 批次里每个文件的路径（算「涉及所选几首」用）。
    pub paths: Vec<String>,
}

pub(crate) fn add_tag_batch_on(
    conn: &mut Connection,
    summary: &str,
    created_at: u64,
    snaps: &[TagSnapshot],
) -> rusqlite::Result<i64> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO tag_batches (created_at, summary) VALUES (?1, ?2)",
        params![created_at as i64, summary],
    )?;
    let batch = tx.last_insert_rowid();
    {
        let mut insert = tx.prepare(
            "INSERT INTO tag_snapshots (batch, path, fields, cover_touched, cover, cover_mime) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for snap in snaps {
            let fields = serde_json::to_string(&snap.fields).unwrap_or_else(|_| "[]".to_owned());
            let (touched, cover, mime) = match &snap.cover {
                None => (0, None, None),
                Some(None) => (1, None, None),
                Some(Some((bytes, mime))) => (1, Some(bytes.as_slice()), Some(mime.as_str())),
            };
            insert.execute(params![batch, snap.path, fields, touched, cover, mime])?;
        }
    }
    // 只留最近 KEEP_BATCHES 个批次。
    tx.execute(
        "DELETE FROM tag_snapshots WHERE batch NOT IN \
         (SELECT id FROM tag_batches ORDER BY id DESC LIMIT ?1)",
        [KEEP_BATCHES as i64],
    )?;
    tx.execute(
        "DELETE FROM tag_batches WHERE id NOT IN \
         (SELECT id FROM tag_batches ORDER BY id DESC LIMIT ?1)",
        [KEEP_BATCHES as i64],
    )?;
    tx.commit()?;
    Ok(batch)
}

pub(crate) fn tag_batches_on(conn: &Connection) -> rusqlite::Result<Vec<TagBatch>> {
    // 文件都没了（`remove_meta` 清掉了快照）的空批次顺手清掉。
    conn.execute(
        "DELETE FROM tag_batches WHERE id NOT IN (SELECT DISTINCT batch FROM tag_snapshots)",
        [],
    )?;
    let mut batches: Vec<TagBatch> = conn
        .prepare("SELECT id, created_at, summary FROM tag_batches ORDER BY id DESC")?
        .query_map([], |r| {
            Ok(TagBatch {
                id: r.get(0)?,
                created_at: r.get::<_, i64>(1)? as u64,
                summary: r.get(2)?,
                paths: Vec::new(),
            })
        })?
        .collect::<Result<_, _>>()?;
    let mut paths = conn.prepare("SELECT path FROM tag_snapshots WHERE batch = ?1")?;
    for batch in &mut batches {
        batch.paths = paths
            .query_map([batch.id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
    }
    Ok(batches)
}

pub(crate) fn tag_snapshots_on(
    conn: &Connection,
    batch: i64,
    only: &HashSet<String>,
) -> rusqlite::Result<Vec<TagSnapshot>> {
    let ids: Vec<(i64, String)> = conn
        .prepare("SELECT id, path FROM tag_snapshots WHERE batch = ?1 ORDER BY id")?
        .query_map([batch], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, path) in ids.into_iter().filter(|(_, p)| only.contains(p)) {
        let row = conn
            .query_row(
                "SELECT fields, cover_touched, cover, cover_mime FROM tag_snapshots WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Option<Vec<u8>>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((fields, touched, cover, mime)) = row else {
            continue;
        };
        out.push(TagSnapshot {
            path,
            fields: serde_json::from_str(&fields).unwrap_or_default(),
            cover: (touched != 0).then(|| cover.map(|bytes| (bytes, mime.unwrap_or_default()))),
        });
    }
    Ok(out)
}

/// 记一个写回批次（`created_at` 为 Unix 秒）；返回批次号。没有快照不记。
pub fn add_tag_batch(summary: &str, created_at: u64, snaps: &[TagSnapshot]) -> Option<i64> {
    if snaps.is_empty() {
        return None;
    }
    with("写标签历史", |conn| {
        add_tag_batch_on(conn, summary, created_at, snaps)
    })
}

/// 最近的写回批次（新的在前，带各自涉及的路径）。
pub fn tag_batches() -> Vec<TagBatch> {
    with("读标签历史", |conn| tag_batches_on(conn)).unwrap_or_default()
}

/// 批次里属于 `only` 的文件的快照（还原用）。
pub fn tag_snapshots(batch: i64, only: &HashSet<String>) -> Vec<TagSnapshot> {
    with("读标签快照", |conn| {
        tag_snapshots_on(conn, batch, only)
    })
    .unwrap_or_default()
}
