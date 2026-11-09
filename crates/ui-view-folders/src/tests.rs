use super::*;
use zeo_playlists::{Field, Op};
use zeo_ui_components::tracks::audio_format;

fn entry(path: &str) -> TrackEntry {
    TrackEntry(std::sync::Arc::new(zeo_types::config::TrackShared {
        provider: zeo_types::config::ProviderId::Local,
        path: path.into(),
        file_name: String::new(),
        album_artist: String::new(),
        genres: Vec::new(),
        size_bytes: 0,
        modified: 0,
        stamp: 0,
        meta: std::sync::Arc::new(zeo_metadata::MetaInfo {
            title: path.rsplit('/').next().unwrap().into(),
            ..Default::default()
        }),
    }))
}

#[test]
fn tree_groups_by_root_and_folder() {
    let tracks = vec![
        entry("/m/a/1.mp3"),
        entry("/m/a/b/2.mp3"),
        entry("/m/3.mp3"),
        entry("/x/4.mp3"),
    ];
    let tree = build_tree_with(&["/m".into()], &tracks, &[], '/');
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].total, 3);
    assert_eq!(tree[0].tracks, vec![2]);
    let a = tree[0].find("/m/a").unwrap();
    assert_eq!(a.total, 2);
    let mut order = Vec::new();
    a.subtree(&mut order);
    assert_eq!(order, vec![1, 0]);
}

#[test]
fn rows_follow_expand_state() {
    let tracks = vec![
        entry("/m/a/1.mp3"),
        entry("/m/a/b/2.mp3"),
        entry("/m/3.mp3"),
    ];
    let tree = build_tree_with(&["/m".into()], &tracks, &[], '/');
    fn paths(rows: &[TreeRow]) -> Vec<&str> {
        rows.iter().map(|r| r.path.as_str()).collect()
    }
    // 全部折叠：只有根。
    assert_eq!(paths(&flatten_with(&tree, &BTreeSet::new())), vec!["/m"]);
    // 根展开：一层子文件夹。
    let open = BTreeSet::from(["/m".to_owned()]);
    assert_eq!(paths(&flatten_with(&tree, &open)), vec!["/m", "/m/a"]);
    // 选中 /m/a/b 时展开祖先：/m 与 /m/a 都展开，子项出现。
    let mut open2 = BTreeSet::new();
    expand_ancestors(&mut open2, "/m/a/b", '/');
    let rows = flatten_with(&tree, &open2);
    assert_eq!(paths(&rows), vec!["/m", "/m/a", "/m/a/b"]);
    assert!(rows[..2].iter().all(|r| r.open));
    assert!(!rows[2].open);
    // 收起父目录 /m/a：子项隐藏，收起成功生效！
    open2.remove("/m/a");
    let rows_collapsed = flatten_with(&tree, &open2);
    assert_eq!(paths(&rows_collapsed), vec!["/m", "/m/a"]);
    assert!(!rows_collapsed[1].open);
}

#[test]
fn place_shows_folder_files_and_subtree() {
    let mut tracks = vec![
        entry("/m/a/1.mp3"),
        entry("/m/a/b/2.mp3"),
        entry("/m/3.mp3"),
    ];
    for (t, size) in tracks.iter_mut().zip([100, 200, 300]) {
        let mut shared = (*t.0).clone();
        shared.size_bytes = size;
        shared.meta = std::sync::Arc::new(zeo_metadata::MetaInfo {
            duration: 60.0,
            ..shared.meta.as_ref().clone()
        });
        *t = TrackEntry(std::sync::Arc::new(shared));
    }
    let tree = build_tree_with(&["/m".into()], &tracks, &[], '/');
    // 没选或选中不存在时都回退到第一个根。
    let m = place_with(&tree, None, '/');
    assert_eq!(m.path.as_deref(), Some("/m"));
    assert_eq!(
        place_with(&tree, Some("/gone"), '/').path.as_deref(),
        Some("/m")
    );
    // 根：files 只是直接曲目；all 是整棵子树；trail 是完整根路径。
    assert_eq!(*m.files, vec![2]);
    assert_eq!(*m.all, vec![1, 0, 2]);
    assert_eq!(m.folders, 1);
    assert_eq!(m.tracks, 3);
    assert_eq!(m.bytes, 600);
    assert_eq!(m.trail, "/m");
    let a = place_with(&tree, Some("/m/a"), '/');
    assert_eq!(*a.files, vec![0]);
    assert_eq!(*a.all, vec![1, 0]);
    assert_eq!(a.trail, "m / a");
    // 多个根：只列根，没选时显示名字靠前的那个。
    let both = vec![entry("/m/1.mp3"), entry("/x/2.mp3")];
    let two = build_tree_with(&["/m".into(), "/x".into()], &both, &[], '/');
    assert_eq!(two.len(), 2);
    assert_eq!(place_with(&two, None, '/').path.as_deref(), Some("/m"));
    assert_eq!(format_bytes(1536 * 1024), "1.5 MB");
    assert_eq!(audio_format("/m/a.flac"), "FLAC");
    assert_eq!(audio_format("C:\\m\\b.Mp3"), "MP3");
}

