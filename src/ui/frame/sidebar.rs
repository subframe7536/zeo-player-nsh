//! 侧栏：搜索框、曲库导航、歌单区、设置入口与底部状态。窗口窄于
//! [`theme::SIDEBAR_COLLAPSE`] 时收成只有图标的栏：结构不换，只是把
//! 文字和搜索框藏起、放大镜浮在同一槽位，行与图标的位置都不动——
//! 栏宽收窄后图标列正好居中。图标栏里点放大镜直接切到搜索页（页顶
//! 的输入框自动聚焦）。

use nsh_core::element::ElementRef;
use nsh_core::element::input;
use nsh_core::prelude::*;
use nsh_core::{TouchEvent, TouchPhase};

use super::super::components::button::icon_button;
use super::super::components::icons::{Icon, icon};
use super::super::theme::{self, *};
use super::super::{Nav, store};

const LIBRARY: [(Nav, &str, Icon); 3] = [
    (Nav::Home, "首页", Icon::Home),
    (Nav::Artists, "歌手", Icon::Mic),
    (Nav::Albums, "专辑", Icon::Disc),
];

/// 搜索框的焦点：点在框外时让它失焦。
#[derive(Clone, Copy)]
pub(crate) struct SearchFocus {
    input: ElementRef,
    focused: Signal<bool>,
    /// 这次按下落在搜索框里（由搜索框自己的触摸处理记下）。
    inside: Signal<bool>,
}

impl SearchFocus {
    pub(crate) fn new() -> Self {
        SearchFocus {
            input: element_ref(),
            focused: signal(false),
            inside: signal(false),
        }
    }

    pub(crate) fn outside_touch(self, e: &TouchEvent) {
        if e.phase != TouchPhase::Start {
            return;
        }
        // 同一次按下，搜索框与根视图都收到触摸，先后不定：等这次事件派发
        // 完再看它是否落在框里。
        spawn(async move {
            let inside = self.inside.get_untracked();
            self.inside.set(false);
            if !inside && self.focused.get_untracked() {
                self.input.blur();
            }
        });
    }
}

/// 搜索框的高度。
const SEARCH_H: f32 = 28.0;

/// 搜索框：侧栏顶部（小）与搜索页页顶（`large` 大）共用。
/// `auto_focus` 给展开挂载用。
pub(crate) fn search_box(focus: SearchFocus, auto_focus: bool, large: bool) -> View {
    let s = store::store();
    let query_empty = move || s.search.with(|q| q.is_empty());
    let h = if large { 44.0 } else { SEARCH_H };
    // 外框画边框与底色，里面一行：搜索图标、输入框（透明）、清除按钮。
    // 原生输入框画在应用内容之上，图标不能叠在它下面。
    view()
        .h(px(h))
        .flex_row()
        .items_center()
        .pl(px(if large { 14.0 } else { 10.0 }))
        .pr(px(if large { 4.0 } else { 1.0 }))
        .rounded(px(h / 2.0))
        .border_1()
        .border_color(border())
        .bg(move || bg())
        .transition_colors()
        .duration(300.0)
        .on_touch(move |e: &TouchEvent| {
            if e.phase == TouchPhase::Start {
                focus.inside.set(true);
            }
        })
        // 点在图标上也聚焦输入框。
        .on_press(move || focus.input.focus())
        .child(icon(Icon::Search, if large { 16.0 } else { 13.0 }, faint).flex_shrink_0())
        .child(
            input()
                .element_ref(&focus.input)
                .auto_focus(auto_focus)
                .flex_1()
                .min_w(px(0.0))
                .h(px(h - 2.0))
                .px(px(if large { 10.0 } else { 6.0 }))
                .text_size(px(if large { 15.0 } else { 13.0 }))
                .text_color(fg())
                .value(move || s.search.get())
                .placeholder("搜索")
                .label("搜索")
                .on_focus(move || focus.focused.set(true))
                .on_blur(move || focus.focused.set(false))
                .on_change(move |t: &str| s.search.set(t.to_owned())),
        )
        .child(when(
            move || !query_empty(),
            move || {
                icon_button(
                    || Icon::Close,
                    if large { 12.0 } else { 10.0 },
                    "清除搜索",
                    faint,
                    || store::store().search.set(String::new()),
                )
                .flex_shrink_0()
            },
        ))
}

