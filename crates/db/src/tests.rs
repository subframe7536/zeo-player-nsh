use crate::conn::{SCHEMA_V1, SCHEMA_VERSION, migrate};
use crate::meta::*;
use crate::stats::*;
use rusqlite::{Connection, params};
use std::sync::Arc;
use zeo_types::meta::{CacheEntry, MetaInfo, ReplayGain};

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

/// 扩展元数据字段（conductor, lyricist, publisher, copyright, comment, rating, lyrics）写入读回。
#[test]
fn extended_meta_db_roundtrip() {
    let mut conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    let meta = MetaInfo {
        conductor: Some("卡拉扬".into()),
        lyricist: Some("林夕".into()),
        publisher: Some("Sony Music".into()),
        copyright: Some("© 2026".into()),
        comment: Some("经典版本".into()),
        rating: Some(5),
        lyrics: Some("[00:01.00]歌词".into()),
        ..Default::default()
    };
    let entry = CacheEntry {
        stamp: 10,
        size: 2048,
        modified: 100,
        value: Arc::new(meta.clone()),
    };
    save_meta_to(&mut conn, &[("track.flac".to_owned(), entry)]).unwrap();
    let rows = load_meta_from(&conn).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "track.flac");
    assert_eq!(rows[0].1.value.conductor.as_deref(), Some("卡拉扬"));
    assert_eq!(rows[0].1.value.lyricist.as_deref(), Some("林夕"));
    assert_eq!(rows[0].1.value.publisher.as_deref(), Some("Sony Music"));
    assert_eq!(rows[0].1.value.copyright.as_deref(), Some("© 2026"));
    assert_eq!(rows[0].1.value.comment.as_deref(), Some("经典版本"));
    assert_eq!(rows[0].1.value.rating, Some(5));
    assert_eq!(rows[0].1.value.lyrics.as_deref(), Some("[00:01.00]歌词"));
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