#[test]
fn drop_target_rejects_noops_and_cycles() {
    // 文件 / 文件夹落到别的文件夹：可以。
    assert!(drop_ok("/m/a/1.mp3", false, "/m/b", '/'));
    assert!(drop_ok("/m/a", true, "/x", '/'));
    // 落到自身或当前所在：不行（等于没动）。
    assert!(!drop_ok("/m/a", true, "/m/a", '/'));
    assert!(!drop_ok("/m/a", true, "/m", '/'));
    assert!(!drop_ok("/m/a/1.mp3", false, "/m/a", '/'));
    // 文件夹不能进自己的子树。
    assert!(!drop_ok("/m/a", true, "/m/a/b", '/'));
    // 子树判断按路径分段比：「/m/abc」不是「/m/ab」的后代。
    assert!(drop_ok("/m/ab", true, "/m/abc", '/'));
    // 尾随分隔符不影响判断。
    assert!(!drop_ok("/m/a/", true, "/m", '/'));
    // Windows 路径同规则。
    assert!(drop_ok("C:\\m\\a", true, "C:\\x", '\\'));
    assert!(!drop_ok("C:\\m\\a", true, "C:\\m\\a\\b", '\\'));
    assert!(!drop_ok("C:\\m\\a", true, "C:\\m", '\\'));
}

#[test]
fn multi_drop_target_validates_all_items() {
    let sources = vec!["/m/a/1.mp3", "/m/a/2.mp3"];
    // 目标为 /m/b：全部合法
    assert!(sources.iter().all(|s| drop_ok(s, false, "/m/b", '/')));
    // 目标为当前所在 /m/a：全不合法
    assert!(!sources.iter().all(|s| drop_ok(s, false, "/m/a", '/')));

    let mixed = vec!["/m/a/1.mp3", "/m/b/2.mp3"];
    // 混合来源落到 /m/b：其中 2.mp3 原本就在 /m/b，整体不合法
    assert!(!mixed.iter().all(|s| drop_ok(s, false, "/m/b", '/')));
}

