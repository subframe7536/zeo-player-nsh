use super::*;
use crate::config::ProviderId;

/// 造一条只带路径 / 大小 / 修改时间的曲目（配对只用这三个字段）。
fn track(path: &str, size: u64, modified: u64) -> TrackEntry {
    TrackEntry(std::sync::Arc::new(crate::config::TrackShared {
        provider: ProviderId::Local,
        path: path.to_owned(),
        file_name: Path::new(path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        album_artist: String::new(),
        genres: Vec::new(),
        size_bytes: size,
        modified,
        stamp: 0,
        meta: std::sync::Arc::new(zeo_metadata::MetaInfo::default()),
    }))
}

/// 落一个已知大小 + 修改时间（秒）的文件，返回其路径字符串。
fn write_file(dir: &Path, rel: &str, size: u64, modified: u64) -> String {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, vec![0u8; size as usize]).unwrap();
    let times = std::fs::FileTimes::new()
        .set_modified(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(modified));
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(times)
        .unwrap();
    path.to_string_lossy().into_owned()
}

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zeo-watch-test-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn is_under_checks_segments() {
    let sep = std::path::MAIN_SEPARATOR;
    assert!(is_under(
        &format!("{sep}m{sep}a{sep}b"),
        &format!("{sep}m{sep}a")
    ));
    assert!(!is_under(
        &format!("{sep}m{sep}ab"),
        &format!("{sep}m{sep}a")
    ));
    assert!(!is_under(
        &format!("{sep}m{sep}a"),
        &format!("{sep}m{sep}a")
    ));
}

