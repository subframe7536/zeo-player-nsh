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
    path.len() > dir.len() && path.starts_with(dir) && path.as_bytes()[dir.len()] == sep as u8
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
    let free = |p: &&String| !taken.iter().any(|(_, t)| p.as_str() == t || is_under(p, t));
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
mod tests;
