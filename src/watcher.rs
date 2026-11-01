//! 文件夹监视：打开后监视所有根目录（macOS 为 FSEvents），音频文件或
//! 文件夹有增删改名时，等变化停下 [`QUIET`] 再扫描一次。扫描按修改时间
//! 复用标签缓存，只有变了的文件会重新读取。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::library::{AUDIO_EXTENSIONS, library_store};

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
fn events() -> &'static Mutex<Sender<usize>> {
    static TX: OnceLock<Mutex<Sender<usize>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = channel::<usize>();
        std::thread::Builder::new()
            .name("zeo-player-watch".into())
            .spawn(move || {
                loop {
                    // 等第一批变化。
                    let Ok(mut changes) = rx.recv() else { return };
                    let mut last = Instant::now();
                    // 收集到变化停下来为止。
                    loop {
                        match rx.recv_timeout(QUIET.saturating_sub(last.elapsed())) {
                            Ok(n) => {
                                changes += n;
                                last = Instant::now();
                            }
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    // 正在扫描（手动或上一次）：等它结束，再扫一次，覆盖期间的变化。
                    while library_store().is_scanning() {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    if !library_store().watching() {
                        continue;
                    }
                    library_store().scan();
                    let mut st = state().lock().unwrap();
                    st.status.last_rescan = Some(crate::library::unix_now());
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
    let n = event.paths.iter().filter(|p| relevant(p)).count();
    if n > 0 {
        let _ = events().lock().unwrap().send(n);
    }
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
        // 关闭后不再扫描
        lib.set_watch(false);
        assert_eq!(status().roots, 0);
        std::fs::copy(&sample, root.join("three.flac")).unwrap();
        std::thread::sleep(QUIET + Duration::from_secs(2));
        assert_eq!(lib.tracks().len(), 1, "关闭后不扫描");
    }
}
