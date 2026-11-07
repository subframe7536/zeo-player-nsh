//! 「设置」弹窗：左栏是搜索框与分类，右栏是所选分类的各节（曲库 / 播放 /
//! 歌词 / 窗口 / 标签 / 外观）。各节无卡片包装，靠间距分开。

mod appearance;
mod covers;
mod library;
mod lyrics;
mod playback;
mod tags;
mod window;

use nsh_core::element::input;
use nsh_core::prelude::*;

use super::super::components::button::icon_button;
use super::super::components::icons::{Icon, icon};
use super::super::store::store;
use super::super::theme::*;

/// 设置里的一节：标题、说明、右上角操作，然后是内容（无卡片包装，
/// 节与节之间靠页面间距分开）。
fn setting_section(title: &'static str, body: &'static str, action: Option<AnyElement>) -> View {
    view().w_full().gap(px(16.0)).child(
        view()
            .flex_row()
            .flex_wrap()
            .items_start()
            .gap(px(16.0))
            .child(
                view()
                    .flex_1()
                    .min_w(px(240.0))
                    .gap(px(4.0))
                    .child(
                        text(title)
                            .text_size(px(15.0))
                            .line_height(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg()),
                    )
                    .child(text(body).text_size(px(13.0)).text_color(muted_fg())),
            )
            .child(action.map(|a| view().flex_row().gap(px(8.0)).child(a))),
    )
}

/// 一行说明文字（操作结果）。
fn note(message: Signal<String>) -> AnyElement {
    dynamic(move || {
        let now = message.get();
        (!now.is_empty()).then(|| {
            text(now)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg())
        })
    })
}

/// 「本地优先」开关行：标题 + 说明 + 右侧开关。
fn pref_row(
    title: &'static str,
    desc: &'static str,
    checked: impl Fn() -> bool + 'static,
    set: fn(bool),
) -> View {
    let toggle = nsh_base::switch()
        .checked(checked)
        .accessibility_label(title)
        .on_change(move |next: &bool| set(*next))
        .track_style(|st| {
            st.w(px(42.0))
                .h(px(24.0))
                .p(px(2.0))
                .rounded_full()
                .bg(muted())
                .border_1()
                .border_color(border())
                .transition_colors()
                .duration(150.0)
                .checked(|st| st.bg(move || primary()).border_color(move || primary()))
        })
        .thumb_style(|st| {
            st.size(px(18.0))
                .rounded_full()
                .bg(AdaptiveColor::rgb(0xffffff, 0xffffff))
                .shadow_sm()
                .transition_transform()
                .duration(150.0)
                .checked(|st| st.translate_x(px(18.0)))
        });
    view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(16.0))
        .child(
            view()
                .flex_1()
                .min_w(px(200.0))
                .gap(px(2.0))
                .child(
                    text(title)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg()),
                )
                .child(
                    text(desc)
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(toggle)
}

// --- 弹窗 -------------------------------------------------------------------

/// 设置的分类（左栏）。选择在 `global` 信号里，关上重开时保留。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Pane {
    #[default]
    Appearance,
    Library,
    Playback,
    Lyrics,
    Covers,
    Window,
    Tags,
}

impl Pane {
    /// 左栏分组（小标题 + 分类，不折叠）：通用是外观 / 曲库 / 播放 /
    /// 窗口；歌词、封面、标签这些描述曲目信息的归「元数据」。
    const GROUPS: [(&'static str, &'static [Pane]); 2] = [
        (
            "通用",
            &[
                Pane::Appearance,
                Pane::Library,
                Pane::Playback,
                Pane::Window,
            ],
        ),
        ("元数据", &[Pane::Lyrics, Pane::Covers, Pane::Tags]),
    ];

    /// 全部分类（显示顺序）。
    fn all() -> impl Iterator<Item = Pane> {
        Pane::GROUPS
            .iter()
            .flat_map(|(_, panes)| panes.iter().copied())
    }

