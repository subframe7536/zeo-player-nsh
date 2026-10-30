//! 界面状态：引擎与曲库的信号（轮询共享状态）、换曲时的加载流水线
//! （元数据 / 歌词 / 波形）、专辑封面缓存，以及播放命令。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nsh_core::prelude::*;

use crate::engine::{LyricsSource, Meta, Player};
use crate::library::{library_store, ScanProgress, TrackEntry};
use crate::lyrics::LyricsOutput;
use crate::metadata::{MetaCache, MetaInfo};

/// 按指针比较的共享值（信号里放大对象，换了才通知）。
#[derive(Debug)]
pub struct Same<T>(pub Arc<T>);

impl<T> Clone for Same<T> {
    fn clone(&self) -> Self {
        Same(self.0.clone())
    }
}

impl<T> PartialEq for Same<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> std::ops::Deref for Same<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

/// 播放进度与开关（≈10 Hz 变化）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Playback {
    pub playing: bool,
    pub loading: bool,
    pub time: f64,
    pub duration: f64,
    pub volume: f64,
}

/// 当前曲目（换曲时变化）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NowPlaying {
    pub path: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Option<SharedString>,
    pub backdrop: Option<SharedString>,
    pub error: Option<String>,
    /// 在播放队列中的位置（判断切歌方向）。
    pub index: i64,
}

/// 曲库概况。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibraryState {
    pub scanning: bool,
    pub roots: Vec<String>,
    pub progress: ScanProgress,
    pub separators: Vec<String>,
    pub watching: bool,
    pub watch: crate::watcher::WatchStatus,
    /// 界面字体（`None`：系统字体）。
    pub font: Option<String>,
    pub close_action: crate::library::CloseAction,
}

fn library_state() -> LibraryState {
    let lib = library_store();
    let progress = lib.progress();
    LibraryState {
        scanning: progress.scanning,
        roots: lib.roots(),
        progress,
        separators: lib.separators(),
        watching: lib.watching(),
        watch: crate::watcher::status(),
        font: lib.font(),
        close_action: lib.close_action(),
    }
}

/// 当前曲目的歌词。
#[derive(Debug, Clone, PartialEq)]
pub struct Lyrics {
    pub output: Same<LyricsOutput>,
    pub source: LyricsSource,
}

/// 全部界面状态（信号句柄，`Copy`）。
#[derive(Clone, Copy)]
pub struct Store {
    pub playback: Signal<Playback>,
    pub track: Signal<NowPlaying>,
    pub library: Signal<LibraryState>,
    pub tracks: Signal<Same<Vec<TrackEntry>>>,
    pub meta: Signal<Option<Same<MetaInfo>>>,
    pub lyrics: Signal<Option<Lyrics>>,
    pub waveform: Signal<Option<Same<Vec<f64>>>>,
    /// 拖动 seek 时的预览位置（秒），抬起后保留到真实进度追上。
    pub seek_preview: Signal<Option<f64>>,
    /// 根组件的 owner（封面信号挂在它下面）。
    owner: Owner,
    /// 静音前的音量。
    last_volume: Signal<f64>,
    /// 当前歌词行（-1：无）。
    pub active_line: Memo<i64>,
    /// 播放队列（路径）。
    pub queue: Signal<Same<Vec<String>>>,
    /// 窗口已隐藏到托盘：界面拆掉，只留播放。
    pub hidden: Signal<bool>,
    /// 点了关闭、等用户选择「托盘 / 退出」。
    pub close_prompt: Signal<bool>,
}

thread_local! {
    static STORE: Cell<Option<Store>> = const { Cell::new(None) };
    /// 已请求封面的曲目上次的文件指纹（内容变了就重新生成）。
    static COVER_STAMPS: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
    static COVERS: RefCell<HashMap<String, Signal<Option<SharedString>>>> = RefCell::new(HashMap::new());
}

/// 界面状态（在 [`install`] 之后可用）。
pub fn store() -> Store {
    STORE.with(|s| s.get()).expect("store installed by the root component")
}

pub fn player() -> &'static Player {
    Player::global()
}

