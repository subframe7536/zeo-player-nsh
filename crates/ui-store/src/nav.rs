use super::*;

/// 侧栏的去处。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Nav {
    /// 首页：随机推荐、曲库与播放统计（默认启动页）。
    #[default]
    Home,
    /// 音乐库：显示全部曲目。
    Library,
    /// 元数据：全库曲目的标签表格，勾选后单改 / 批改标签与纯文件重命名。
    Metadata,
    Folders,
    Artists,
    Albums,
    Genres,
    /// 一个歌单（id）。
    Playlist(u64),
}

// 专辑网格（测量脚本滚动它）。生产构建里没有测量脚本，整项剔除。
#[cfg(any(debug_assertions, test))]
thread_local! {
    /// 专辑网格（测量脚本滚动它）。
    pub static BENCH_GRID: Cell<Option<nsh_core::element::ElementRef>> = const { Cell::new(None) };
}

/// 设置弹窗的分类（左栏）。选择在 `global` 信号里，关上重开时保留。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Pane {
    #[default]
    Appearance,
    Library,
    Playback,
    Lyrics,
    Covers,
    Window,
    Other,
    About,
}

impl Pane {
    /// 左栏分组（小标题 + 分类，不折叠）：通用是外观 / 曲库 / 播放 /
    /// 窗口；歌词、封面、其他这些描述曲目信息的归「元数据」。
    pub const GROUPS: [(&'static str, &'static [Pane]); 3] = [
        (
            "通用",
            &[
                Pane::Appearance,
                Pane::Library,
                Pane::Playback,
                Pane::Window,
            ],
        ),
        ("元数据", &[Pane::Lyrics, Pane::Covers, Pane::Other]),
        ("关于", &[Pane::About]),
    ];

    /// 全部分类（显示顺序）。
    pub fn all() -> impl Iterator<Item = Pane> {
        Pane::GROUPS
            .iter()
            .flat_map(|(_, panes)| panes.iter().copied())
    }

    /// 分类标题。
    pub fn title(self) -> &'static str {
        match self {
            Pane::Appearance => "外观",
            Pane::Library => "曲库",
            Pane::Playback => "播放",
            Pane::Lyrics => "歌词",
            Pane::Covers => "封面",
            Pane::Window => "窗口",
            Pane::Other => "其他",
            Pane::About => "关于",
        }
    }
}

/// 打开设置弹窗并选中 `pane`（侧栏「设置」只置 `Store::settings`，保留
/// 上次的选择；空曲库的「添加文件夹…」要落在曲库分类所以走这里）。
pub fn open_settings(pane: Pane) {
    global::<Pane>().set(pane);
    store().settings.set(true);
}
