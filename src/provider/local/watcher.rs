//! 文件夹监视：打开后监视所有根目录（macOS 为 FSEvents）。改名 / 移
//! 动配对后只改索引与记录里的路径（不重读标签）；增删内容等变化停下
//! [`QUIET`] 再扫描一次，扫描按修改时间复用标签缓存，只有变了的文件
//! 会重新读取。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::AUDIO_EXTENSIONS;
use crate::config::{LibraryStore, TrackEntry, library_store};

/// 最后一次变化后等多久再扫描。
pub const QUIET: Duration = Duration::from_millis(1500);

/// 监视状态（给设置页显示）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WatchStatus {
    /// 正在监视的根目录数。
    pub roots: usize,
    /// 上一次自动扫描的时间（Unix 秒）。
    pub last_rescan: Option<u64>,
    /// 上一次自动扫描前检测到的变化数。
    pub last_changes: usize,
    /// 监视失败的原因。
    pub error: Option<String>,
}

struct State {
    watcher: Option<RecommendedWatcher>,
    roots: Vec<String>,
    status: WatchStatus,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(State {
            watcher: None,
            roots: Vec::new(),
            status: WatchStatus::default(),
        })
    })
}

/// 变化事件送往去抖线程。
fn events() -> &'static Mutex<Sender<notify::Event>> {
    static TX: OnceLock<Mutex<Sender<notify::Event>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = channel::<notify::Event>();
        std::thread::Builder::new()
            .name("zeo-player-watch".into())
            .spawn(move || {
                loop {
                    // 等第一批变化。
                    let Ok(first) = rx.recv() else { return };
                    let mut batch = vec![first];
                    let mut last = Instant::now();
                    // 收集到变化停下来为止。
                    loop {
                        match rx.recv_timeout(QUIET.saturating_sub(last.elapsed())) {
                            Ok(event) => {
                                batch.push(event);
                                last = Instant::now();
                            }
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    // 正在扫描（手动或上一次）：等它结束再处理，覆盖期间的变化。
                    let lib = library_store();
                    while lib.is_scanning() {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    if !lib.watching() {
                        continue;
                    }
                    let changes = reconcile(&batch, lib);
                    let mut st = state().lock().unwrap();
                    st.status.last_rescan = Some(crate::config::unix_now());
                    st.status.last_changes = changes;
                }
            })
            .expect("spawn watcher thread");
        Mutex::new(tx)
    })
}

/// 这个路径的变化需要重新扫描吗：音频文件，或没有扩展名的路径（文件夹
/// 的增删改名）；隐藏文件（`.DS_Store`、临时文件）不算。
pub fn relevant(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.') {
        return false;
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => AUDIO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()),
        None => true,
    }
}

fn on_event(result: notify::Result<notify::Event>) {
    let Ok(event) = result else { return };
    if matches!(event.kind, EventKind::Access(_)) {
        return;
    }
    if event.paths.iter().any(|p| relevant(p)) {
        let _ = events().lock().unwrap().send(event);
    }
}

/// `path` 在 `dir` 下面（严格后代）。
fn is_under(path: &str, dir: &str) -> bool {
    let sep = std::path::MAIN_SEPARATOR;
    path.len() > dir.len()
        && path.starts_with(dir)
        && path.as_bytes()[dir.len()] == sep as u8
}

/// `p` 是 `t` 这条曲目移动后的文件吗：大小 + 修改时间一致（rename /
/// mv 不动这两项；ctime、inode 会变不算数）。
fn same_track_file(t: &TrackEntry, p: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    meta.is_file()
        && meta.len() == t.size_bytes
        && meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .is_some_and(|d| d.as_secs() == t.modified)
}

/// 给已不存在的 `from` 在 `presents` 里找移动目标：`from` 是已索引文
/// 件时按 (大小, 修改时间) 验同一个文件；是已索引目录的祖先时，其全
/// 部子项都要按相对位置在候选目录下指纹一致。唯一命中才算配成（多
/// 个同指纹的候选说明分不出，留给重扫）。已配走的目标不再配。
fn find_dest(
    from: &str,
    presents: &[String],
    taken: &[(String, String)],
    tracks: &[TrackEntry],
) -> Option<String> {
    let free = |p: &&String| {
        !taken
            .iter()
            .any(|(_, t)| p.as_str() == t || is_under(p, t))
    };
    if let Some(t) = tracks.iter().find(|t| t.path == from) {
        let mut hit = presents
            .iter()
            .filter(|p| free(p) && same_track_file(t, Path::new(p)));
        let only = hit.next()?;
        return hit.next().is_none().then(|| only.clone());
    }
    let sep = std::path::MAIN_SEPARATOR;
    let under = format!("{from}{sep}");
    let children: Vec<&TrackEntry> = tracks
        .iter()
        .filter(|t| t.path.starts_with(&under))
        .collect();
    if children.is_empty() {
        return None;
    }
    let mut hit = presents.iter().filter(|p| {
        free(p)
            && Path::new(p).is_dir()
            && children.iter().all(|t| {
                let rel = &t.path[under.len()..];
                same_track_file(t, &Path::new(p).join(rel))
            })
    });
    let only = hit.next()?;
    hit.next().is_none().then(|| only.clone())
}