/// 建立状态并启动轮询；在根组件里调用一次。
pub fn install() -> Store {
    // 导入的字体先注册，首帧文字就能用上。
    crate::fonts::register_imported();
    let snap = player().state();
    let lib = library_store();
    let playback = signal(Playback {
        playing: snap.is_playing,
        loading: snap.is_loading,
        time: snap.current_time,
        duration: snap.duration,
        volume: snap.volume,
    });
    let track = signal(now_playing(&snap));
    let library = signal(library_state());
    let tracks = signal(Same(lib.tracks()));
    let lyrics = signal(None::<Lyrics>);
    let active_line = computed(move || {
        let time = playback.with(|p| p.time);
        lyrics.with(|l| match l {
            Some(l) if l.output.timed => active_index(&l.output, time),
            _ => -1,
        })
    });
    let s = Store {
        playback,
        track,
        library,
        tracks,
        meta: signal(None),
        lyrics,
        waveform: signal(None),
        seek_preview: signal(None),
        owner: Owner::current(),
        last_volume: signal(0.8),
        active_line,
        queue: signal(Same(snap.queue.clone())),
        hidden: signal(false),
        close_prompt: signal(false),
    };
    STORE.with(|cell| cell.set(Some(s)));

    // 引擎状态：100 ms 轮询共享状态（信号只在值变化时通知）。
    spawn(async move {
        loop {
            timer(Duration::from_millis(100)).await;
            let snap = player().state();
            s.playback.set(Playback {
                playing: snap.is_playing,
                loading: snap.is_loading,
                time: snap.current_time,
                duration: snap.duration,
                volume: snap.volume,
            });
            s.track.set(now_playing(&snap));
            s.queue.set(Same(snap.queue.clone()));
            drain_covers();
            // 窗口与托盘。
            if crate::window::take_close_request() {
                s.close_prompt.set(true);
            }
            s.hidden.set(crate::window::is_hidden());
            for action in crate::window::take_tray_actions() {
                match action {
                    crate::window::TrayAction::Show => crate::window::show(),
                    crate::window::TrayAction::Toggle => player().toggle(),
                    crate::window::TrayAction::Previous => player().previous(),
                    crate::window::TrayAction::Next => player().next(),
                    crate::window::TrayAction::Quit => crate::window::quit(),
                }
            }
        }
    });

    // 托盘菜单跟着当前曲目与播放状态。
    effect(move || {
        let state = crate::window::TrayState {
            title: s.track.with(|t| if t.artist.is_empty() { t.title.clone() } else { format!("{} — {}", t.title, t.artist) }),
            playing: s.playback.with(|p| p.playing),
            has_track: s.track.with(|t| t.path.is_some()),
        };
        crate::window::update_tray(state);
    });

    // 曲库：扫盘状态、根目录与索引。
    spawn(async move {
        loop {
            timer(Duration::from_millis(120)).await;
            s.library.set(library_state());
            s.tracks.set(Same(library_store().tracks()));
        }
    });

    // 拖动预览：真实进度追上后清除（消除 目标→原始→目标 的跳变）。
    effect(move || {
        let Some(pending) = s.seek_preview.get() else { return };
        if (s.playback.with(|p| p.time) - pending).abs() < 0.35 {
            s.seek_preview.set(None);
        }
    });

    // 换曲：读元数据、歌词、波形（后台线程）。
    let path = computed(move || s.track.with(|t| t.path.clone()));
    effect(move || {
        let current = path.get();
        s.meta.set(None);
        s.lyrics.set(None);
        let Some(stamp) = current else {
            s.waveform.set(None);
            return;
        };
        let floor = crate::waveform::WaveformOptions::default().min;
        s.waveform.set(Some(Same(Arc::new(vec![floor; crate::waveform::BARS]))));
        let still = move |stamp: &str| untracked(|| path.with(|p| p.as_deref() == Some(stamp)));
        let p = stamp.clone();
        spawn(async move {
            let meta = blocking({
                let p = p.clone();
                move || MetaCache::global().get(Path::new(&p))
            })
            .await;
            if still(&p)
                && let Ok(Ok(meta)) = meta
            {
                s.meta.set(Some(Same(meta)));
            }
            let lyrics = blocking({
                let p = p.clone();
                move || Meta::lyrics(&p)
            })
            .await;
            if still(&p)
                && let Ok((output, source)) = lyrics
            {
                s.lyrics.set(Some(Lyrics { output: Same(Arc::new(output)), source }));
            }
            let bars = blocking({
                let p = p.clone();
                move || crate::waveform::waveform(Path::new(&p))
            })
            .await;
            if still(&p) {
                s.waveform.set(match bars {
                    Ok(Ok(bars)) => Some(Same(bars)),
                    _ => None,
                });
            }
        });
    });

    // 文件内容变了（监视或手动扫描后指纹不同）：重新生成它的封面。
    effect(move || {
        let tracks = s.tracks.get();
        let stamps: HashMap<&str, u64> = tracks.iter().map(|t| (t.path.as_str(), t.stamp)).collect();
        let requested: Vec<String> = COVERS.with(|c| c.borrow().keys().cloned().collect());
        COVER_STAMPS.with(|known| {
            let mut known = known.borrow_mut();
            for path in requested {
                let Some(&stamp) = stamps.get(path.as_str()) else { continue };
                match known.insert(path.clone(), stamp) {
                    Some(old) if old != stamp => crate::covers::request(&path),
                    _ => {}
                }
            }
        });
    });

    crate::watcher::sync();
    crate::window::set_close_action(library_store().close_action());
    crate::window::start();
    if !library_store().roots().is_empty() {
        scan();
    }
    s
}