#[test]
fn file_move_pairs_by_size_and_mtime() {
    let dir = temp(&line!().to_string());
    let to = write_file(&dir, "b.flac", 1234, 1700000000);
    let tracks = vec![track("/m/a.flac", 1234, 1700000000)];
    assert_eq!(
        find_dest("/m/a.flac", &[to.clone()], &[], &tracks).as_deref(),
        Some(to.as_str())
    );
    // 大小或修改时间对不上 = 不是同一个文件。
    let tracks = vec![track("/m/a.flac", 1234, 1)];
    assert_eq!(find_dest("/m/a.flac", &[to.clone()], &[], &tracks), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ambiguous_candidates_do_not_pair() {
    let dir = temp(&line!().to_string());
    // 两个目标指纹一致时分不出哪个是源，留给重扫。
    let a = write_file(&dir, "b.flac", 99, 1700000000);
    let b = write_file(&dir, "c.flac", 99, 1700000000);
    let tracks = vec![track("/m/a.flac", 99, 1700000000)];
    assert_eq!(find_dest("/m/a.flac", &[a, b], &[], &tracks), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn folder_move_pairs_when_children_match() {
    let dir = temp(&line!().to_string());
    let to = dir.join("renamed");
    let sep = std::path::MAIN_SEPARATOR;
    write_file(&to, &format!("a.flac"), 10, 1700000000);
    write_file(&to, &format!("sub{sep}b.flac"), 20, 1700000001);
    let to_str = to.to_string_lossy().into_owned();
    let tracks = vec![
        track(&format!("{sep}m{sep}album{sep}a.flac"), 10, 1700000000),
        track(
            &format!("{sep}m{sep}album{sep}sub{sep}b.flac"),
            20,
            1700000001,
        ),
    ];
    assert_eq!(
        find_dest(
            &format!("{sep}m{sep}album"),
            &[to_str.clone()],
            &[],
            &tracks
        )
        .as_deref(),
        Some(to_str.as_str())
    );
    // 少一个子项就配不上。
    let tracks = vec![
        track(&format!("{sep}m{sep}album{sep}a.flac"), 10, 1700000000),
        track(&format!("{sep}m{sep}album{sep}gone.flac"), 30, 2),
    ];
    assert_eq!(
        find_dest(&format!("{sep}m{sep}album"), &[to_str], &[], &tracks),
        None
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pair_moves_covers_subtree_and_reports_leftovers() {
    let dir = temp(&line!().to_string());
    let sep = std::path::MAIN_SEPARATOR;
    let to = dir.join("renamed");
    let to_a = write_file(&to, "a.flac", 10, 1700000000);
    let stray = write_file(&dir, "new.flac", 5, 1700000000);
    let to_str = to.to_string_lossy().into_owned();
    let from_dir = format!("{sep}m{sep}album");
    let tracks = vec![track(&format!("{from_dir}{sep}a.flac"), 10, 1700000000)];
    // 文件夹 + 子项各自一条 Any 事件；外加一条「到达」的新文件。
    let mut gones = vec![from_dir.clone(), format!("{from_dir}{sep}a.flac")];
    let presents = vec![to_str.clone(), to_a, stray.clone()];
    let (pairs, dirty) = pair_moves(&mut gones, &presents, &tracks);
    assert_eq!(pairs, vec![(from_dir, to_str)]);
    assert!(dirty, "没配走的新文件要重扫");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn only_audio_and_folders_trigger() {
    assert!(relevant(Path::new("/m/a.flac")));
    assert!(relevant(Path::new("/m/a.MP3")));
    assert!(relevant(Path::new("/m/新专辑")));
    assert!(!relevant(Path::new("/m/.DS_Store")));
    assert!(!relevant(Path::new("/m/cover.jpg")));
    assert!(!relevant(Path::new("/m/a.lrc")));
    assert!(!relevant(Path::new("/m/.a.flac.tmp")));
}

/// `cargo test -- --ignored watcher_rescans`：在临时根目录里真实地增、改、删文件。
#[test]
#[ignore = "touches the file system and waits for FSEvents"]
fn watcher_rescans_after_changes() {
    let music = dirs::audio_dir().expect("~/Music");
    let Some(sample) = walkdir::WalkDir::new(&music)
        .max_depth(3)
        .into_iter()
        .flatten()
        .map(|e| e.into_path())
        .find(|p| p.extension().is_some_and(|e| e == "flac"))
    else {
        return;
    };
    let root = PathBuf::from("/tmp/zeo-player-watch-root");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // SAFETY(测试): 只有本测试读写这个变量。
    unsafe { std::env::set_var("ZEO_PLAYER_DATA_DIR", "/tmp/zeo-player-test-data-watch") };
    let _ = std::fs::remove_dir_all("/tmp/zeo-player-test-data-watch");
    let lib = library_store();
    lib.add_root(&root.to_string_lossy()).unwrap();
    let root = PathBuf::from(&lib.roots()[0]);
    assert_eq!(lib.scan(), 0);
    lib.set_watch(true);
    assert_eq!(status().roots, 1);
    let wait = |want: usize| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if !lib.is_scanning() && lib.tracks().len() == want && status().last_rescan.is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    };
    // 新增
    std::fs::copy(&sample, root.join("one.flac")).unwrap();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::copy(&sample, root.join("sub/two.flac")).unwrap();
    assert!(wait(2), "新增后自动扫描：{} 首", lib.tracks().len());
    // 内容变化：重写文件，修改时间变 → 重新读取
    // （std::fs::copy 保留源文件的修改时间，正好验证指纹不只看 mtime。）
    let before = lib
        .tracks()
        .iter()
        .find(|t| t.path.ends_with("one.flac"))
        .unwrap()
        .stamp;
    std::fs::copy(&sample, root.join("one.flac")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut after = before;
    while Instant::now() < deadline && after == before {
        std::thread::sleep(Duration::from_millis(100));
        after = lib
            .tracks()
            .iter()
            .find(|t| t.path.ends_with("one.flac"))
            .map_or(before, |t| t.stamp);
    }
    assert_ne!(after, before, "内容变化后重新扫描");
    // 删除文件夹
    std::fs::remove_dir_all(root.join("sub")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && lib.tracks().len() != 1 {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(lib.tracks().len(), 1, "删除后自动扫描");
    // 改名：FSEvents 不关联两侧，按指纹配对后直接换索引路径，
    // 不整库重扫（曲目数不变）。
    std::fs::rename(root.join("one.flac"), root.join("renamed.flac")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline
        && !lib
            .tracks()
            .iter()
            .any(|t| t.path.ends_with("renamed.flac"))
    {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        lib.tracks()
            .iter()
            .any(|t| t.path.ends_with("renamed.flac")),
        "改名后索引直接换路径"
    );
    assert!(
        !lib.tracks().iter().any(|t| t.path.ends_with("one.flac")),
        "旧路径已不在索引"
    );
    // 关闭后不再扫描
    lib.set_watch(false);
    assert_eq!(status().roots, 0);
    std::fs::copy(&sample, root.join("three.flac")).unwrap();
    std::thread::sleep(QUIET + Duration::from_secs(2));
    assert_eq!(lib.tracks().len(), 1, "关闭后不扫描");
}