#[test]
fn move_into_renames_and_reports_collisions() {
    let base = std::env::temp_dir().join(format!("zeo-move-into-test-{}", std::process::id()));
    let src = base.join("src");
    let dst = base.join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    let file = src.join("a.mp3");
    std::fs::write(&file, b"x").unwrap();
    let to = move_into(file.to_str().unwrap(), dst.to_str().unwrap(), '/').unwrap();
    assert!(std::path::Path::new(&to).exists());
    assert!(!file.exists());
    // 目标已有同名：报错且原文件还在。
    let file2 = src.join("a.mp3");
    std::fs::write(&file2, b"y").unwrap();
    assert!(move_into(file2.to_str().unwrap(), dst.to_str().unwrap(), '/').is_err());
    assert!(file2.exists());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn windows_paths_build_the_same_tree() {
    let tracks = vec![
        entry("C:\\m\\a\\1.mp3"),
        entry("C:\\m\\2.mp3"),
        entry("D:\\3.mp3"),
    ];
    let tree = build_tree_with(&["C:\\m".into(), "D:\\".into()], &tracks, &[], '\\');
    // 根按名字排序：「D:」在「m」前 → 默认显示它。
    assert_eq!(
        tree.iter()
            .map(|r| (r.name.as_str(), r.total))
            .collect::<Vec<_>>(),
        vec![("D:", 1), ("m", 2)]
    );
    let top = place_with(&tree, None, '\\');
    assert_eq!(top.path.as_deref(), Some("D:\\"));
    let m = place_with(&tree, Some("C:\\m"), '\\');
    assert_eq!(*m.files, vec![1]);
    assert_eq!(m.trail, "C:\\m");
    let a = place_with(&tree, Some("C:\\m\\a"), '\\');
    assert_eq!(a.trail, "m / a");
    assert_eq!(*a.all, vec![0]);
}

#[test]
fn names_are_validated() {
    // 首尾空白裁掉。
    assert_eq!(valid_name("  新建文件夹  ").unwrap(), "新建文件夹");
    // 空名、分隔符、相对路径都不行。
    for bad in ["", "   ", "a/b", "a\\b", ".", ".."] {
        assert!(valid_name(bad).is_err(), "{bad}");
    }
    assert!(valid_name("a:b").is_ok());
}

#[test]
fn rename_dest_keeps_extension() {
    // 文件夹整名换掉。
    assert_eq!(rename_dest("/m/a/b", true, "c", '/').unwrap(), "/m/a/c");
    // 文件只换主干；写了扩展名也不会重复拼；大小写不同也认。
    assert_eq!(
        rename_dest("/m/a/s.mp3", false, "t", '/').unwrap(),
        "/m/a/t.mp3"
    );
    assert_eq!(
        rename_dest("/m/a/s.mp3", false, "t.mp3", '/').unwrap(),
        "/m/a/t.mp3"
    );
    assert_eq!(
        rename_dest("/m/a/s.MP3", false, "t.mp3", '/').unwrap(),
        "/m/a/t.mp3"
    );
    // 无扩展名与点文件：名字原样用。
    assert_eq!(
        rename_dest("/m/a/s", false, "t.flac", '/').unwrap(),
        "/m/a/t.flac"
    );
    assert_eq!(
        rename_dest("/m/a/.env", false, ".env2", '/').unwrap(),
        "/m/a/.env2"
    );
    // Windows 分隔符。
    assert_eq!(
        rename_dest("C:\\m\\a\\s.mp3", false, "t", '\\').unwrap(),
        "C:\\m\\a\\t.mp3"
    );
    // 不合法的名字原样把错误传出来。
    assert!(rename_dest("/m/a", true, "x/y", '/').is_err());
    // 父目录：`/m` 的父是 `/`，盘符下是 `C:\`。
    assert_eq!(parent_dir("/m", '/').as_deref(), Some("/"));
    assert_eq!(parent_dir("/m/a", '/').as_deref(), Some("/m"));
    assert_eq!(parent_dir("C:\\m\\a", '\\').as_deref(), Some("C:\\m"));
    assert_eq!(parent_dir("C:\\a", '\\').as_deref(), Some("C:\\"));
}

#[test]
fn rename_default_is_stem_for_files() {
    assert_eq!(rename_default("/m/a/b", true), "b");
    assert_eq!(rename_default("/m/a/s.mp3", false), "s");
    assert_eq!(rename_default("/m/a/s", false), "s");
    assert_eq!(rename_default("C:\\m\\s.flac", false), "s");
}

#[test]
fn extra_empty_dirs_join_the_tree() {
    let tracks = vec![entry("/m/a/1.mp3")];
    let extra = vec![
        "/m/b".to_string(),
        "/m/a/deep".to_string(),
        "/outside".to_string(),
    ];
    let tree = build_tree_with(&["/m".into()], &tracks, &extra, '/');
    let kids: Vec<&str> = tree[0].children.values().map(|c| c.name.as_str()).collect();
    // 曲目推不出的 b 挂上了；a 里的 deep 也逐级保证；根外的 /outside 不进来。
    assert_eq!(kids, vec!["a", "b"]);
    let a = &tree[0].children["a\u{0}a"];
    assert_eq!(
        a.children
            .values()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["deep"]
    );
    assert_eq!(tree.len(), 1);
}

#[test]
fn folder_condition_filter_matches() {
    let meta = std::sync::Arc::new(zeo_metadata::MetaInfo {
        title: "My Favorite Song".into(),
        artist: "Awesome Singer".into(),
        album: "Greatest Hits".into(),
        duration: 215.0,
        ..Default::default()
    });
    let t = TrackEntry(std::sync::Arc::new(zeo_types::config::TrackShared {
        provider: zeo_types::config::ProviderId::Local,
        path: "/music/artist/album/song.flac".into(),
        file_name: "song.flac".into(),
        album_artist: "Various Artists".into(),
        genres: vec!["Pop".into(), "Rock".into()],
        size_bytes: 1024,
        modified: 0,
        stamp: 0,
        meta,
    }));

    let c_title = Condition {
        field: Field::Title,
        op: Op::Contains,
        value: "favorite".into(),
    };
    assert!(c_title.matches(&t, 0));

    let c_artist = Condition {
        field: Field::Artist,
        op: Op::Is,
        value: "Awesome Singer".into(),
    };
    assert!(c_artist.matches(&t, 0));

    let c_format = Condition {
        field: Field::Format,
        op: Op::Is,
        value: "flac".into(),
    };
    assert!(c_format.matches(&t, 0));

    let c_genre = Condition {
        field: Field::Tag,
        op: Op::Is,
        value: "Pop".into(),
    };
    assert!(c_genre.matches(&t, 0));

    let c_filename = Condition {
        field: Field::FileName,
        op: Op::Contains,
        value: "song".into(),
    };
    assert!(c_filename.matches(&t, 0));
}