    fn title(self) -> &'static str {
        match self {
            Pane::Appearance => "外观",
            Pane::Library => "曲库",
            Pane::Playback => "播放",
            Pane::Lyrics => "歌词",
            Pane::Covers => "封面",
            Pane::Window => "窗口",
            Pane::Tags => "标签",
        }
    }

    fn glyph(self) -> Icon {
        match self {
            Pane::Library => Icon::Folder,
            Pane::Playback => Icon::Play,
            Pane::Lyrics => Icon::Note,
            Pane::Covers => Icon::Disc,
            Pane::Window => Icon::Monitor,
            Pane::Tags => Icon::Tag,
            Pane::Appearance => Icon::Sun,
        }
    }
}

/// 一节设置与它的搜索词：左栏搜索按 `title` / `keys` 命中（分类名命中时
/// 该分类的节全部算命中）。
struct Section {
    pane: Pane,
    title: &'static str,
    keys: &'static str,
    build: fn() -> View,
}

const SECTIONS: &[Section] = &[
    Section {
        pane: Pane::Library,
        title: "音乐文件夹",
        keys: "添加 文件夹 目录 拖放 根目录 mp3 flac wav m4a aac alac ogg aiff",
        build: library::folders_section,
    },
    Section {
        pane: Pane::Library,
        title: "曲库",
        keys: "扫描更新 重建曲库 索引 监视 文件夹变化 自动 统计 曲目 专辑 歌手 总时长 缓存",
        build: library::library_section,
    },
    Section {
        pane: Pane::Library,
        title: "音频过滤",
        keys: "过滤 阈值 时长 大小 短音频 小文件 秒 KB 采样 铃声",
        build: library::filter_section,
    },
    Section {
        pane: Pane::Library,
        title: "删除文件",
        keys: "删除 文件 回收站 永久删除 直接删除 曲目 右键 菜单",
        build: library::delete_section,
    },
    Section {
        pane: Pane::Covers,
        title: "封面来源",
        keys: "封面 图片 本地 同名 内嵌 优先 来源 cover jpg jpeg png webp 专辑",
        build: covers::covers_source_section,
    },
    Section {
        pane: Pane::Playback,
        title: "淡入淡出",
        keys: "播放 音量 渐变 切歌 暂停 毫秒 时长 无缝",
        build: playback::playback_section,
    },
    Section {
        pane: Pane::Playback,
        title: "播放统计",
        keys: "统计 播放次数 首页 排行 阈值 百分比 秒 听过",
        build: playback::stats_section,
    },
    Section {
        pane: Pane::Playback,
        title: "输出设备",
        keys: "输出 设备 耳机 拔出 暂停 默认设备 声卡 蓝牙",
        build: playback::device_section,
    },
    Section {
        pane: Pane::Playback,
        title: "ReplayGain",
        keys: "replaygain 回放增益 音量 标准化 响度 专辑 曲目 预放大 削波 峰值",
        build: playback::replaygain_section,
    },
    Section {
        pane: Pane::Playback,
        title: "均衡器",
        keys: "均衡器 eq equalizer 频段 低音 高音 预设 流行 摇滚 爵士 古典 人声",
        build: playback::eq_section,
    },
    Section {
        pane: Pane::Lyrics,
        title: "歌词对齐",
        keys: "歌词 全屏 左对齐 居中 右对齐 对齐方式",
        build: lyrics::lyrics_section,
    },
    Section {
        pane: Pane::Lyrics,
        title: "歌词行时间戳",
        keys: "歌词 时间戳 行 悬停 时间 跳转 timestamp",
        build: lyrics::lyrics_timestamps_section,
    },
    Section {
        pane: Pane::Lyrics,
        title: "全局歌词偏移",
        keys: "歌词 偏移 全局 提前 延后 同步 offset 蓝牙 延迟",
        build: lyrics::lyrics_offset_section,
    },
    Section {
        pane: Pane::Lyrics,
        title: "歌词来源",
        keys: "歌词 本地 lrc 同名 内嵌 优先 来源",
        build: lyrics::lyrics_source_section,
    },
    Section {
        pane: Pane::Window,
        title: "关闭窗口时",
        keys: "关闭 窗口 最小化 托盘 退出 询问 记住 大小 位置 恢复",
        build: window::window_section,
    },
    #[cfg(target_os = "macos")]
    Section {
        pane: Pane::Window,
        title: "菜单栏",
        keys: "菜单栏 状态栏 托盘 曲名 标题 图标 menubar tray title",
        build: window::menubar_section,
    },
    Section {
        pane: Pane::Tags,
        title: "标签",
        keys: "流派 标签 分隔符 分号 顿号 斜杠 拆分",
        build: tags::genres_section,
    },
    Section {
        pane: Pane::Appearance,
        title: "外观",
        keys: "浅色 深色 系统 主题 颜色 跟随",
        build: appearance::appearance_section,
    },
    Section {
        pane: Pane::Appearance,
        title: "主题色",
        keys: "主题色 颜色 强调色 黑白 红 橙 绿 蓝 紫 封面 自动",
        build: appearance::accent_section,
    },
    Section {
        pane: Pane::Appearance,
        title: "字体",
        keys: "字体 系统字体 已安装 预览 字形",
        build: appearance::font_section,
    },
];

