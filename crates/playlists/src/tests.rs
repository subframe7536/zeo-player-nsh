use super::*;
use std::path::Path;

use zeo_types::config::TrackEntry;

fn track(
    path: &str,
    artist: &str,
    genres: &[&str],
    year: Option<u32>,
    duration: f64,
) -> TrackEntry {
    TrackEntry(std::sync::Arc::new(zeo_types::config::TrackShared {
        provider: zeo_types::config::ProviderId::Local,
        path: path.to_owned(),
        file_name: Path::new(path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        album_artist: String::new(),
        genres: genres.iter().map(|g| (*g).to_owned()).collect(),
        size_bytes: 8_000_000,
        modified: 1_000_000,
        stamp: 0,
        meta: std::sync::Arc::new(zeo_types::meta::MetaInfo {
            title: Path::new(path)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            artist: artist.to_owned(),
            album: "Album".to_owned(),
            year,
            duration,
            bit_rate: Some(320),
            sample_rate: Some(44_100),
            channels: Some(2),
            ..Default::default()
        }),
    }))
}

fn library() -> Vec<TrackEntry> {
    vec![
        track(
            "/m/rock/a.flac",
            "Alpha",
            &["Rock", "Indie"],
            Some(1999),
            200.0,
        ),
        track("/m/pop/b.mp3", "Beta", &["Pop"], Some(2015), 185.0),
        track("/m/rock/c.mp3", "Alpha; Gamma", &["Rock"], None, 400.0),
    ]
}

fn cond(field: Field, op: Op, value: &str) -> Condition {
    Condition {
        field,
        op,
        value: value.to_owned(),
    }
}

fn names(tracks: &[TrackEntry]) -> Vec<&str> {
    tracks.iter().map(|t| t.title.as_str()).collect()
}

#[test]
fn text_conditions_ignore_case_and_cover_every_tag() {
    let lib = library();
    let rule = |c: Condition| Rule {
        conditions: vec![c],
        ..Rule::default()
    };
    assert_eq!(
        names(&rule(cond(Field::Artist, Op::Contains, "alpha")).select(&lib, 0)),
        ["a", "c"]
    );
    assert_eq!(
        names(&rule(cond(Field::Tag, Op::Is, "indie")).select(&lib, 0)),
        ["a"]
    );
    assert_eq!(
        names(&rule(cond(Field::Tag, Op::IsNot, "rock")).select(&lib, 0)),
        ["b"]
    );
    assert_eq!(
        names(&rule(cond(Field::Format, Op::Is, "MP3")).select(&lib, 0)),
        ["b", "c"]
    );
    assert_eq!(
        names(&rule(cond(Field::Folder, Op::EndsWith, "/rock")).select(&lib, 0)),
        ["a", "c"]
    );
    // 值为空的条件不参与：匹配整个曲库。
    assert_eq!(
        rule(cond(Field::Title, Op::Contains, "  "))
            .select(&lib, 0)
            .len(),
        3
    );
}

#[test]
fn number_and_date_conditions_use_field_units() {
    let lib = library();
    let rule = |c: Condition| Rule {
        conditions: vec![c],
        ..Rule::default()
    };
    assert_eq!(
        names(&rule(cond(Field::Duration, Op::Greater, "5:00")).select(&lib, 0)),
        ["c"]
    );
    assert_eq!(
        names(&rule(cond(Field::Duration, Op::Is, "3:05")).select(&lib, 0)),
        ["b"]
    );
    assert_eq!(
        names(&rule(cond(Field::Year, Op::Less, "2000")).select(&lib, 0)),
        ["a"],
        "没有年份的不算"
    );
    assert_eq!(
        rule(cond(Field::SampleRate, Op::Is, "44.1"))
            .select(&lib, 0)
            .len(),
        3
    );
    assert_eq!(
        rule(cond(Field::Size, Op::Greater, "7.5"))
            .select(&lib, 0)
            .len(),
        3
    );
    assert!(parse_number(Field::Duration, "1:02:03") == Some(3723.0));
    let now = 1_000_000 + 3 * 86_400;
    assert_eq!(
        rule(cond(Field::Modified, Op::InLast, "7"))
            .select(&lib, now)
            .len(),
        3
    );
    assert_eq!(
        rule(cond(Field::Modified, Op::InLast, "2"))
            .select(&lib, now)
            .len(),
        0
    );
    assert_eq!(
        rule(cond(Field::Modified, Op::NotInLast, "2"))
            .select(&lib, now)
            .len(),
        3
    );
}

#[test]
fn all_or_any_then_sort_and_limit() {
    let lib = library();
    let conditions = vec![
        cond(Field::Tag, Op::Is, "rock"),
        cond(Field::Format, Op::Is, "mp3"),
    ];
    let all = Rule {
        conditions: conditions.clone(),
        ..Rule::default()
    };
    assert_eq!(names(&all.select(&lib, 0)), ["c"]);
    let any = Rule {
        match_all: false,
        conditions,
        sort: Sort::DurationDesc,
        limit: Some(2),
    };
    assert_eq!(names(&any.select(&lib, 0)), ["c", "a"]);
}

#[test]
fn manual_playlists_keep_order_and_skip_missing_files() {
    let lib = library();
    let list = Playlist {
        id: 1,
        name: "x".into(),
        kind: Kind::Manual {
            tracks: vec![
                "/m/rock/c.mp3".into(),
                "/gone.mp3".into(),
                "/m/rock/a.flac".into(),
            ],
        },
        favorite: false,
    };
    assert_eq!(names(&list.tracks(&lib)), ["c", "a"]);
}

#[test]
fn saves_and_loads_both_kinds() {
    let dir = std::env::temp_dir().join(format!("zeo-player-playlists-{}", std::process::id()));
    let path = dir.join("playlists.json");
    let lists = vec![
        Playlist {
            id: 1,
            name: "手动".into(),
            kind: Kind::Manual {
                tracks: vec!["/a.mp3".into()],
            },
            favorite: false,
        },
        Playlist {
            id: 2,
            name: "智能".into(),
            kind: Kind::Smart {
                rule: Rule {
                    match_all: false,
                    conditions: vec![cond(Field::BitRate, Op::Greater, "256")],
                    sort: Sort::YearDesc,
                    limit: Some(25),
                },
            },
            favorite: false,
        },
    ];
    save_to(&path, &lists).unwrap();
    assert_eq!(load_from(&path), lists);
    assert_eq!(next_id(&lists), 3);
    assert!(load_from(&dir.join("missing.json")).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn genre_field_reads_as_tag() {
    // 旧版文件里字段叫 "genre"，读进来是标签；写回去仍是 "genre"。
    let c: Condition =
        serde_json::from_str(r#"{"field":"genre","op":"is","value":"rock"}"#).unwrap();
    assert_eq!(c.field, Field::Tag);
    assert!(
        serde_json::to_string(&c)
            .unwrap()
            .contains(r#""field":"genre""#)
    );
}

#[test]
fn favorites_are_created_once_and_kept_first() {
    let manual = |id: u64, favorite: bool| Playlist {
        id,
        name: format!("p{id}"),
        kind: Kind::Manual { tracks: Vec::new() },
        favorite,
    };
    // 旧文件：没有「我喜欢」，新建一个，id 不与现有的冲突。
    let mut lists = vec![manual(1, false), manual(4, false)];
    ensure_favorites(&mut lists);
    assert!(lists[0].favorite && lists[0].name == FAVORITES_NAME && lists[0].id == 5);
    assert_eq!(lists.iter().filter(|p| p.favorite).count(), 1);
    // 已有的移到最前；多出来的降为普通歌单。
    let mut lists = vec![manual(1, false), manual(2, true), manual(3, true)];
    ensure_favorites(&mut lists);
    assert_eq!(
        lists.iter().map(|p| (p.id, p.favorite)).collect::<Vec<_>>(),
        [(2, true), (1, false), (3, false)]
    );
    // 旧文件里没有 favorite 字段也能读；普通歌单不写这个字段。
    let old: Playlist =
        serde_json::from_str(r#"{"id":1,"name":"x","kind":"manual","tracks":[]}"#).unwrap();
    assert!(!old.favorite);
    assert!(!serde_json::to_string(&old).unwrap().contains("favorite"));
}
