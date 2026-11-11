use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::conn::{json, list, with};

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