/// 搜索词是否命中一段文本（调用方已把 `q` 转成小写并去掉两端空格）。
fn matches(q: &str, s: &str) -> bool {
    s.to_lowercase().contains(q)
}

/// 分类命中：名字匹配，或它任一节的标题 / 关键词匹配。
fn pane_hits(q: &str, p: Pane) -> bool {
    matches(q, p.title())
        || SECTIONS
            .iter()
            .any(|s| s.pane == p && (matches(q, s.title) || matches(q, s.keys)))
}

/// 一节在右栏显示：分类名命中时整节显示，否则按自身标题 / 关键词。
fn section_hits(q: &str, s: &Section) -> bool {
    matches(q, s.pane.title()) || matches(q, s.title) || matches(q, s.keys)
}

/// 左栏顶部的搜索框（外观同主侧栏的搜索框；有词时出现清空按钮）。
fn search_box(query: Signal<String>) -> View {
    let empty = move || query.with(String::is_empty);
    view()
        .h(px(28.0))
        .flex_row()
        .items_center()
        .pl(px(10.0))
        .pr(px(1.0))
        .rounded(px(14.0))
        .border_1()
        .border_color(border())
        .bg(move || bg())
        .child(icon(Icon::Search, 13.0, faint).flex_shrink_0())
        .child(
            input()
                .flex_1()
                .min_w(px(0.0))
                .h(px(26.0))
                .px(px(6.0))
                .text_size(px(13.0))
                .text_color(fg())
                .value(move || query.get())
                .placeholder("搜索")
                .label("搜索设置")
                .on_change(move |t: &str| query.set(t.to_owned())),
        )
        .child(when(
            move || !empty(),
            move || {
                icon_button(
                    || Icon::Close,
                    10.0,
                    "清除搜索",
                    faint,
                    move || query.set(String::new()),
                )
                .flex_shrink_0()
            },
        ))
}

/// 左栏分类行（外观同主侧栏的导航行）。
fn pane_row(p: Pane, pane: Signal<Pane>, rail: bool) -> AnyElement {
    let active = move || pane.get() == p;
    let b = nsh_base::button()
        .rounded(px(8.0))
        .accessibility_label(p.title())
        .selected(active)
        .selected_style(|st| st.bg(move || primary().opacity(0.12)))
        .hover(|st| st.bg(move || primary().opacity(0.08)))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| pane.set(p));
    if rail {
        // tipped 的包装在列里会拉满宽、按钮贴左；再套一层居中让它回到栏中线。
        return view()
            .w_full()
            .items_center()
            .child(super::super::components::button::tipped(
                b.size(px(40.0)).items_center().justify_center().child(icon(
                    p.glyph(),
                    17.0,
                    move || {
                        if active() { primary() } else { sidebar_fg() }
                    },
                )),
                p.title(),
            ))
            .into_element();
    }
    b.h(px(32.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(10.0))
        .w_full()
        .child(icon(p.glyph(), 15.0, move || {
            if active() { primary() } else { sidebar_fg() }
        }))
        .child(
            text(p.title())
                .text_size(px(13.0))
                .line_height(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if active() { fg() } else { sidebar_fg() }),
        )
        .into_element()
}