/// 图标栏模式下文字透明（布局不变，图标留在原列）。
pub(crate) fn label_fade(rail: impl Fn() -> bool + Copy + 'static) -> impl Fn() -> f32 + Copy {
    move || if rail() { 0.0 } else { 1.0 }
}

/// 侧栏里的一行（导航 / 设置 / 歌单共用外观）：图标 + 文字；图标栏时
/// 只隐藏文字，其余不动。`label` 用 `SharedString`：导航是静态串，歌单是名字。
pub(crate) fn adaptive_row(
    rail: impl Fn() -> bool + Copy + 'static,
    glyph: Icon,
    label: SharedString,
    selected: impl Fn() -> bool + Copy + 'static,
    on_press: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    let fade = label_fade(rail);
    nsh_base::button()
        .w_full()
        .h(px(34.0))
        .flex_row()
        .items_center()
        // 图标列中心固定在 30 pt：展开态与「曲库」标签对齐；图标栏宽
        // 60 pt 时正好居中（`SIDEBAR_RAIL` 就是按这个列定的）。
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
            view().flex_1().min_w(px(0.0)).opacity(fade).child(
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

/// 侧栏内容：展开与图标栏共用同一结构；图标栏时只把文字与搜索框
/// 藏起（布局不变），放大镜在同一槽位浮出来。
fn adaptive_body(search: SearchFocus, search_page: SearchFocus) -> View {
    let s = store::store();
    let rail = sidebar_collapsed;
    let fade = label_fade(rail);
    // 搜索区：展开态是输入框，图标栏是放大镜（切到搜索页）；两者在同
    // 一槽位，输入框隐藏后把指针事件也关掉。
    let search_area = view().px(px(10.0)).child(
        view()
            .h(px(SEARCH_H))
            .child(
                search_box(search, false, false)
                    .opacity(fade)
                    .pointer_events(move || {
                        if rail() {
                            PointerEvents::None
                        } else {
                            PointerEvents::Auto
                        }
                    }),
            )
            .child(
                view()
                    .absolute()
                    .inset(px(0.0))
                    .items_center()
                    .justify_center()
                    .opacity(move || if rail() { 1.0_f32 } else { 0.0 })
                    // 展开态透明但仍盖着搜索框：指针事件也要关掉。
                    .pointer_events(move || {
                        if rail() {
                            PointerEvents::Auto
                        } else {
                            PointerEvents::None
                        }
                    })
                    // 栏的 overflow_hidden 会裁掉 tooltip，这里不挂提示。
                    .child(
                        view()
                            .size(px(28.0))
                            .items_center()
                            .justify_center()
                            .rounded(px(8.0))
                            .role(AccessibilityRole::Button)
                            .label("搜索")
                            .hover(|st| st.bg(move || primary().opacity(0.08)))
                            .active(|st| st.opacity(0.7))
                            .on_press(move |_: &()| {
                                s.search_open.set(true);
                                // 等搜索页这一帧挂出来再聚焦页顶的输入框。
                                spawn(async move {
                                    let _ = next_frame().await;
                                    search_page.input.focus();
                                });
                            })
                            .child(icon(Icon::Search, 16.0, sidebar_fg)),
                    ),
            ),
    );
    view()
        .flex_1()
        .min_h(px(0.0))
        .child(search_area)
        .child(
            view()
                .px(px(10.0))
                .gap(px(2.0))
                .child(
                    view()
                        .h(move || px(if rail() { 0.0 } else { 33.0 }))
                        .overflow_hidden()
                        .opacity(fade)
                        .child(
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
                    adaptive_row(
                        rail,
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
        .child(sidebar_lower_section(rail))
        .child(
            view()
                .px(px(10.0))
                .pb(px(12.0))
                .gap(px(2.0))
                .child(adaptive_row(
                    rail,
                    Icon::Settings,
                    "设置".into(),
                    move || s.settings.get(),
                    move || s.settings.set(true),
                ))
                .child(
                    // 文字藏起时高度也收掉：图标栏里设置行落到底部。
                    view()
                        .h(move || px(if rail() { 0.0 } else { 20.0 }))
                        .overflow_hidden()
                        .opacity(fade)
                        .child(
                            text(move || {
                                s.library.with(|l| {
                                    if l.progress.scanning {
                                        "正在扫描曲库…".to_owned()
                                    } else {
                                        format!("{} 首曲目", s.tracks.with(|t| t.len()))
                                    }
                                })
                            })
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .tabular_nums()
                            .text_color(faint())
                            .px(px(10.0))
                            .pt(px(8.0)),
                        ),
                ),
        )
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

fn sidebar_lower_section(rail: impl Fn() -> bool + Copy + 'static) -> View {
    let s = store::store();
    let fade = label_fade(rail);
    let nav = global::<Nav>();
    // 主视图变化时自动联动侧栏下半部
    effect(move || match nav.get() {
        Nav::Folders => s.sidebar_section.set(store::SidebarSection::Folders),
        Nav::Genres => s.sidebar_section.set(store::SidebarSection::Tags),
        Nav::Playlist(_) => s.sidebar_section.set(store::SidebarSection::Playlists),
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
            s.sidebar_section.set(store::SidebarSection::Playlists);
            let nav = global::<Nav>();
            if matches!(nav.get_untracked(), Nav::Folders | Nav::Genres) {
                if let Some(first) = s.playlists.with(|p| p.first().map(|pl| pl.id)) {
                    nav.set(Nav::Playlist(first));
                }
            }
        })
        .with_glyph(Icon::Playlist),
        super::super::components::form::SelectOption::new("标签", move || {
            s.sidebar_section.set(store::SidebarSection::Tags);
            let nav = global::<Nav>();
            if nav.get_untracked() != Nav::Genres {
                nav.set(Nav::Genres);
            }
        })
        .with_glyph(Icon::Tag),
        super::super::components::form::SelectOption::new("文件夹", move || {
            s.sidebar_section.set(store::SidebarSection::Folders);
            let nav = global::<Nav>();
            if nav.get_untracked() != Nav::Folders {
                nav.set(Nav::Folders);
            }
        })
        .with_glyph(Icon::Folder),
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

    let header = view()
        .h(move || px(if rail() { 0.0 } else { 33.0 }))
        .overflow_hidden()
        .opacity(fade)
        .flex_row()
        .items_center()
        .px(px(10.0))
        .child(view().flex_1().child(select_el))
        .child(view().pt(px(10.0)).child(header_action));

    let content = dynamic(move || match s.sidebar_section.get() {
        store::SidebarSection::Playlists => {
            super::super::views::playlists::sidebar_list(rail).into_element()
        }
        store::SidebarSection::Tags => super::super::views::tags_sidebar_list(rail).into_element(),
        store::SidebarSection::Folders => {
            super::super::views::tree_sidebar_list(rail).into_element()
        }
    });

    view().flex_1().min_h(px(0.0)).child(header).child(content)
}

pub(crate) fn sidebar(search: SearchFocus, search_page: SearchFocus) -> View {
    view()
        .w(move || px(sidebar_width()))
        .flex_shrink_0()
        .overflow_hidden()
        .bg(move || theme::sidebar())
        .transition_colors()
        .duration(300.0)
        .border_r_1()
        .border_r_color(sidebar_border())
        // 顶部让给标题栏（侧栏底色一直铺到窗口顶边，交通灯落在上面）。
        .child(view().h(move || px(top_inset())).flex_shrink_0())
        .child(adaptive_body(search, search_page))
}
