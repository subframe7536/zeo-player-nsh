//! 界面状态：引擎与曲库的信号（轮询共享状态）、换曲时的加载流水线
//! （元数据 / 歌词 / 波形）、专辑封面缓存，以及播放命令。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use nsh_core::prelude::*;

use crate::config::{ScanProgress, TrackEntry, library_store};
use crate::engine::{LyricsSource, Meta, Player};
use crate::metadata::lyrics::LyricsOutput;
use crate::metadata::{MetaCache, MetaInfo};
use crate::playlists::{Kind, Playlist, Rule};

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

impl<T: Default> Default for Same<T> {
    fn default() -> Self {
        Same(Arc::new(T::default()))
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
    /// 队列循环模式（顺序 / 单曲 / 随机）。
    pub loop_mode: crate::engine::audio::LoopMode,
}

/// 当前曲目（换曲时变化）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NowPlaying {
    pub path: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Option<SharedString>,
    /// 封面已生成（换歌后、生成前为 `false`）。
    pub covers_ready: bool,
    pub error: Option<String>,
    /// 在播放队列中的位置（判断切歌方向）。
    pub index: i64,
}

/// 曲库概况：冷字段是一份配置的共享快照（设置写入才换新 `Arc`，
/// 轮询按指针判变化），热字段只剩扫描进度与监视器状态。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibraryState {
    /// 曲库配置快照（根目录、外观、过滤阈值、各项偏好……）。
    pub config: Same<crate::config::LibraryConfig>,
    /// 扫描进度（扫描期间推进，平时不变）。
    pub progress: ScanProgress,
    /// 文件夹监视器的运行状态（连接 / 报错；`config.watch` 是设置开关）。
    pub watcher: crate::provider::local::watcher::WatchStatus,
}

impl LibraryState {
    /// 改配置字段：克隆当前快照、应用修改、换新 `Arc`（指针变即通知）。
    /// 磁盘写入与 `LibraryStore` 侧的正式换新由调用方负责；下轮轮询
    /// 会把那边换好的 `Arc` 收进来。
    fn set_config(&mut self, f: impl FnOnce(&mut crate::config::LibraryConfig)) {
        let mut config = (*self.config).clone();
        f(&mut config);
        self.config = Same(Arc::new(config));
    }
}

fn library_state() -> LibraryState {
    let lib = library_store();
    LibraryState {
        config: Same(lib.config()),
        progress: lib.progress(),
        watcher: crate::provider::local::watcher::status(),
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
    /// 当前曲目的手动歌词偏移（毫秒，正值 = 提前；换曲时从数据库读）。
    pub lyrics_offset: Signal<i64>,
    pub waveform: Signal<Option<Same<Vec<f64>>>>,
    /// 拖动 seek 时的预览位置（秒），抬起后保留到真实进度追上。
    pub seek_preview: Signal<Option<f64>>,
    /// 根组件的 owner（`later` 的任务挂在它下面；封面信号另有自己的
    /// 可 dispose owner，见 `COVER_OWNER`）。
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
    /// 标题栏搜索框的内容；非空时内容区显示搜索结果。
    pub search: Signal<String>,
    /// 显示搜索页（没有搜索词时也展示：图标栏点放大镜打开，页顶有输入框）。
    pub search_open: Signal<bool>,
    /// 全部歌单（侧栏顺序）。
    pub playlists: Signal<Same<Vec<Playlist>>>,
    /// 正在编辑的歌单（弹窗打开时为 `Some`）。
    pub editor: Signal<Option<Draft>>,
    /// 设置弹窗打开中。
    pub settings: Signal<bool>,
    /// 「删除文件」的确认弹窗（`Some` 时打开）。
    pub delete_ask: Signal<Option<DeleteAsk>>,
    /// 主题色覆盖的强调色（`None`：主题默认黑白）；`theme::primary` 读它。
    pub accent: Signal<Option<AdaptiveColor>>,
    /// 「跟随封面」时封面的原始主色调（色相圈、饱和度）——界面底色向它
    /// 轻调（`theme::bg` 等）；预设色 / 黑白时 `None`。
    pub tint: Signal<Option<(f32, f32)>>,
    /// 当前开着的弹出菜单（`menu_control` 的 id）：同一时刻只开一个。
    /// 桌面上右键按下不产生 PressOutside（平台只发 ContextMenu 事件），
    /// 已开的菜单不会被平台关掉，由它互斥。
    pub menu_open: Signal<Option<u64>>,
}

/// 歌单编辑弹窗的草稿：`id` 为 `None` 时是新建。
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub id: Option<u64>,
    pub name: String,
    /// 手动歌单为 `None`。
    pub rule: Option<Rule>,
    /// 新建手动歌单时一并加入的曲目（「添加到歌单 → 新建歌单…」）。
    pub add: Vec<String>,
}

thread_local! {
    static STORE: Cell<Option<Store>> = const { Cell::new(None) };
    /// 已请求封面的曲目上次的文件指纹（内容变了就重新生成）。
    static COVER_STAMPS: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
    /// 每个曲目一份封面信号，带已请求过的最大尺寸（升档去重）。
    static COVERS: RefCell<HashMap<String, (u32, Signal<Option<SharedString>>)>> =
        RefCell::new(HashMap::new());
    /// 封面信号的 owner：信号都建在它的独立 root 下，隐藏到托盘时
    /// 整个 dispose——`COVERS` 清掉的只是 `Copy` 句柄，节点与它们存
    /// 的值要靠 dispose 才真正释放。
    static COVER_OWNER: RefCell<Option<Owner>> = const { RefCell::new(None) };
}

/// 界面状态（在 [`install`] 之后可用）。
pub fn store() -> Store {
    STORE
        .with(|s| s.get())
        .expect("store installed by the root component")
}

pub fn player() -> &'static Player {
    Player::global()
}

