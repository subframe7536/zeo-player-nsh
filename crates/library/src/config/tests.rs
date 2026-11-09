use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Mutex, RwLock};

/// 造一条只带路径 / 大小 / 修改时间的曲目。
fn entry(path: &str, size: u64) -> TrackEntry {
    TrackEntry(Arc::new(TrackShared {
        provider: ProviderId::Local,
        path: path.to_owned(),
        file_name: std::path::Path::new(path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        album_artist: String::new(),
        genres: Vec::new(),
        size_bytes: size,
        modified: 0,
        stamp: 0,
        meta: Arc::new(zeo_metadata::MetaInfo::default()),
    }))
}

fn test_store(dir: PathBuf, roots: Vec<String>, tracks: Vec<TrackEntry>) -> LibraryStore {
    LibraryStore {
        dir,
        config: Mutex::new(Arc::new(LibraryConfig {
            roots,
            ..Default::default()
        })),
        tracks: RwLock::new(Arc::new(tracks)),
        dirs: RwLock::new(Arc::new(Vec::new())),
        scanning: AtomicBool::new(false),
        done: AtomicUsize::new(0),
        total: AtomicUsize::new(0),
        current: Mutex::new(String::new()),
    }
}

#[test]
fn move_path_rekeys_index_and_roots() {
    let sep = std::path::MAIN_SEPARATOR;
    let base = std::env::temp_dir().join(format!("zeo-move-path-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let sub = base.join("music/sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("a.flac"), b"x").unwrap();
    let music = base.join("music").to_string_lossy().into_owned();
    let renamed = base.join("renamed").to_string_lossy().into_owned();
    let old_sub = format!("{music}{sep}sub");
    let new_sub = format!("{music}{sep}renamed");
    let store = test_store(
        base.clone(),
        vec![music.clone()],
        vec![entry(&format!("{old_sub}{sep}a.flac"), 1)],
    );
    // 文件夹已移走：索引换前缀，不在其下的根不动，指纹按新路径重算。
    std::fs::rename(&old_sub, &new_sub).unwrap();
    store.move_path(&old_sub, &new_sub);
    assert_eq!(store.tracks()[0].path, format!("{new_sub}{sep}a.flac"));
    assert_eq!(store.roots(), vec![music.clone()]);
    assert_ne!(store.tracks()[0].stamp, 0, "指纹按新路径重算");
    // 根本身被改名：根配置跟着换。
    std::fs::rename(&music, &renamed).unwrap();
    store.move_path(&music, &renamed);
    assert_eq!(store.roots(), vec![renamed.clone()]);
    assert_eq!(
        store.tracks()[0].path,
        format!("{renamed}{sep}renamed{sep}a.flac")
    );
    // 摘除：索引清掉。
    store.remove_under(&format!("{renamed}{sep}renamed"));
    assert!(store.tracks().is_empty());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn genre_split() {
    let raw = vec!["Rock; Indie; Dream Pop".to_owned(), " Rock ".to_owned()];
    assert_eq!(
        split_genres(&raw, &[";".into()]),
        vec!["Rock", "Indie", "Dream Pop"]
    );
}

#[test]
fn artist_split() {
    let raw = "Jay Chou; Mayday / Eason Chan";
    let seps = vec![";".to_owned(), "/".to_owned()];
    assert_eq!(
        split_artists(raw, &seps),
        vec!["Jay Chou", "Mayday", "Eason Chan"]
    );
    assert_eq!(split_artists("  ;;  ", &[";".into()]), Vec::<String>::new());
    assert_eq!(split_artists("Solo", &[]), vec!["Solo"]);
}

#[test]
fn genre_split_drops_empty() {
    let raw = vec![";;".to_owned(), " ; Jazz ;".to_owned()];
    assert_eq!(split_genres(&raw, &[";".into()]), vec!["Jazz"]);
}

#[test]
fn genre_split_several_separators() {
    let raw = vec!["J-Pop / Anime, Game".to_owned()];
    let seps = vec!["/".to_owned(), ",".to_owned()];
    assert_eq!(split_genres(&raw, &seps), vec!["J-Pop", "Anime", "Game"]);
    assert_eq!(
        split_genres(&raw, &[]),
        vec!["J-Pop / Anime, Game"],
        "没有分隔符时整段保留"
    );
}

#[test]
fn old_config_migrates() {
    let old: LibraryConfig =
        serde_json::from_str(r#"{"roots":["/m"],"genre_separator":"/"}"#).unwrap();
    let config = old.normalized();
    assert_eq!(config.genre_separators, vec!["/"]);
    let json = serde_json::to_string(&config).unwrap();
    assert!(
        !json.contains("genre_separator\""),
        "旧字段不再写出：{json}"
    );
    let fresh: LibraryConfig = serde_json::from_str(r#"{"roots":[]}"#).unwrap();
    assert_eq!(
        fresh.theme_color,
        ThemeColor::Cover,
        "缺主题色字段时出厂为跟随封面"
    );
    assert_eq!(
        fresh.sidebar_section,
        SidebarSection::Playlists,
        "缺侧栏视图字段时出厂为歌单"
    );
    assert_eq!(
        fresh.normalized().genre_separators,
        vec![";"],
        "缺字段用出厂值"
    );
    let none: LibraryConfig =
        serde_json::from_str(r#"{"roots":[],"genre_separators":[]}"#).unwrap();
    assert!(
        none.normalized().genre_separators.is_empty(),
        "显式清空时不拆分"
    );
}

#[test]
fn theme_color_serde_roundtrip() {
    assert_eq!(
        serde_json::from_str::<ThemeColor>(r#""neutral""#).unwrap(),
        ThemeColor::Neutral
    );
    assert_eq!(
        serde_json::from_str::<ThemeColor>(r#""cover""#).unwrap(),
        ThemeColor::Cover
    );
    assert_eq!(
        serde_json::from_str::<ThemeColor>(r#""red""#).unwrap(),
        ThemeColor::Custom(0xe5263f)
    );
    assert_eq!(
        serde_json::from_str::<ThemeColor>(r#""blue""#).unwrap(),
        ThemeColor::Custom(0x2563eb)
    );
    assert_eq!(
        serde_json::from_str::<ThemeColor>(r##""#123456""##).unwrap(),
        ThemeColor::Custom(0x123456)
    );
    assert_eq!(
        serde_json::from_str::<ThemeColor>(r#""123456""#).unwrap(),
        ThemeColor::Custom(0x123456)
    );
    assert_eq!(
        serde_json::to_string(&ThemeColor::Neutral).unwrap(),
        r#""neutral""#
    );
    assert_eq!(
        serde_json::to_string(&ThemeColor::Cover).unwrap(),
        r#""cover""#
    );
    assert_eq!(
        serde_json::to_string(&ThemeColor::Custom(0xe5263f)).unwrap(),
        r#""red""#
    );
    assert_eq!(
        serde_json::to_string(&ThemeColor::Custom(0x123456)).unwrap(),
        r##""#123456""##
    );
}

#[test]
fn sidebar_section_serde_roundtrip() {
    assert_eq!(
        serde_json::from_str::<SidebarSection>(r#""playlists""#).unwrap(),
        SidebarSection::Playlists
    );
    assert_eq!(
        serde_json::from_str::<SidebarSection>(r#""tags""#).unwrap(),
        SidebarSection::Tags
    );
    assert_eq!(
        serde_json::from_str::<SidebarSection>(r#""folders""#).unwrap(),
        SidebarSection::Folders
    );
    assert_eq!(
        serde_json::to_string(&SidebarSection::Playlists).unwrap(),
        r#""playlists""#
    );
    assert_eq!(
        serde_json::to_string(&SidebarSection::Tags).unwrap(),
        r#""tags""#
    );
    assert_eq!(
        serde_json::to_string(&SidebarSection::Folders).unwrap(),
        r#""folders""#
    );
}

/// 本地天数 / 小时 / 周起点：东八区（+8h）与负偏移都验。
#[test]
fn local_time_math() {
    const CST: i64 = 8 * 3600;
    // 2026-11-04 00:00:00+08:00 = 2026-11-03T16:00:00Z（日序号 20760）。
    let utc = |h: u64, m: u64| 1_793_664_000 + h * 3600 + m * 60;
    // UTC 16:00 → 本地 11-04 00:00。
    assert_eq!(local_hour(utc(16, 0), CST), 0);
    assert_eq!(local_hour(utc(17, 30), CST), 1);
    // UTC 15:59:59 还在前一天。
    assert_eq!(local_hour(utc(15, 59), CST), 23);
    // 11-04 是周三：本周起点 = 11-02 周一 00:00+08:00 = UTC 11-01 16:00。
    let monday = local_week_start(utc(16, 0), CST);
    assert_eq!(local_day(monday, CST) - local_day(utc(16, 0), CST), -2);
    assert_eq!(local_hour(monday, CST), 0, "周起点落在本地周一 00:00");
    // 周日（11-08 本地 12:00）仍归本周一。
    let sunday = monday + 5 * 86400 + 12 * 3600;
    assert_eq!(local_week_start(sunday, CST), monday);
    // 下周一零点后起点前移。
    let next = monday + 7 * 86400;
    assert_eq!(local_week_start(next, CST), next);
    // 负偏移（UTC-5）：本地时间比 UTC 慢。
    assert_eq!(local_hour(utc(16, 0), -5 * 3600), 11);
    let m = local_week_start(utc(16, 0), -5 * 3600);
    assert_eq!(local_hour(m, -5 * 3600), 0);
}

/// 本地日期时间格式化：跨日（+8h）、负偏移、纪元与闰日边界。
#[test]
fn format_local_datetime_math() {
    // 2026-11-03T16:00:00Z +8h → 次日 00:00。
    assert_eq!(format_local_at(1_793_721_600, 8 * 3600), "2026-11-04 00:00");
    // 同一时刻 UTC-5 → 当天上午 11:00。
    assert_eq!(
        format_local_at(1_793_721_600, -5 * 3600),
        "2026-11-03 11:00"
    );
    // 纪元零秒；负偏移回前一天。
    assert_eq!(format_local_at(0, 0), "1970-01-01 00:00");
    assert_eq!(format_local_at(0, -3600), "1969-12-31 23:00");
    // 闰日：2024-02-29T12:34:00Z。
    assert_eq!(format_local_at(1_709_210_040, 0), "2024-02-29 12:34");
}
