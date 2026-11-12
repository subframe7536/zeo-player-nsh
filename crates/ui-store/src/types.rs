use super::*;

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
    pub loop_mode: zeo_engine::audio::LoopMode,
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
    pub config: Same<zeo_types::config::LibraryConfig>,
    /// 扫描进度（扫描期间推进，平时不变）。
    pub progress: ScanProgress,
    /// 文件夹监视器的运行状态（连接 / 报错；`config.watch` 是设置开关）。
    pub watcher: zeo_library::provider::local::watcher::WatchStatus,
}

impl LibraryState {
    /// 改配置字段：克隆当前快照、应用修改、换新 `Arc`（指针变即通知）。
    /// 磁盘写入与 `LibraryStore` 侧的正式换新由调用方负责；下轮轮询
    /// 会把那边换好的 `Arc` 收进来。
    pub(crate) fn set_config(&mut self, f: impl FnOnce(&mut zeo_types::config::LibraryConfig)) {
        let mut config = (*self.config).clone();
        f(&mut config);
        self.config = Same(Arc::new(config));
    }
}

pub(crate) fn library_state() -> LibraryState {
    let lib = library_store();
    LibraryState {
        config: Same(lib.config()),
        progress: lib.progress(),
        watcher: zeo_library::provider::local::watcher::status(),
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
    /// 播放 / 暂停的用户意图：点击立刻记下目标态，播放键马上形变，
    /// 不等引擎命令的轮询回传（见 `toggle` / `shown_playing`）。
    /// `Instant` 是**截止时刻**——引擎一直没跟上（没有可播曲目之类）
    /// 到期就丢弃，回落到真实状态。
    pub intent: Signal<Option<(bool, Instant)>>,
    pub track: Signal<NowPlaying>,
    pub library: Signal<LibraryState>,
    pub tracks: Signal<Same<Vec<TrackEntry>>>,
    pub dirs: Signal<Same<Vec<String>>>,
    pub meta: Signal<Option<Same<MetaInfo>>>,
    pub lyrics: Signal<Option<Lyrics>>,
    /// 当前曲目的手动歌词偏移（毫秒，正值 = 提前；换曲时从数据库读）。
    pub lyrics_offset: Signal<i64>,
    pub waveform: Signal<Option<Same<Vec<f64>>>>,
    /// 拖动 seek 时的预览位置（秒），抬起后保留到真实进度追上。
    pub seek_preview: Signal<Option<f64>>,
    /// 根组件的 owner（`later` 的任务挂在它下面；封面信号另有自己的
    /// 可 dispose owner，见 `COVER_OWNER`）。
    pub(crate) owner: Owner,
    /// 静音前的音量。
    pub(crate) last_volume: Signal<f64>,
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
    /// 期望的全屏播放状态：`App` 的本地信号才是真正挂载状态，外部
    /// （快捷键、命令面板）改这里，由根组件的对账 effect 调 open/close
    /// （动画与歌词 Reveal 记数只在那里做，直接写挂载信号拿不到）。
    pub full_screen: Signal<bool>,
    /// 「删除文件」的确认弹窗（`Some` 时打开）。
    pub delete_ask: Signal<Option<DeleteAsk>>,
    /// 「编辑元数据」弹窗打开的曲目路径（`Some` 时打开）。
    pub edit_info: Signal<Option<String>>,
    /// 主题色覆盖的强调色（`None`：主题默认黑白）；`theme::primary` 读它。
    pub accent: Signal<Option<AdaptiveColor>>,
    /// 「跟随封面」时封面的原始主色调（色相圈、饱和度）——界面底色向它
    /// 轻调（`theme::bg` 等）；预设色 / 黑白时 `None`。
    pub tint: Signal<Option<(f32, f32)>>,
    /// 「歌手」页当前点开的歌手（分组 key，即歌手标签原文）；首页
    /// 「热门艺术家」深链进详情也写它。
    pub artist_sel: Signal<Option<String>>,
    /// 侧栏下半部当前显示的视图（歌单 / 标签 / 文件夹）。
    pub sidebar_section: Signal<SidebarSection>,
    /// 「标签」页当前选中的流派（分组 key）；侧栏与主视图共用。
    pub genre_sel: Signal<Option<String>>,
    /// 「文件夹」页当前选中的目录路径；侧栏树与主视图共用。
    pub folder_sel: Signal<Option<String>>,
    /// 文件夹树当前展开的目录路径集合。
    pub folder_expanded: Signal<BTreeSet<String>>,
    /// 本次会话创建但尚无曲目的空目录集合（用于挂进目录树）。
    pub folder_extra: Signal<BTreeSet<String>>,
    /// 文件夹操作反馈提示（副标题展示）。
    pub folder_message: Signal<String>,
    /// 文件夹增删改弹窗（`Some` 时打开）。
    pub folder_op: Signal<Option<FileOp>>,
    /// 应用更新状态（「关于」页）。
    pub update: Signal<UpdateState>,
    /// 生效的快捷键表（`KeyAction` → 键位；空串 = 未绑定）：绑定挂在读
    /// 它的 `dynamic` 里，换绑时换 owner 重绑。
    pub keybinds: Signal<Same<Keybinds>>,
    /// 生效的全局快捷键表（`KeyAction` → 键位；空串 = 未绑定，默认全
    /// 不绑）：`install` 的 effect 换绑时整体注销重绑。
    pub global_keybinds: Signal<Same<Keybinds>>,
    /// 设置页「按下新键」捕获态（`Some` = 正在捕获）：全局快捷键此时
    /// 整体注销，按下的候选键不会被已注册的全局键吃掉。
    pub key_capture: Signal<Option<(zeo_keybinds::BindScope, KeyAction)>>,
}

pub use zeo_types::config::SidebarSection;

/// 文件夹增删改的操作请求（弹窗状态）。
#[derive(Debug, Clone, PartialEq)]
pub enum FileOp {
    /// 在 `parent` 下新建文件夹。
    NewFolder { parent: String },
    /// 重命名 `path`（`dir`：文件夹；否则是文件）。
    Rename { path: String, dir: bool },
    /// 删除文件夹 `path`。
    DeleteFolder { path: String },
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
    pub(crate) static STORE: Cell<Option<Store>> = const { Cell::new(None) };
    /// 已请求封面的曲目上次的文件指纹（内容变了就重新生成）。
    pub(crate) static COVER_STAMPS: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
    /// 每个曲目一份封面信号，带已请求过的最大尺寸（升档去重）。
    pub(crate) static COVERS: RefCell<HashMap<String, (u32, Signal<Option<SharedString>>)>> =
        RefCell::new(HashMap::new());
    /// 封面信号的 owner：信号都建在它的独立 root 下，隐藏到托盘时
    /// 整个 dispose——`COVERS` 清掉的只是 `Copy` 句柄，节点与它们存
    /// 的值要靠 dispose 才真正释放。
    pub(crate) static COVER_OWNER: RefCell<Option<Owner>> = const { RefCell::new(None) };
}

/// 应用更新的进度状态（「关于」页；检查 / 下载在后台线程跑）。
#[derive(Debug, Clone, Default, PartialEq)]
pub enum UpdateState {
    /// 尚未检查（或打包外运行不显示更新区）。
    #[default]
    Idle,
    /// 检查中。
    Checking,
    /// 已是最新。
    Current,
    /// 有新版本：版本号、说明、能否原地安装、产物地址。
    Available {
        version: String,
        notes: Option<String>,
        can_install: bool,
        url: String,
        can_download: bool,
    },
    /// 下载中（已收字节 / 总字节）。
    Downloading { received: u64, total: Option<u64> },
    /// 下好并安装完成，待重启。
    Ready { version: String },
    /// 失败。
    Error(String),
}