/// 单路径改名事件的配对：源已不在、目标存在、按索引里的（大小, 修改
/// 时间）验同一文件。返回（配成的对, 有配不上的变化要重扫）。已被
/// 文件夹对覆盖的源 / 目标不再单独配。
fn pair_moves(
    gones: &mut Vec<String>,
    presents: &[String],
    tracks: &[TrackEntry],
) -> (Vec<(String, String)>, bool) {
    gones.sort();
    gones.dedup();
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut dirty = false;
    for from in gones.iter() {
        if pairs.iter().any(|(f, _)| is_under(from, f)) {
            continue;
        }
        match find_dest(from, presents, &pairs, tracks) {
            Some(to) => pairs.push((from.clone(), to)),
            None => dirty = true,
        }
    }
    // 没配走的「到达」路径是新内容（从外面移进来的）：留给重扫。
    if presents
        .iter()
        .any(|p| !pairs.iter().any(|(_, t)| p == t || is_under(p, t)))
    {
        dirty = true;
    }
    (pairs, dirty)
}

/// 处理一批文件变化：改名 / 移动配对的直接换索引与记录里的路径（不
/// 重读标签），其余变化重扫一次；返回相关变化数（状态展示用）。
fn reconcile(events: &[notify::Event], lib: &LibraryStore) -> usize {
    let mut changes = 0usize;
    let mut gones: Vec<String> = Vec::new();
    let mut presents: Vec<String> = Vec::new();
    let mut moves: Vec<(String, String)> = Vec::new();
    let mut dirty = false;
    for event in events {
        let paths: Vec<String> = event
            .paths
            .iter()
            .filter(|p| relevant(p))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if paths.is_empty() {
            continue;
        }
        changes += paths.len();
        match event.kind {
            // OS 成对给的（Linux inotify 的 move cookie）。
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if paths.len() >= 2 => {
                moves.push((paths[0].clone(), paths[1].clone()));
            }
            // Windows 成对拆成两条：From 是旧路径、To 是新路径。
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => gones.extend(paths),
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => presents.extend(paths),
            // macOS FSEvents 无法关联改名的两侧：两侧各发一条 Any，按
            // 「处理时还存不存在」归类，再靠指纹配对。
            EventKind::Modify(ModifyKind::Name(RenameMode::Any)) => {
                for p in paths {
                    if Path::new(&p).exists() {
                        presents.push(p);
                    } else {
                        gones.push(p);
                    }
                }
            }
            _ => dirty = true,
        }
    }
    for (from, to) in moves {
        lib.move_path(&from, &to);
    }
    let (pairs, pair_dirty) = pair_moves(&mut gones, &presents, &lib.tracks());
    dirty |= pair_dirty;
    for (from, to) in pairs {
        lib.move_path(&from, &to);
    }
    if dirty {
        lib.scan();
    }
    changes
}

/// 按当前设置开关监视、更新监视的根目录。
pub fn sync() {
    let lib = library_store();
    let want: Vec<String> = if lib.watching() {
        lib.roots()
    } else {
        Vec::new()
    };
    let mut st = state().lock().unwrap();
    if want.is_empty() {
        st.watcher = None;
        st.roots.clear();
        st.status.roots = 0;
        st.status.error = None;
        return;
    }
    if st.watcher.is_some() && st.roots == want {
        return;
    }
    st.watcher = None;
    let mut watcher = match notify::recommended_watcher(on_event) {
        Ok(w) => w,
        Err(error) => {
            st.status.error = Some(format!("无法监视：{error}"));
            return;
        }
    };
    let mut ok = 0;
    let mut errors = Vec::new();
    for root in &want {
        match watcher.watch(&PathBuf::from(root), RecursiveMode::Recursive) {
            Ok(()) => ok += 1,
            Err(error) => errors.push(format!("{root}：{error}")),
        }
    }
    st.watcher = Some(watcher);
    st.roots = want;
    st.status.roots = ok;
    st.status.error = (!errors.is_empty()).then(|| errors.join("\n"));
}

pub fn status() -> WatchStatus {
    state().lock().unwrap().status.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderId;

    /// 造一条只带路径 / 大小 / 修改时间的曲目（配对只用这三个字段）。
    fn track(path: &str, size: u64, modified: u64) -> TrackEntry {
        TrackEntry {
            provider: ProviderId::Local,
            path: path.to_owned(),
            file_name: Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            album_artist: String::new(),
            genres: Vec::new(),
            year: None,
            track: None,
            disk: None,
            duration: 0.0,
            bit_rate: None,
            sample_rate: None,
            channels: None,
            has_art: false,
            size_bytes: size,
            modified,
            stamp: 0,
        }
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
        let dir =
            std::env::temp_dir().join(format!("zeo-watch-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn is_under_checks_segments() {
        let sep = std::path::MAIN_SEPARATOR;
        assert!(is_under(&format!("{sep}m{sep}a{sep}b"), &format!("{sep}m{sep}a")));
        assert!(!is_under(&format!("{sep}m{sep}ab"), &format!("{sep}m{sep}a")));
        assert!(!is_under(&format!("{sep}m{sep}a"), &format!("{sep}m{sep}a")));
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
            track(&format!("{sep}m{sep}album{sep}sub{sep}b.flac"), 20, 1700000001),
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
        let tracks = vec![track(
            &format!("{from_dir}{sep}a.flac"),
            10,
            1700000000,
        )];
        // 文件夹 + 子项各自一条 Any 事件；外加一条「到达」的新文件。
        let mut gones = vec![
            from_dir.clone(),
            format!("{from_dir}{sep}a.flac"),
        ];
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
                if !lib.is_scanning()
                    && lib.tracks().len() == want
                    && status().last_rescan.is_some()
                {
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
            lib.tracks().iter().any(|t| t.path.ends_with("renamed.flac")),
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
}