/// 界面字体（`None`：系统字体）。立即生效并保存。
pub fn set_font(font: Option<String>) {
    store().library.update(|l| l.font = font.clone());
    std::thread::spawn(move || library_store().set_font(font));
}

/// 根视图的字体家族。
pub fn font_family() -> SharedString {
    store().library.with(|l| l.font.clone()).map_or_else(|| SharedString::from("system-ui"), SharedString::from)
}

/// 弹出系统文件选择器导入字体文件；返回（结果说明，第一个导入的家族名）。
pub async fn import_fonts() -> Option<(String, Option<String>)> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let picked = blocking(|| {
            rfd::FileDialog::new()
                .set_title("导入字体")
                .add_filter("字体", crate::fonts::FONT_EXTENSIONS)
                .pick_files()
        })
        .await
        .ok()
        .flatten()?;
        let results = blocking(move || picked.iter().map(|p| crate::fonts::import(p)).collect::<Vec<_>>())
            .await
            .ok()?;
        let mut lines = Vec::new();
        let mut first = None;
        for result in results {
            match result {
                Ok(families) => {
                    first = first.or_else(|| families.first().cloned());
                    lines.push(format!("已导入 {}", families.join("、")));
                }
                Err(error) => lines.push(error),
            }
        }
        Some((lines.join("\n"), first))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Some(("此平台暂不支持导入字体".to_owned(), None))
    }
}

/// 关闭窗口时的行为（保存）。
pub fn set_close_action(action: crate::library::CloseAction) {
    store().library.update(|l| l.close_action = action);
    std::thread::spawn(move || library_store().set_close_action(action));
}

/// 回答关闭询问：记住选择，然后照做。
pub fn answer_close(action: crate::library::CloseAction) {
    store().close_prompt.set(false);
    set_close_action(action);
    match action {
        crate::library::CloseAction::Tray => crate::window::hide(),
        crate::library::CloseAction::Quit => crate::window::quit(),
        crate::library::CloseAction::Ask => {}
    }
}

/// 打开/关闭文件夹监视。
pub fn set_watch(on: bool) {
    store().library.update(|l| l.watching = on);
    std::thread::spawn(move || library_store().set_watch(on));
}

fn now_playing(snap: &crate::audio::Snapshot) -> NowPlaying {
    NowPlaying {
        path: snap.path.clone(),
        title: snap.title.clone(),
        artist: snap.artist.clone(),
        album: snap.album.clone(),
        artwork: snap.artwork.clone().map(SharedString::from),
        backdrop: snap.backdrop.clone().map(SharedString::from),
        error: snap.error.clone(),
        index: snap.queue_index,
    }
}

/// 当前歌词行（ap-spec：提前 0.5 s，开头 1 s 内不提前）。
pub fn active_index(output: &LyricsOutput, time: f64) -> i64 {
    let lookahead = if time > 1.0 { 0.5 } else { 0.0 };
    let mut active = -1;
    for (i, line) in output.lines.iter().enumerate() {
        if line.time >= 0.0 && line.time <= time + lookahead {
            active = i as i64;
        }
    }
    active
}

// --- 命令 ---------------------------------------------------------------------

/// 播放 `tracks[index]`，以 `tracks` 为队列。
pub fn play_list(tracks: &[TrackEntry], index: usize) {
    if let Some(track) = tracks.get(index) {
        let queue = tracks.iter().map(|t| t.path.clone()).collect();
        player().open(track.path.clone(), queue);
    }
}

/// 拖动/点击进度的提交。
pub fn seek_to(seconds: f64) {
    store().seek_preview.set(Some(seconds));
    player().seek(seconds);
}

