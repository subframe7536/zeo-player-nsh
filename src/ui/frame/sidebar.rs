//! 侧栏：搜索框、曲库导航、歌单区、设置入口与底部状态。栏宽固定
//! [`theme::SIDEBAR`]，不随窗口宽度收缩；点击搜索框直接打开
//! command palette。

use nsh_core::prelude::*;

use super::super::components::icons::{Icon, icon};
use super::super::theme::{self, *};
use super::super::{Nav, store};

const LIBRARY: [(Nav, &str, Icon); 5] = [
    (Nav::Home, "首页", Icon::Home),
    (Nav::Library, "音乐库", Icon::Note),
    (Nav::Artists, "艺术家", Icon::Mic),
    (Nav::Albums, "专辑", Icon::Disc),
    (Nav::Metadata, "元数据", Icon::Edit),
];

/// 搜索框的高度。
const SEARCH_H: f32 = 28.0;

/// 侧栏里的一行（导航 / 设置 / 歌单共用外观）：图标 + 文字。`label`
/// 用 `SharedString`：导航是静态串，歌单是名字。
pub(crate) fn nav_row(
    glyph: Icon,
    label: SharedString,
    selected: impl Fn() -> bool + Copy + 'static,
    on_press: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    nsh_base::button()
        .w_full()
        .h(px(34.0))
        .flex_row()
        .items_center()
        // 图标列中心固定在 30 pt（10 外边距 + 10 行内边距 + 半个
        // 20 pt 图标格），与「曲库」小标题的左缘对齐。
        .pl(px(10.0))
        .pr(px(10.0))
        .gap(px(10.0))
        .rounded(px(8.0))
        .overflow_hidden()
        .accessibility_label(label.clone())
        .selected(selected)
        .selected_style(|st| st.bg(move || primary().opacity(0.12)))
        .hover(|st| st.bg(move || primary().opacity(0.08)))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| on_press())
        .child(
            view()
                .size(px(20.0))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .child(icon(glyph, 17.0, move || {
                    if selected() { primary() } else { sidebar_fg() }
                })),
        )
        .child(
            view().flex_1().min_w(px(0.0)).child(
                text(label)
                    .number_of_lines(1)
                    .text_size(px(14.0))
                    .line_height(px(14.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || {
                        if selected() {
                            theme::fg()
                        } else {
                            sidebar_fg()
                        }
                    }),
            ),
        )
}