/// 搜索结果行：节的标题 + 所属分类，按下选中该分类（保留搜索词，右栏
/// 只显示命中的节）。
fn result_row(s: &'static Section, pane: Signal<Pane>) -> nsh_base::ButtonElement {
    nsh_base::button()
        .h(px(40.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(10.0))
        .rounded(px(8.0))
        .accessibility_label(s.title)
        .hover(|st| st.bg(move || primary().opacity(0.08)))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| pane.set(s.pane))
        .child(icon(s.pane.glyph(), 15.0, sidebar_fg))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(1.0))
                .child(
                    text(s.title)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(sidebar_fg()),
                )
                .child(
                    text(s.pane.title())
                        .text_size(px(11.0))
                        .line_height(px(11.0))
                        .text_color(faint()),
                ),
        )
}

/// 左栏分组的小标题（首个与搜索框之间不留大间隔，其余组间拉开）。
fn group_heading(name: &'static str, spaced: bool) -> View {
    view()
        .px(px(10.0))
        .pt(if spaced { px(18.0) } else { px(4.0) })
        .pb(px(4.0))
        .child(
            text(name)
                .text_size(px(11.0))
                .line_height(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(faint()),
        )
}

/// 左栏：顶部搜索框，下面是分类列表（有搜索词时为命中的节列表）。
/// 窗口窄时收成图标栏：搜索框与标题隐藏，分类只剩图标（悬停提示里看名字）。
fn nav_column(pane: Signal<Pane>, query: Signal<String>) -> View {
    let rail = move || window().width.0 < 760.0;
    let rows = dynamic(move || {
        let rail = rail();
        let q = if rail {
            String::new()
        } else {
            query.get().trim().to_lowercase()
        };
        if q.is_empty() {
            return Pane::GROUPS
                .iter()
                .enumerate()
                .flat_map(|(i, &(name, panes))| {
                    let heading = (!rail).then(|| group_heading(name, i > 0).into_element());
                    heading
                        .into_iter()
                        .chain(panes.iter().map(|&p| pane_row(p, pane, rail)))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
        }
        SECTIONS
            .iter()
            .filter(|s| section_hits(&q, s))
            .map(|s| result_row(s, pane).into_element())
            .collect()
    });
    view()
        .w(move || px(if rail() { 64.0 } else { 220.0 }))
        .flex_shrink_0()
        .bg(move || sidebar())
        .transition_colors()
        .duration(300.0)
        .test_id("settings-nav")
        .child(when(
            move || !rail(),
            move || view().p(px(10.0)).child(search_box(query)),
        ))
        .child(
            scroll().flex_1().child(
                view()
                    .px(px(10.0))
                    // 图标栏没有搜索框，顶部留同样的间距。
                    .pt(move || px(if rail() { 10.0 } else { 0.0 }))
                    .pb(px(10.0))
                    .gap(px(2.0))
                    .style_when(rail, |st| st.items_center())
                    .child(rows),
            ),
        )
}

/// 右栏：当前分类的标题与关闭按钮，下面是它的各节；有搜索词时只显示
/// 命中的节，所选分类没命中时落到第一个有命中的分类。
fn content_column(pane: Signal<Pane>, query: Signal<String>) -> View {
    let s = store();
    let shown = move || {
        let q = query.get().trim().to_lowercase();
        let hits: Vec<Pane> = Pane::all().filter(|p| pane_hits(&q, *p)).collect();
        let cur = pane.get();
        hits.iter()
            .find(|&&p| p == cur)
            .or(hits.first())
            .copied()
            .map(|p| (p, q))
    };
    view()
        .flex_1()
        .min_w(px(0.0))
        .test_id("settings-content")
        .child(dynamic(move || {
            let Some((p, q)) = shown() else {
                return view()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .p(px(32.0))
                    .child(icon(Icon::Search, 24.0, faint))
                    .child(
                        text("没有匹配的设置")
                            .text_size(px(14.0))
                            .line_height(px(14.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg()),
                    )
                    .child(
                        text("换个关键词，或清空搜索看全部分类。")
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .text_color(muted_fg()),
                    )
                    .into_element();
            };
            // 节的构建放进 untracked：build 里有顺手读信号初始化草稿的
            // （如 playback 的 fade_ms / replaygain 的 db_row），被 dynamic
            // 记成依赖后任何 `s.library` 写都会整栏重建——拖动中的推子节点
            // 被换掉、responder 随之丢失（均衡器推子曾因此在滚动区里拖不动）。
            untracked(move || {
                view()
                    .flex_1()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .h(px(52.0))
                            .pl(px(24.0))
                            .pr(px(14.0))
                            .border_b_1()
                            .border_b_color(border())
                            .child(
                                text(p.title())
                                    .text_size(px(15.0))
                                    .line_height(px(15.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(fg()),
                            )
                            .child(view().flex_1())
                            .child(icon_button(
                                || Icon::Close,
                                14.0,
                                "关闭设置",
                                muted_fg,
                                move || s.settings.set(false),
                            )),
                    )
                    .child(
                        scroll().flex_1().child(
                            view()
                                .px(px(24.0))
                                .pt(px(20.0))
                                .pb(px(28.0))
                                .gap(px(36.0))
                                .children(
                                    SECTIONS
                                        .iter()
                                        .filter(|sec| sec.pane == p && section_hits(&q, sec))
                                        .map(|sec| (sec.build)()),
                                ),
                        ),
                    )
                    .into_element()
            })
        }))
}

/// 设置弹窗（nsh-base `dialog`，Esc / 点遮罩关闭）。
pub(crate) fn dialog() -> AnyElement {
    let pane = global::<Pane>();
    let query = signal(String::new());
    let content = view()
        .w(px(920.0))
        .max_w(move || px((window().width.0 - 32.0).max(400.0)))
        .h(px(640.0))
        .max_h(move || px((window().height.0 - 60.0).max(400.0)))
        .flex_row()
        .overflow_hidden()
        .rounded(px(super::super::components::form::RADIUS_LG))
        .border_1()
        .border_color(border())
        .bg(move || bg())
        .transition_colors()
        .duration(300.0)
        .shadow_lg()
        // 点在内容上不会穿到背后的遮罩。
        .on_press(|| {})
        .child(nav_column(pane, query))
        .child(view().w(px(1.0)).bg(border()).flex_shrink_0())
        .child(content_column(pane, query));
    super::super::components::form::modal(content, move || store().settings.set(false))
}

/// 打开设置弹窗并选中 `pane`（侧栏「设置」只置 `Store::settings`，保留
/// 上次的选择；空曲库的「添加文件夹…」要落在曲库分类所以走这里）。
pub(super) fn open(pane: Pane) {
    global::<Pane>().set(pane);
    store().settings.set(true);
}

#[cfg(test)]
mod tests {
    use super::super::{last_segment, url_to_path};

    #[test]
    fn file_urls_decode() {
        assert_eq!(
            url_to_path("file:///Users/me/My%20Music/"),
            "/Users/me/My Music/"
        );
        assert_eq!(url_to_path("file:///tmp/%E9%9F%B3%E4%B9%90"), "/tmp/音乐");
        assert_eq!(
            url_to_path("file:///C:/Users/me/My%20Music"),
            "C:\\Users\\me\\My Music"
        );
        assert_eq!(last_segment("C:\\Users\\me\\Music\\"), "Music");
        assert_eq!(last_segment("/Users/me/Music"), "Music");
    }
}