/// 设置音量：界面立即跟手，不等下一次轮询。
pub fn set_volume(volume: f64) {
    let volume = volume.clamp(0.0, 1.0);
    store().playback.update(|p| p.volume = volume);
    player().set_volume(volume);
}

/// 静音切换（记住静音前音量）。
pub fn toggle_mute() {
    let s = store();
    let volume = s.playback.with_untracked(|p| p.volume);
    if volume > 0.0 {
        s.last_volume.set(volume);
        set_volume(0.0);
    } else {
        let last = s.last_volume.get_untracked();
        set_volume(if last > 0.0 { last } else { 0.8 });
    }
}

/// 扫描所有根目录（后台线程）。
pub fn scan() {
    if library_store().is_scanning() {
        return;
    }
    std::thread::spawn(|| library_store().scan());
}

/// 重建曲库：清掉标签与封面缓存后重新读取（后台线程）。
pub fn rebuild() {
    if library_store().is_scanning() {
        return;
    }
    // 已显示的封面保留；重建完成后重新生成，信号再收到新文件。
    let known: Vec<String> = COVERS.with(|c| c.borrow().keys().cloned().collect());
    std::thread::spawn(move || {
        library_store().rebuild();
        for path in known {
            crate::covers::request(&path);
        }
    });
}

/// 改流派分隔符（立即重新拆分，后台线程）。
pub fn set_separators(separators: Vec<String>) {
    std::thread::spawn(move || library_store().set_separators(&separators));
}

/// 弹出系统文件夹选择器；选中的文件夹加入曲库并扫描。返回结果说明。
pub async fn pick_folders() -> Option<String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let picked = blocking(|| {
            let start = dirs::audio_dir().or_else(dirs::home_dir).unwrap_or_default();
            rfd::FileDialog::new().set_title("添加音乐文件夹").set_directory(start).pick_folders()
        })
        .await
        .ok()
        .flatten()?;
        let lib = library_store();
        let mut added = Vec::new();
        let mut errors = Vec::new();
        for path in picked {
            match lib.add_root(&path.to_string_lossy()) {
                Ok(_) => added.push(path.to_string_lossy().into_owned()),
                Err(error) => errors.push(error),
            }
        }
        if !added.is_empty() {
            scan();
        }
        let mut lines: Vec<String> = added.iter().map(|p| format!("已添加 {p}")).collect();
        lines.extend(errors);
        Some(lines.join("\n"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Some("此平台暂不支持文件夹选择器，请把文件夹拖进来".to_owned())
    }
}

// --- 封面 ---------------------------------------------------------------------

/// `path` 所在曲目的封面缩略图（文件路径）。首次读取时交给后台线程池
/// 生成；每个曲目一个信号，生成后只有读它的绑定再运行。信号属于根组件，
/// 列表单元格复用或销毁都不会丢掉进行中的请求。
pub fn cover_of(path: &str) -> Option<SharedString> {
    if std::env::var_os("ZEO_BENCH_NO_COVERS").is_some() {
        return None;
    }
    let hit = COVERS.with(|c| c.borrow().get(path).copied());
    let cover = match hit {
        Some(cover) => cover,
        None => {
            let owner = store().owner;
            let cover = owner.run(|| signal(None::<SharedString>));
            COVERS.with(|c| c.borrow_mut().insert(path.to_owned(), cover));
            crate::covers::request(path);
            cover
        }
    };
    cover.get()
}

fn drain_covers() {
    for (track, thumb) in crate::covers::take_done() {
        if let Some(thumb) = thumb
            && let Some(cover) = COVERS.with(|c| c.borrow().get(&track).copied())
        {
            cover.set(Some(thumb.into()));
        }
    }
}

// --- 格式化 -------------------------------------------------------------------

/// 分:秒。
pub fn format_time(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 { seconds.floor() as u64 } else { 0 };
    format!("{}:{:02}", total / 60, total % 60)
}

/// 总时长：1 小时 5 分钟 / 42 分钟。
pub fn format_total(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    if minutes >= 60 {
        format!("{} 小时 {} 分钟", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} 分钟")
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_formats() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(65.9), "1:05");
        assert_eq!(format_time(f64::NAN), "0:00");
        assert_eq!(format_total(42.0 * 60.0), "42 分钟");
        assert_eq!(format_total(65.0 * 60.0), "1 小时 5 分钟");
    }
}