/// 侧栏内容；点击搜索框直接打开 command palette。
fn body() -> View {
    let s = store::store();

    #[cfg(target_os = "macos")]
    let shortcut_hint = "⌘K";
    #[cfg(not(target_os = "macos"))]
    let shortcut_hint = "Ctrl K";

    // 搜索区：输入框外观的按钮（提示词与快捷键提示）；点击直接打开 command palette。
    let search_button = view()
        .w_full()
        .h(px(SEARCH_H))
        .flex_row()
        .items_center()
        .rounded(px(SEARCH_H / 2.0))
        .cursor(Cursor::Pointer)
        .role(AccessibilityRole::Button)
        .label("搜索")
        .on_press(move |_: &()| {
            s.search_open.set(true);
        })
        .border_1()
        .border_color(border())
        .bg(move || bg())
        .hover(|st| {
            st.bg(move || primary().opacity(0.06))
                .border_color(move || primary().opacity(0.3))
        })
        .active(|st| st.opacity(0.7))
        .child(
            view()
                .w(px(28.0))
                .h_full()
                .items_center()
                .justify_center()
                .child(icon(Icon::Search, 14.0, faint)),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .h_full()
                .flex_row()
                .items_center()
                .pr(px(8.0))
                .gap(px(6.0))
                .overflow_hidden()
                .child(
                    text("搜索...")
                        .flex_1()
                        .min_w(px(0.0))
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(faint()),
                )
                .child(
                    text(shortcut_hint)
                        .flex_shrink_0()
                        .text_size(px(10.0))
                        .line_height(px(10.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(faint())
                        .px(px(5.0))
                        .py(px(2.0))
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(border().opacity(0.5)),
                ),
        );

    let search_area = view()
        .px(px(10.0))
        .h(px(SEARCH_H))
        .overflow_hidden()
        .child(search_button);

    view()
        .flex_1()
        .min_h(px(0.0))
        .child(search_area)
        .child(
            view()
                .px(px(10.0))
                .gap(px(2.0))
                .child(
                    view().h(px(33.0)).child(
                        text("曲库")
                            .text_size(px(11.0))
                            .line_height(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .letter_spacing(px(0.6))
                            .text_color(faint())
                            .px(px(10.0))
                            .pt(px(16.0))
                            .pb(px(6.0)),
                    ),
                )
                .children(LIBRARY.iter().map(|&(target, label, glyph)| {
                    let nav = global::<Nav>();
                    nav_row(
                        glyph,
                        label.into(),
                        move || nav.get() == target,
                        move || {
                            s.search_open.set(false);
                            nav.set(target);
                        },
                    )
                    .into_element()
                })),
        )
        .child(sidebar_lower_section())
        .child(view().px(px(10.0)).pb(px(12.0)).child(nav_row(
            Icon::Settings,
            "设置".into(),
            move || s.settings.get(),
            move || s.settings.set(true),
        )))
}

fn sidebar_rescan_button() -> AnyElement {
    let s = store::store();
    let scanning = move || s.library.with(|l| l.progress.scanning);
    view()
        .size(px(22.0))
        .rounded(px(6.0))
        .items_center()
        .justify_center()
        .role(AccessibilityRole::Button)
        .label("重新扫描")
        .window_drag(false)
        .opacity(move || if scanning() { 0.4_f32 } else { 1.0_f32 })
        .hover(move |st| {
            if !scanning() {
                st.bg(primary().opacity(0.08))
            } else {
                st
            }
        })
        .active(move |st| if !scanning() { st.opacity(0.6) } else { st })
        .on_press(move |_: &()| {
            if !scanning() {
                store::scan();
            }
        })
        .child(icon(Icon::Refresh, 14.0, faint))
        .into_element()
}

fn sidebar_lower_section() -> View {
    let s = store::store();
    let nav = global::<Nav>();
    // 主视图变化时自动联动侧栏下半部
    effect(move || match nav.get() {
        Nav::Folders => store::set_sidebar_section(store::SidebarSection::Folders),
        Nav::Genres => store::set_sidebar_section(store::SidebarSection::Tags),
        Nav::Playlist(_) => store::set_sidebar_section(store::SidebarSection::Playlists),
        _ => {}
    });

    let trigger = view()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .px(px(10.0))
        .pt(px(16.0))
        .pb(px(6.0))
        .role(AccessibilityRole::Button)
        .label("切换视图")
        .cursor(Cursor::Pointer)
        .child(
            text(move || match s.sidebar_section.get() {
                store::SidebarSection::Playlists => "歌单",
                store::SidebarSection::Tags => "标签",
                store::SidebarSection::Folders => "文件夹",
            })
            .text_size(px(11.0))
            .line_height(px(11.0))
            .font_weight(FontWeight::SEMIBOLD)
            .letter_spacing(px(0.6))
            .text_color(faint()),
        )
        .child(icon(Icon::ChevronDown, 10.0, faint));

    let options = vec![
        super::super::components::form::SelectOption::new("歌单", move || {
            store::set_sidebar_section(store::SidebarSection::Playlists);
            let nav = global::<Nav>();
            if matches!(nav.get_untracked(), Nav::Folders | Nav::Genres) {
                if let Some(first) = s.playlists.with(|p| p.first().map(|pl| pl.id)) {
                    nav.set(Nav::Playlist(first));
                }
            }
        }),
        super::super::components::form::SelectOption::new("标签", move || {
            store::set_sidebar_section(store::SidebarSection::Tags);
            let nav = global::<Nav>();
            if nav.get_untracked() != Nav::Genres {
                nav.set(Nav::Genres);
            }
        }),
        super::super::components::form::SelectOption::new("文件夹", move || {
            store::set_sidebar_section(store::SidebarSection::Folders);
            let nav = global::<Nav>();
            if nav.get_untracked() != Nav::Folders {
                nav.set(Nav::Folders);
            }
        }),
    ];

    let current = move || match s.sidebar_section.get() {
        store::SidebarSection::Playlists => "歌单".to_owned(),
        store::SidebarSection::Tags => "标签".to_owned(),
        store::SidebarSection::Folders => "文件夹".to_owned(),
    };

    let select_el = super::super::components::form::select_with_trigger(
        current,
        "切换视图",
        options,
        116.0,
        Some(trigger),
    );

    let header_action = dynamic(move || match s.sidebar_section.get() {
        store::SidebarSection::Playlists => super::super::views::playlists::add_playlist_button(),
        store::SidebarSection::Folders => sidebar_rescan_button(),
        store::SidebarSection::Tags => view().into_element(),
    });

    // 小标题行：左侧是「歌单 / 标签 / 文件夹」的视图切换，右侧是该
    // 视图的快捷操作（新建歌单 / 重新扫描）。
    let header = view()
        .h(px(33.0))
        .flex_row()
        .items_center()
        .px(px(10.0))
        .child(view().flex_1().child(select_el))
        .child(view().pt(px(10.0)).child(header_action));

    let content = dynamic(move || match s.sidebar_section.get() {
        store::SidebarSection::Playlists => {
            super::super::views::playlists::sidebar_list().into_element()
        }
        store::SidebarSection::Tags => super::super::views::tags_sidebar_list().into_element(),
        store::SidebarSection::Folders => super::super::views::tree_sidebar_list().into_element(),
    });

    view().flex_1().min_h(px(0.0)).child(header).child(content)
}

pub(crate) fn sidebar() -> View {
    view()
        .w(px(SIDEBAR))
        .flex_shrink_0()
        .overflow_hidden()
        .bg(move || theme::sidebar())
        .transition_colors()
        .duration(300.0)
        .border_r_1()
        .border_r_color(sidebar_border())
        // 顶部让给标题栏（侧栏底色一直铺到窗口顶边，交通灯落在上面）。
        .child(view().h(move || px(top_inset())).flex_shrink_0())
        .child(body())
}