/// 建立状态并启动轮询；在根组件里调用一次。
pub fn install() -> Store {
    let snap = player().state();
    let lib = library_store();
    let playback = signal(Playback {
        playing: snap.is_playing,
        loading: snap.is_loading,
        time: snap.current_time,
        duration: snap.track.duration,
        volume: snap.volume,
        loop_mode: snap.loop_mode,
    });
    let track = signal(now_playing(&snap));
    let library = signal(library_state());
    let tracks = signal(Same(lib.tracks()));
    let lyrics = signal(None::<Lyrics>);
    let lyrics_offset = signal(0_i64);
    let active_line = computed(move || {
        let time = playback.with(|p| p.time);
        // 生效偏移 = 每曲手动偏移 + 全局偏移（正值 = 提前）。
        let offset =
            lyrics_offset.get() + library.with(|l| l.config.global_lyrics_offset_ms);
        lyrics.with(|l| match l {
            Some(l) if l.output.timed => {
                active_index(&l.output, time + offset as f64 / 1000.0)
            }
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
        lyrics_offset,
        waveform: signal(None),
        seek_preview: signal(None),
        owner: Owner::current(),
        last_volume: signal(0.8),
        active_line,
        queue: signal(Same(snap.track.queue.clone())),
        hidden: signal(false),
        close_prompt: signal(false),
        search: signal(String::new()),
        playlists: signal(Same(Arc::new({
            let mut lists = crate::playlists::load();
            crate::playlists::ensure_favorites(&mut lists);
            lists
        }))),
        editor: signal(None),
        search_open: signal(false),
        settings: signal(false),
        delete_ask: signal(None),
        accent: signal(None),
        tint: signal(None),
        menu_open: signal(None),
    };
    STORE.with(|cell| cell.set(Some(s)));
    let (tx, posted) = mpsc::channel::<Box<dyn FnOnce() + Send>>();
    POSTER.with(|p| *p.borrow_mut() = Some(Post(tx)));
    apply_accent();

    // 引擎状态：100 ms 轮询共享状态（信号只在值变化时通知）。
    spawn(async move {
        // 跟随封面的强调色：上次请求主色调的曲目。
        let mut accent_track: Option<String> = None;
        // 上次见到的曲目快照：引擎侧的冷字段整块换新 `Arc`，指针一样
        // 就说明曲目 / 封面 / 队列 / 错误都没动，跳过整条冷路径。
        let mut last_track = snap.track;
        loop {
            timer(Duration::from_millis(100)).await;
            let snap = player().state();
            s.playback.set(Playback {
                playing: snap.is_playing,
                loading: snap.is_loading,
                time: snap.current_time,
                duration: snap.track.duration,
                volume: snap.volume,
                loop_mode: snap.loop_mode,
            });
            if !Arc::ptr_eq(&snap.track, &last_track) {
                last_track = snap.track.clone();
                s.track.set(now_playing(&snap));
                s.queue.set(Same(snap.track.queue.clone()));
            }
            player().remember(&snap);
            if snap.track.path != accent_track {
                accent_track = snap.track.path.clone();
                request_accent(&s, snap.track.path.as_deref());
            }
            drain_covers();
            drain_dominant();
        }
    });

    // 窗口与托盘：等窗口事件（关闭请求、托盘操作、显示 / 隐藏）再处理。
    spawn(async move {
        loop {
            crate::window::changed().await;
            if crate::window::take_close_request() {
                s.close_prompt.set(true);
            }
            let hidden = crate::window::is_hidden();
            if hidden && !s.hidden.get_untracked() {
                // 刚进托盘：封面信号的 owner 整个 dispose（缩略图在磁盘
                // 缓存，显示时重新请求）；大图 LRU、歌词缓存与 SQLite 页
                // 缓存一并放掉（当前曲目的大图 Arc 由 NowPlaying 持着，
                // 不受影响）；延迟 1 秒等界面拆完，再把空闲堆页还给系统。
                COVERS.with(|c| c.borrow_mut().clear());
                COVER_STAMPS.with(|c| c.borrow_mut().clear());
                COVER_OWNER.with(|o| {
                    if let Some(owner) = o.borrow_mut().take() {
                        owner.dispose();
                    }
                });
                crate::metadata::covers::clear_artwork();
                crate::engine::Meta::clear_lyrics();
                crate::db::shrink_memory();
                spawn(async {
                    timer(Duration::from_secs(1)).await;
                    crate::window::trim_memory();
                });
            }
            s.hidden.set(hidden);
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

    // 托盘菜单跟着当前曲目与播放状态（memo 拦掉进度刷新：订阅整个
    // playback 信号会让 effect 每 100ms 都跑一次 format! + 通道 +
    // SetMenuItemInfoW）。
    let tray_key = computed(move || {
        let (title, has_track) = s.track.with(|t| {
            (
                if t.artist.is_empty() {
                    t.title.clone()
                } else {
                    format!("{} — {}", t.title, t.artist)
                },
                t.path.is_some(),
            )
        });
        (title, s.playback.with(|p| p.playing), has_track)
    });
    effect(move || {
        let (title, playing, has_track) = tray_key.get();
        crate::window::update_tray(crate::window::TrayState {
            title,
            playing,
            has_track,
        });
    });

    // 曲库：扫盘状态、根目录与索引。主题色走全局配置——别处（多窗口 /
    // 其它实例）改了它时重取强调色，不然换曲才刷新的强调色会停在旧值。
    let mut last_color = s.library.with(|l| l.config.theme_color);
    spawn(async move {
        loop {
            timer(Duration::from_millis(120)).await;
            // 后台线程经 poster() 排来的事（重命名 / 删除的收尾等）在这跑；
            // 单个出事只丢它自己，别拖垮轮询。
            while let Ok(f) = posted.try_recv() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            }
            let state = library_state();
            let changed = state.config.theme_color != last_color;
            last_color = state.config.theme_color;
            s.library.set(state);
            if changed {
                apply_accent();
            }
            s.tracks.set(Same(library_store().tracks()));
        }
    });

    // 拖动预览：真实进度追上后清除（消除 目标→原始→目标 的跳变）。
    effect(move || {
        let Some(pending) = s.seek_preview.get() else {
            return;
        };
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
        s.lyrics_offset.set(0);
        let Some(stamp) = current else {
            s.waveform.set(None);
            return;
        };
        let floor = crate::metadata::waveform::WaveformOptions::default().min;
        s.waveform.set(Some(Same(Arc::new(vec![
            floor;
            crate::metadata::waveform::BARS
        ]))));
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
                s.lyrics.set(Some(Lyrics {
                    output: Same(Arc::new(output)),
                    source,
                }));
            }
            let offset = blocking({
                let p = p.clone();
                move || crate::db::lyrics_offset(&p)
            })
            .await;
            if still(&p)
                && let Ok(offset) = offset
            {
                s.lyrics_offset.set(offset);
            }
            let bars = blocking({
                let p = p.clone();
                move || crate::metadata::waveform::waveform(Path::new(&p))
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

    // 队尾预载：当前曲目的后三首提前生成全部封面尺寸、解析好歌词
    // （各自的有界缓存；歌词不进全局元数据缓存）。换曲或队列变化时
    // 重算；重复请求在封面池 / 歌词缓存里去重，成本约等于零。
    effect(move || {
        path.get();
        s.queue.get();
        let snap = player().state();
        let Ok(index) = usize::try_from(snap.track.queue_index) else {
            return;
        };
        let next: Vec<String> = snap
            .track
            .queue
            .iter()
            .skip(index + 1)
            .take(3)
            .cloned()
            .collect();
        if next.is_empty() {
            return;
        }
        std::thread::spawn(move || {
            for p in next {
                crate::metadata::covers::request_warm(&p);
                Meta::lyrics(&p);
            }
        });
    });

    // 文件内容变了（监视或手动扫描后指纹不同）：重新生成它的封面。
    effect(move || {
        let tracks = s.tracks.get();
        let stamps: HashMap<&str, u64> =
            tracks.iter().map(|t| (t.path.as_str(), t.stamp)).collect();
        let requested: Vec<(String, u32)> = COVERS.with(|c| {
            c.borrow()
                .iter()
                .map(|(p, (side, _))| (p.clone(), *side))
                .collect()
        });
        COVER_STAMPS.with(|known| {
            let mut known = known.borrow_mut();
            for (path, side) in requested {
                let Some(&stamp) = stamps.get(path.as_str()) else {
                    continue;
                };
                match known.insert(path.clone(), stamp) {
                    Some(old) if old != stamp => {
                        crate::metadata::covers::request(&path, side);
                    }
                    _ => {}
                }
            }
        });
    });

    crate::provider::local::watcher::sync();
    crate::window::set_close_action(library_store().close_action());
    crate::window::set_remember_frame(library_store().remember_frame());
    set_color_scheme(color_scheme(library_store().appearance()));
    player().set_fade_ms(library_store().fade_ms());
    player().set_pause_on_device_change(library_store().pause_on_device_change());
    player().set_replaygain(library_store().replaygain());
    player().set_eq(library_store().eq());
    player().set_loop_mode(library_store().loop_mode());
    // 在设好循环模式之后：随机模式下恢复的队列按它重新洗牌。
    player().restore_last();
    crate::window::start();
    if !library_store().roots().is_empty() {
        scan();
    }
    s
}

/// 曲库配置的界面侧更新：克隆当前快照、应用修改、换新 `Arc`（界面
/// 立即看到新值；磁盘与 `LibraryStore` 侧的正式写入由调用方负责，
/// 下轮轮询把那边换好的 `Arc` 收进来）。
fn update_config(f: impl FnOnce(&mut crate::config::LibraryConfig)) {
    store().library.update(|l| l.set_config(f));
}

/// 界面字体（`None`：系统字体）。立即生效并保存。
pub fn set_font(font: Option<String>) {
    update_config(|c| c.font = font.clone());
    std::thread::spawn(move || library_store().set_font(font));
}

/// 根视图的字体家族。
pub fn font_family() -> SharedString {
    store()
        .library
        .with(|l| l.config.font.clone())
        .map_or_else(|| SharedString::from("system-ui"), SharedString::from)
}

/// 全屏歌词的对齐方式（立即生效并保存）。
pub fn set_lyrics_align(align: crate::config::LyricsAlign) {
    update_config(|c| c.lyrics_align = align);
    std::thread::spawn(move || library_store().set_lyrics_align(align));
}

/// 当前曲目的歌词偏移设为 `ms`（钳到 ±30000）并保存；没有正在播放的
/// 曲目时不做。
pub fn set_lyrics_offset(ms: i64) {
    let Some(path) = player().state().track.path.clone() else {
        return;
    };
    let v = ms.clamp(-30000, 30000);
    store().lyrics_offset.set(v);
    crate::db::later(move || crate::db::set_lyrics_offset(&path, v));
}

/// 当前曲目的歌词偏移加 `delta_ms`（钳到 ±30000）并保存；`None` 时清零。
pub fn nudge_lyrics_offset(delta_ms: Option<i64>) {
    match delta_ms {
        Some(d) => set_lyrics_offset(store().lyrics_offset.get() + d),
        None => set_lyrics_offset(0),
    }
}

/// 全屏歌词行悬停显示时间戳（立即生效并保存）。
pub fn set_lyrics_timestamps(on: bool) {
    update_config(|c| c.lyrics_timestamps = on);
    std::thread::spawn(move || library_store().set_lyrics_timestamps(on));
}

/// 全局歌词偏移（立即生效并保存；钳到 ±30000 毫秒）。
pub fn set_global_lyrics_offset(ms: i64) {
    let ms = ms.clamp(-30000, 30000);
    update_config(|c| c.global_lyrics_offset_ms = ms);
    std::thread::spawn(move || library_store().set_global_lyrics_offset_ms(ms));
}

/// 持久化的外观 → NSH 的 `Appearance`（`set_color_scheme` 的参数）。
fn color_scheme(mode: crate::config::Appearance) -> Appearance {
    match mode {
        crate::config::Appearance::System => Appearance::System,
        crate::config::Appearance::Light => Appearance::Light,
        crate::config::Appearance::Dark => Appearance::Dark,
    }
}

/// 界面外观（立即生效并保存）。
pub fn set_appearance(mode: crate::config::Appearance) {
    update_config(|c| c.appearance = mode);
    set_color_scheme(color_scheme(mode));
    std::thread::spawn(move || library_store().set_appearance(mode));
}

/// 主题色的当前覆盖强调色（`None`：主题默认黑白）；`theme::primary` 读它。
pub fn accent() -> Option<AdaptiveColor> {
    store().accent.get()
}

/// 「跟随封面」的原始主色调（界面底色轻调用；非封面模式为 `None`）。
pub fn tint() -> Option<(f32, f32)> {
    store().tint.get()
}

/// 主题色：预设强调色或跟随当前曲目封面（立即生效并保存）。
pub fn set_theme_color(color: crate::config::ThemeColor) {
    update_config(|c| c.theme_color = color);
    apply_accent();
    std::thread::spawn(move || library_store().set_theme_color(color));
}

/// 按 `theme_color` 刷新覆盖色：预设色直接写；跟随封面时对当前曲目
/// 异步取主色调（结果由轮询的 [`drain_dominant`] 写回）。模式读 `library`
/// 信号（`set_theme_color` 先更新它，落盘在线程里，内存配置有延迟）。
fn apply_accent() {
    let s = store();
    match s.library.with(|l| l.config.theme_color) {
        crate::config::ThemeColor::Cover => {
            let path = s.track.with(|t| t.path.clone());
            request_accent(&s, path.as_deref())
        }
        mode => {
            s.accent.set(super::theme::preset_accent(mode));
            s.tint.set(None);
        }
    }
}

/// 跟随封面模式下取 `path` 的主色调（扫描时已写进 SQLite，同步读）；
/// 库里没有（老库 / 还没扫到）就让 covers 工作线程补算，结果经
/// [`drain_dominant`] 写回。没有曲目时回主题色。
fn request_accent(s: &Store, path: Option<&str>) {
    if s.library.with(|l| l.config.theme_color) != crate::config::ThemeColor::Cover {
        return;
    }
    let Some(path) = path else {
        s.accent.set(None);
        s.tint.set(None);
        return;
    };
    match crate::metadata::covers::art_hue(std::path::Path::new(path)) {
        Some((h, sat)) => {
            s.accent.set(Some(super::theme::accent_of(h, sat)));
            s.tint.set(Some((h, sat)));
        }
        None => {
            s.accent.set(None);
            s.tint.set(None);
            crate::metadata::covers::request(path, crate::metadata::covers::TINY);
        }
    }
}

/// 封面主色调结果：跟随封面模式下成为强调色（只对当前曲目生效）。
fn drain_dominant() {
    for (path, hue) in crate::metadata::covers::take_dominant() {
        let s = store();
        if s.library.with(|l| l.config.theme_color) == crate::config::ThemeColor::Cover
            && s.track.with(|t| t.path.as_deref() == Some(path.as_str()))
        {
            s.accent
                .set(hue.map(|(h, sat)| super::theme::accent_of(h, sat)));
            s.tint.set(hue);
        }
    }
}

/// 淡入淡出时长（立即生效并保存）。
pub fn set_fade_ms(ms: u32) {
    let ms = ms.min(5000);
    update_config(|c| c.fade_ms = ms);
    player().set_fade_ms(ms);
    std::thread::spawn(move || library_store().set_fade_ms(ms));
}

/// 切到下一个循环模式（顺序 → 单曲 → 随机），并保存。
pub fn cycle_loop_mode() {
    let mode = player().loop_mode().cycle();
    player().set_loop_mode(mode);
    std::thread::spawn(move || library_store().set_loop_mode(mode));
}

/// 本地歌词优先（保存）。歌词缓存键带开关位，下次读取自动换来源。
pub fn set_prefer_local_lyrics(on: bool) {
    update_config(|c| c.prefer_local_lyrics = on);
    std::thread::spawn(move || library_store().set_prefer_local_lyrics(on));
}

/// 「删除文件」不经过回收站（保存）。
pub fn set_delete_permanently(on: bool) {
    update_config(|c| c.delete_permanently = on);
    std::thread::spawn(move || library_store().set_delete_permanently(on));
}

/// 输出设备变化时暂停播放（立即生效并保存）。
pub fn set_pause_on_device_change(on: bool) {
    update_config(|c| c.pause_on_device_change = on);
    player().set_pause_on_device_change(on);
    std::thread::spawn(move || library_store().set_pause_on_device_change(on));
}

/// ReplayGain 设置（立即生效并保存）。
pub fn set_replaygain(cfg: crate::engine::audio::ReplayGainConfig) {
    update_config(|c| c.replaygain = cfg);
    player().set_replaygain(cfg);
    std::thread::spawn(move || library_store().set_replaygain(cfg));
}

/// 均衡器设置（立即生效；持久化防抖 400ms——拖滑块不每动一下都写盘）。
pub fn set_eq(cfg: crate::engine::eq::EqConfig) {
    update_config(|c| c.eq = cfg);
    player().set_eq(cfg);
    // 内存快照立即更新（120ms 轮询不会把界面弹回旧档）。
    library_store().set_eq(cfg);
    // 落盘防抖：GEN>0 表示已有守候线程在等安静——拖动推子一秒能产生
    // 上百次调用，不能每动一下都起一个线程。
    static GEN: AtomicU64 = AtomicU64::new(0);
    if GEN.fetch_add(1, Ordering::Relaxed) == 0 {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_millis(400));
            // 400ms 内没人再调（GEN 仍是 0）：落盘并收工。
            if GEN.swap(0, Ordering::Relaxed) == 0 {
                library_store().save();
                return;
            }
        });
    }
}

/// 「删除文件」确认弹窗的请求：记下路径与当前的删除方式（回收站 / 直接删）。
#[derive(Debug, Clone, PartialEq)]
pub struct DeleteAsk {
    pub path: String,
    pub permanent: bool,
}

/// 从曲库移除 `path` 的记录：索引、元数据缓存与数据库、封面缓存文件、
/// 各手动歌单（含「我喜欢」）都清掉；文件不碰（还在时下次扫描会重新
/// 入库）。正在播放这首就停掉。
pub fn remove_record(path: &str) {
    let s = store();
    if s.track.with_untracked(|t| t.path.as_deref() == Some(path)) {
        player().stop();
    }
    let fs_path = Path::new(path);
    crate::metadata::covers::evict(fs_path);
    MetaCache::global().remove(fs_path);
    let db_path = path.to_owned();
    crate::db::later(move || crate::db::remove_meta(&db_path));
    change_playlists(|lists| {
        for p in lists.iter_mut() {
            if let Kind::Manual { tracks } = &mut p.kind {
                tracks.retain(|t| t != path);
            }
        }
    });
    library_store().remove_track(path);
    s.tracks.set(Same(library_store().tracks()));
}

/// `src` 移 / 改名到 `dest`：手动歌单里等于它或以 `src/` 为前缀的路
/// 径换成新前缀，曲目留在歌单里。
pub fn move_playlist_paths(src: &str, dest: &str) {
    let sep = std::path::MAIN_SEPARATOR;
    let (src, dest) = (
        src.trim_end_matches(sep).to_owned(),
        dest.trim_end_matches(sep).to_owned(),
    );
    if src == dest {
        return;
    }
    let under = format!("{src}{sep}");
    change_playlists(|lists| {
        for p in lists.iter_mut() {
            if let Kind::Manual { tracks } = &mut p.kind {
                for t in tracks.iter_mut() {
                    if *t == src {
                        *t = dest.clone();
                    } else if let Some(rest) = t.strip_prefix(&under) {
                        *t = format!("{dest}{sep}{rest}");
                    }
                }
            }
        }
    });
}

/// 删掉 `path` 下（含自身）的手动歌单条目（文件夹已删 / 记录被清，
/// 留着是死条目）。
pub fn remove_playlist_paths_under(path: &str) {
    let sep = std::path::MAIN_SEPARATOR;
    let trimmed = path.trim_end_matches(sep);
    let under = format!("{trimmed}{sep}");
    change_playlists(|lists| {
        for p in lists.iter_mut() {
            if let Kind::Manual { tracks } = &mut p.kind {
                tracks.retain(|t| t != trimmed && !t.starts_with(&under));
            }
        }
    });
}

/// 「删除文件…」菜单项：打开确认弹窗（回收站 / 直接删按当前设置）。
pub fn ask_delete_file(path: &str) {
    let permanent = library_store().delete_permanently();
    store().delete_ask.set(Some(DeleteAsk {
        path: path.to_owned(),
        permanent,
    }));
}

/// 确认删除：后台线程做文件操作（解码器可能还握着文件——Windows 上开着
/// 移不走，先停掉再多试几次），成功后删掉曲库记录。
pub fn confirm_delete_file() {
    let Some(ask) = store().delete_ask.get() else {
        return;
    };
    let ui = poster();
    std::thread::spawn(move || {
        // 工作线程拿不到 store()（线程局部），用播放器快照看当前曲。
        if player().state().track.path.as_deref() == Some(ask.path.as_str()) {
            player().stop();
        }
        let path = Path::new(&ask.path);
        let mut gone = !path.exists();
        for _ in 0..10 {
            if gone {
                break;
            }
            if crate::provider::local::delete_file(path, ask.permanent).is_ok() {
                gone = true;
            } else {
                std::thread::sleep(Duration::from_millis(150));
            }
        }
        ui.post(move || {
            if gone {
                remove_record(&ask.path);
            }
        });
    });
}

/// 本地封面优先（保存）：封面缓存键含开关位，重算已显示的封面
/// （已生成文件保留，翻回来直接命中）。
pub fn set_prefer_local_covers(on: bool) {
    update_config(|c| c.prefer_local_covers = on);
    std::thread::spawn(move || {
        library_store().set_prefer_local_covers(on);
        // 先写配置再请求：缓存键按新开关算，已显示的信号拿到新来源的图。
        let known: Vec<(String, u32)> = COVERS.with(|c| {
            c.borrow()
                .iter()
                .map(|(p, (side, _))| (p.clone(), *side))
                .collect()
        });
        for (path, side) in known {
            crate::metadata::covers::request(&path, side);
        }
    });
}

/// 列表是否显示封面（保存）：关掉后行 / 网格 / 队列只画占位图，
/// 省掉所有列表封面的解码内存；打开即恢复（信号还在，值现成）。
pub fn set_show_covers(on: bool) {
    update_config(|c| c.show_covers = on);
    std::thread::spawn(move || library_store().set_show_covers(on));
}

/// 关闭窗口时的行为（保存）。
pub fn set_close_action(action: crate::config::CloseAction) {
    update_config(|c| c.close_action = action);
    std::thread::spawn(move || library_store().set_close_action(action));
}

/// 「记住窗口大小和位置」（立即生效并保存；关掉后已存的位置丢弃）。
pub fn set_remember_frame(on: bool) {
    update_config(|c| c.remember_frame = on);
    std::thread::spawn(move || library_store().set_remember_frame(on));
}

/// 回答关闭询问：记住选择，然后照做。
pub fn answer_close(action: crate::config::CloseAction) {
    store().close_prompt.set(false);
    set_close_action(action);
    match action {
        crate::config::CloseAction::Tray => crate::window::hide(),
        crate::config::CloseAction::Quit => crate::window::quit(),
        crate::config::CloseAction::Ask => {}
    }
}

/// 打开/关闭文件夹监视。
pub fn set_watch(on: bool) {
    update_config(|c| c.watch = on);
    std::thread::spawn(move || library_store().set_watch(on));
}

fn now_playing(snap: &crate::engine::audio::Snapshot) -> NowPlaying {
    let t = &snap.track;
    NowPlaying {
        path: t.path.clone(),
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        artwork: t.artwork.clone().map(SharedString::from),
        covers_ready: t.covers_ready,
        error: t.error.clone(),
        index: t.queue_index,
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

// --- 歌单 ---------------------------------------------------------------------

/// 改歌单并保存（文件只有几 KB，直接在界面线程写）。
fn change_playlists(f: impl FnOnce(&mut Vec<Playlist>)) {
    let s = store();
    let mut lists = s.playlists.with_untracked(|l| l.0.as_ref().clone());
    f(&mut lists);
    if let Err(err) = crate::playlists::save(&lists) {
        eprintln!("zeo-player: 歌单保存失败: {err}");
    }
    s.playlists.set(Same(Arc::new(lists)));
}

/// 按 id 取歌单。
pub fn playlist(id: u64) -> Option<Playlist> {
    store()
        .playlists
        .with_untracked(|l| l.iter().find(|p| p.id == id).cloned())
}

/// 打开新建弹窗（`smart`：智能歌单；`add`：建好后加入的曲目）。
pub fn new_playlist(smart: bool, add: Vec<String>) {
    let name = if smart { "智能歌单" } else { "新歌单" };
    store().editor.set(Some(Draft {
        id: None,
        name: name.to_owned(),
        rule: smart.then(Rule::default),
        add,
    }));
}

/// 打开编辑弹窗（「我喜欢」不能改名）。
pub fn edit_playlist(id: u64) {
    if let Some(p) = playlist(id).filter(|p| !p.favorite) {
        let rule = match p.kind {
            Kind::Smart { rule } => Some(rule),
            Kind::Manual { .. } => None,
        };
        store().editor.set(Some(Draft {
            id: Some(id),
            name: p.name,
            rule,
            add: Vec::new(),
        }));
    }
}

/// 保存弹窗里的草稿；新建的歌单会打开。返回歌单 id。
pub fn save_draft(draft: Draft) -> u64 {
    let name = match draft.name.trim() {
        "" if draft.rule.is_some() => "智能歌单".to_owned(),
        "" => "新歌单".to_owned(),
        name => name.to_owned(),
    };
    let mut id = 0;
    change_playlists(|lists| {
        match draft
            .id
            .and_then(|id| lists.iter_mut().find(|p| p.id == id))
        {
            Some(p) => {
                id = p.id;
                p.name = name;
                if let (Kind::Smart { rule }, Some(new)) = (&mut p.kind, draft.rule) {
                    *rule = new;
                }
            }
            None => {
                id = crate::playlists::next_id(lists);
                let kind = match draft.rule {
                    Some(rule) => Kind::Smart { rule },
                    None => Kind::Manual {
                        tracks: dedup(draft.add),
                    },
                };
                lists.push(Playlist {
                    id,
                    name,
                    kind,
                    favorite: false,
                });
            }
        }
    });
    store().editor.set(None);
    if draft.id.is_none() {
        store().search.set(String::new());
        global::<super::Nav>().set(super::Nav::Playlist(id));
    }
    id
}

fn dedup(paths: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    paths
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

/// 删除歌单（正看着它时回到文件夹页）。「我喜欢」不能删除。
pub fn delete_playlist(id: u64) {
    if playlist(id).is_none_or(|p| p.favorite) {
        return;
    }
    change_playlists(|lists| lists.retain(|p| p.id != id));
    let nav = global::<super::Nav>();
    if nav.get_untracked() == super::Nav::Playlist(id) {
        nav.set(super::Nav::Folders);
    }
}

/// 把曲目加到手动歌单末尾（已在里面的不重复加）。
pub fn add_to_playlist(id: u64, paths: Vec<String>) {
    change_playlists(|lists| {
        if let Some(Playlist {
            kind: Kind::Manual { tracks },
            ..
        }) = lists.iter_mut().find(|p| p.id == id)
        {
            for path in paths {
                if !tracks.contains(&path) {
                    tracks.push(path);
                }
            }
        }
    });
}

/// 在根组件的生命周期里、下一轮再做 `f`：菜单项触发、但做的事会拆掉
/// 菜单自己所在的那一行时用（行里的任务会随行一起取消）。
pub fn later(f: impl FnOnce() + 'static) {
    store().owner.run(|| {
        spawn(async move {
            // 让菜单先关完（关闭也是一轮更新），再做会拆掉菜单的事。
            timer(Duration::from_millis(16)).await;
            f()
        });
    });
}

/// 下一个受控弹出层的 id（`menu_control`）。
static NEXT_MENU: AtomicU64 = AtomicU64::new(1);

/// 一个弹出层（`menu` / `context_menu` / `popover`）的互斥门：共享
/// `store().menu_open` 令牌，同一时刻只开一个。桌面上右键按下不产生
/// PressOutside（macOS / Windows 的右键只发 ContextMenu 事件，没有
/// touch Start），平台不会替我们关掉上一个——开新弹层时令牌换号，
/// 旧弹层的 `open` 翻 false 自己关掉。
#[derive(Clone, Copy)]
pub struct MenuGate {
    id: u64,
    open: Signal<Option<u64>>,
}

impl MenuGate {
    /// `menu` / `context_menu` 的 `.open(..)`。
    pub fn open(self) -> impl Fn() -> Option<bool> + 'static {
        let (id, open) = (self.id, self.open);
        move || Some(open.get() == Some(id))
    }

    /// `menu` / `context_menu` 的 `.on_open_change(..)`：开关请求写回令牌。
    pub fn menu_change(self) -> impl Fn(&nsh_base::MenuEvent) + 'static {
        let (id, open) = (self.id, self.open);
        move |e| {
            if let nsh_base::MenuEvent::OpenChange(next, _) = e {
                if *next {
                    open.set(Some(id));
                } else if open.get_untracked() == Some(id) {
                    open.set(None);
                }
            }
        }
    }

    /// `popover` 的 `.open(..)`：它另有自己的开关信号 `local`，弹层开着
    /// 且令牌还在时才显示（令牌被夺走即被挤掉）。
    pub fn open_with(self, local: Signal<bool>) -> impl Fn() -> Option<bool> + 'static {
        let (id, open) = (self.id, self.open);
        move || Some(local.get() && open.get() == Some(id))
    }

    /// `popover` 的 `.on_open_change(..)`：请求写回本地信号与令牌。
    pub fn popover_change(self, local: Signal<bool>) -> impl Fn(&nsh_base::PopoverEvent) + 'static {
        let (id, open) = (self.id, self.open);
        move |e| {
            if let nsh_base::PopoverEvent::OpenChange(next, _) = e {
                local.set(*next);
                if *next {
                    open.set(Some(id));
                } else if open.get_untracked() == Some(id) {
                    open.set(None);
                }
            }
        }
    }

    /// `effect(gate.evicted(local))`：令牌被别的弹层夺走时复位 `popover`
    /// 的本地信号（触发器高亮、面板状态读它，不复位会留下「看着还开着」
    /// 的残影）。
    pub fn evicted(self, local: Signal<bool>) -> impl Fn() + 'static {
        let (id, open) = (self.id, self.open);
        move || {
            if open.get() != Some(id) {
                local.set(false);
            }
        }
    }
}

/// 一个弹出层的互斥门：`.open(..)` 用 `gate.open()`、`.on_open_change(..)`
/// 用 `gate.menu_change()`；`popover` 用 `gate.open_with(local)` /
/// `gate.popover_change(local)` 并 `effect(gate.evicted(local))`。
pub fn menu_control() -> MenuGate {
    MenuGate {
        id: NEXT_MENU.fetch_add(1, Ordering::Relaxed),
        open: store().menu_open,
    }
}

/// 「把一件事排回界面侧跑」的把手：`Send`，可以带进后台线程。`STORE`
/// 是线程局部的，工作线程拿不到 `store()`，在界面侧拿 `poster()` 的
/// 克隆带去，`post(f)` 排的事由 `install` 的轮询循环在界面侧执行。
/// 每个应用实例一条通道（界面测试里多个实例并行互不串）。
#[derive(Clone)]
pub struct Post(mpsc::Sender<Box<dyn FnOnce() + Send>>);

impl Post {
    /// 把 `f` 排给界面侧执行。
    pub fn post(&self, f: impl FnOnce() + Send + 'static) {
        let _ = self.0.send(Box::new(f));
    }
}

thread_local! {
    static POSTER: RefCell<Option<Post>> = const { RefCell::new(None) };
}

/// 当前应用实例的回排把手（组件上下文里拿）。
pub fn poster() -> Post {
    POSTER
        .with(|p| p.borrow().clone())
        .expect("store installed by the root component")
}

/// 从手动歌单里移除一首。
pub fn remove_from_playlist(id: u64, path: &str) {
    change_playlists(|lists| {
        if let Some(Playlist {
            kind: Kind::Manual { tracks },
            ..
        }) = lists.iter_mut().find(|p| p.id == id)
        {
            tracks.retain(|p| p != path);
        }
    });
}

/// `path` 在「我喜欢」里。读取即订阅。
pub fn is_liked(path: &str) -> bool {
    store().playlists.with(|l| {
        l.iter().any(|p| {
            p.favorite
                && matches!(&p.kind, Kind::Manual { tracks } if tracks.iter().any(|t| t == path))
        })
    })
}

/// 喜欢 / 取消喜欢 `path`：加到「我喜欢」最前面，或从中移除。
pub fn toggle_like(path: &str) {
    change_playlists(|lists| {
        if let Some(Playlist {
            kind: Kind::Manual { tracks },
            ..
        }) = lists.iter_mut().find(|p| p.favorite)
        {
            if let Some(at) = tracks.iter().position(|t| t == path) {
                tracks.remove(at);
            } else {
                tracks.insert(0, path.to_owned());
            }
        }
    });
}

/// 当前曲目在「我喜欢」里。读取即订阅。
pub fn current_liked() -> bool {
    store()
        .track
        .with(|t| t.path.clone())
        .is_some_and(|p| is_liked(&p))
}

/// 喜欢 / 取消喜欢当前曲目（没有曲目时什么也不做）。
pub fn toggle_like_current() {
    if let Some(path) = store().track.with_untracked(|t| t.path.clone()) {
        toggle_like(&path);
    }
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
    let known: Vec<(String, u32)> = COVERS.with(|c| {
        c.borrow()
            .iter()
            .map(|(p, (side, _))| (p.clone(), *side))
            .collect()
    });
    std::thread::spawn(move || {
        library_store().rebuild();
        for (path, side) in known {
            crate::metadata::covers::request(&path, side);
        }
    });
}

/// 改流派分隔符（立即重新拆分，后台线程）。
pub fn set_separators(separators: Vec<String>) {
    std::thread::spawn(move || library_store().set_separators(&separators));
}

/// 过滤阈值：短于 `secs` 的音频不进曲库（0 不过滤）；保存后重新扫描。
pub fn set_min_duration_secs(secs: u32) {
    std::thread::spawn(move || {
        library_store().set_min_duration_secs(secs);
        rescan_when_idle();
    });
}

/// 过滤阈值：小于 `kb` KB 的音频不进曲库（0 不过滤）；保存后重新扫描。
pub fn set_min_size_kb(kb: u32) {
    std::thread::spawn(move || {
        library_store().set_min_size_kb(kb);
        rescan_when_idle();
    });
}

/// 改完阈值重新扫描；进行中的扫描先等它结束，不然用的是旧阈值。
fn rescan_when_idle() {
    while library_store().is_scanning() {
        std::thread::sleep(Duration::from_millis(200));
    }
    library_store().scan();
}

/// 弹出系统文件夹选择器；选中的文件夹加入曲库并扫描。返回结果说明。
pub async fn pick_folders() -> Option<String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let picked = blocking(|| {
            let start = dirs::audio_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_default();
            rfd::FileDialog::new()
                .set_title("添加音乐文件夹")
                .set_directory(start)
                .pick_folders()
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

/// `path` 的列表封面（文件路径）：`pt` ≤ 48 的插槽（曲目行、队列、
/// chips）给 96px 小图，更大的（网格 ~184pt、详情页 hero）升档到
/// 400px——每个音频文件最多一份缓存封面，升档后小文件删掉。首次读取
/// 时交给后台线程池生成；每个曲目一个信号，生成后只有读它的绑定再
/// 运行。信号属于专用的 `COVER_OWNER`，列表单元格复用或销毁都不会
/// 丢掉进行中的请求，隐藏到托盘时才整个拆掉。
/// 「列表显示封面」开关只管列表，关掉时这里返回 `None`——不建信号、
/// 不发请求，列表完全不解码封面；读库信号让调用处的闭包订阅它，切换
/// 即重渲。
pub fn cover_for(path: &str, pt: f32) -> Option<SharedString> {
    if !store().library.with(|l| l.config.show_covers) {
        return None;
    }
    cover_for_always(path, pt)
}

/// 与 [`cover_for`] 相同但不受「列表显示封面」开关影响：播放条的当前
/// 曲目封面一直显示（开关只管列表渲染）。
pub fn cover_for_always(path: &str, pt: f32) -> Option<SharedString> {
    if cfg!(any(debug_assertions, test))
        && std::env::var_os("ZEO_PLAYER_BENCH_NO_COVERS").is_some()
    {
        return None;
    }
    let side = if pt <= 48.0 {
        crate::metadata::covers::TINY
    } else {
        crate::metadata::covers::THUMB
    };
    let hit = COVERS.with(|c| c.borrow().get(path).copied());
    let cover = match hit {
        Some((have, cover)) => {
            // 升档：先记下已请求的尺寸，重复调用不再排队。
            if have < side {
                COVERS.with(|c| c.borrow_mut().insert(path.to_owned(), (side, cover)));
                crate::metadata::covers::request(path, side);
            }
            cover
        }
        None => {
            // 挂在专用 owner 下：根 owner 永不 dispose，信号建在它下面
            // 的话隐藏时清句柄也放不掉节点。
            let cover = COVER_OWNER.with(|o| {
                let mut o = o.borrow_mut();
                if o.as_ref().is_none_or(|owner| !owner.is_live()) {
                    *o = Some(nsh_core::element::create_root(|| {}).0);
                }
                o.unwrap().run(|| signal(None::<SharedString>))
            });
            let mut have = side;
            // 封面已在磁盘缓存（扫描预热 / 队尾预载 / 播过）就先填上，
            // 免得闪一次占位符；请求照旧入队（色调补算 / 指纹变更重算）。
            if let Some((s, file)) = crate::metadata::covers::cached_list(Path::new(path)) {
                cover.set(Some(file.to_string_lossy().into_owned().into()));
                have = have.max(s);
            }
            COVERS.with(|c| c.borrow_mut().insert(path.to_owned(), (have, cover)));
            crate::metadata::covers::request(path, side);
            cover
        }
    };
    cover.get()
}

fn drain_covers() {
    for (track, side, file) in crate::metadata::covers::take_done() {
        if let Some((have, cover)) = COVERS.with(|c| c.borrow().get(&track).copied()) {
            let keep = side.max(have);
            if side >= have {
                COVERS.with(|c| c.borrow_mut().insert(track.clone(), (keep, cover)));
                // None（无封面 / 来源换掉了）也要写：清掉可能已过期的旧图。
                cover.set(file.map(SharedString::from));
            }
            // 每个音频文件最多一份列表封面：信号已换到大文件（或升档
            // 请求已记下），小文件删掉。
            crate::metadata::covers::prune_smaller(Path::new(&track), keep);
        }
    }
}

// --- 格式化 -------------------------------------------------------------------

/// 分:秒。
pub fn format_time(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds.floor() as u64
    } else {
        0
    };
    format!("{}:{:02}", total / 60, total % 60)
}

/// 当前播放进度（拖动时为预览）。
pub fn shown_time() -> f64 {
    let s = store();
    s.seek_preview
        .get()
        .unwrap_or_else(|| s.playback.with(|p| p.time))
}

/// 当前进度比例。
pub fn progress_fraction() -> f64 {
    let s = store();
    let duration = s.playback.with(|p| p.duration);
    if duration > 0.0 {
        (shown_time() / duration).min(1.0)
    } else {
        0.0
    }
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
