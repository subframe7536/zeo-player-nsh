//! 「歌手 / 专辑 / 标签」页：曲库曲目按键分组，分组网格 + 详情列表
//! （详情页头与曲目列表是共享件）。

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use nsh_core::prelude::*;

use super::super::components::button::pill_button;
use super::super::components::cover::{cover, cover_with};
use super::super::components::icons::Icon;
use super::super::components::page::{empty_state, page_header};
use super::super::components::tracks::{Columns, track_columns};
use super::super::store::{cover_of, format_total, play_list, store};
use super::super::theme::*;
use super::{by_disc_track, compare, hero, no_library, total_duration, track_list};
use crate::config::TrackEntry;

/// 专辑详情：头部已有封面与专辑名。
const ALBUM_COLS: Columns = Columns {
    cover: false,
    album: false,
    size: false,
    format: false,
};

// --- 歌手 ---------------------------------------------------------------------

#[derive(Clone, PartialEq)]
struct Group {
    key: String,
    title: String,
    subtitle: String,
    year: Option<u32>,
    tracks: Rc<Vec<TrackEntry>>,
    cover: Option<String>,
}

impl Group {
    fn albums(&self) -> usize {
        self.tracks
            .iter()
            .map(|t| t.album.as_str())
            .collect::<HashSet<_>>()
            .len()
    }
}

fn group_by(
    tracks: &[TrackEntry],
    key: impl Fn(&TrackEntry) -> Vec<(String, String, String)>,
) -> Vec<Group> {
    let mut map: HashMap<String, (String, String, Vec<TrackEntry>)> = HashMap::new();
    for track in tracks {
        for (k, title, subtitle) in key(track) {
            map.entry(k)
                .or_insert_with(|| (title, subtitle, Vec::new()))
                .2
                .push(track.clone());
        }
    }
    let mut out: Vec<Group> = map
        .into_iter()
        .map(|(key, (title, subtitle, mut list))| {
            list.sort_by(|a, b| compare(&a.album, &b.album).then_with(|| by_disc_track(a, b)));
            let cover = list.iter().find(|t| t.has_art).map(|t| t.path.clone());
            let year = list.iter().filter_map(|t| t.year).max();
            Group {
                key,
                title,
                subtitle,
                year,
                tracks: Rc::new(list),
                cover,
            }
        })
        .collect();
    out.sort_by(|a, b| compare(&a.title, &b.title).then_with(|| compare(&a.subtitle, &b.subtitle)));
    out
}

/// 圆形头像：封面或首字。`size` 为 `None` 时铺满宽度、正方形。
fn avatar(
    cover: impl Fn() -> Option<String> + Copy + 'static,
    name: impl Fn() -> String + 'static,
    size: Option<f32>,
) -> View {
    let frame = view()
        .rounded_full()
        .overflow_hidden()
        .bg(muted())
        .items_center()
        .justify_center();
    let frame = match size {
        Some(size) => frame.size(px(size)),
        None => frame.w_full().aspect_square(),
    };
    let glyph = size.map_or(64.0, |s| s * 0.4);
    frame
        .child(
            text(move || name().chars().next().map(String::from).unwrap_or_default())
                .text_size(px(glyph))
                .line_height(px(glyph))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted_fg()),
        )
        .child(when(
            move || cover().and_then(|p| cover_of(&p)).is_some(),
            move || {
                image(move || cover().and_then(|p| cover_of(&p)).unwrap_or_default())
                    .absolute()
                    .inset(px(0.0))
                    .resize_mode(ResizeMode::Cover)
            },
        ))
}

#[component]
pub fn Artists() -> impl IntoElement {
    let s = store();
    let selected = signal(None::<String>);
    let groups = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(group_by(&tracks, |t| {
            vec![(t.artist.clone(), t.artist.clone(), String::new())]
        }))
    });
    view().flex_1().child(dynamic(move || {
        if s.tracks.with(|t| t.is_empty()) {
            return no_library().into_element();
        }
        match selected.get() {
            Some(key) => {
                let group = untracked(|| groups.with(|g| g.iter().find(|g| g.key == key).cloned()));
                let Some(group) = group else {
                    selected.set(None);
                    return view().into_element();
                };
                let items = computed({
                    let list = group.tracks.clone();
                    move || list.clone()
                });
                let cover_path = stored(group.cover.clone());
                let name = group.title.clone();
                let header = hero(
                    "歌手",
                    avatar(move || cover_path.get(), move || name.clone(), Some(168.0)),
                    group.title.clone(),
                    format!("{} 首 · {} 张专辑", group.tracks.len(), group.albums()),
                    format_total(total_duration(&group.tracks)),
                    Rc::new(move || selected.set(None)),
                    move || items.with_untracked(|v| play_list(v, 0)),
                )
                .child(track_columns(Columns::ALL).mx(px(-12.0)));
                view()
                    .flex_1()
                    .child(track_list(
                        items,
                        false,
                        Columns::ALL,
                        header.into_element(),
                    ))
                    .into_element()
            }
            None => {
                let count = computed(move || groups.with(|g| g.len()));
                let columns = move || {
                    let width = window().width.0 - sidebar_width() - 80.0;
                    ((width + 8.0) / (184.0 + 8.0)).floor().clamp(2.0, 10.0) as u32
                };
                let grid = virtual_list(count, move |at| {
                    let group = computed(move || groups.with(|g| g.get(at.index()).cloned()));
                    let cover = move || group.with(|g| g.as_ref().and_then(|g| g.cover.clone()));
                    let name = move || {
                        group.with(|g| g.as_ref().map(|g| g.title.clone()).unwrap_or_default())
                    };
                    nsh_base::button()
                        .p(px(8.0))
                        .gap(px(10.0))
                        .rounded(px(14.0))
                        .accessibility_label(move || SharedString::from(name()))
                        .hover(|st| st.bg(hover()))
                        .active(|st| st.opacity(0.75))
                        .on_press(move |_: &()| selected.set(group.get_untracked().map(|g| g.key)))
                        .child(avatar(cover, name, None))
                        .child(
                            view()
                                .gap(px(2.0))
                                .px(px(2.0))
                                .child(
                                    text(name)
                                        .number_of_lines(1)
                                        .text_size(px(15.0))
                                        .line_height(px(15.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(fg()),
                                )
                                .child(
                                    text(move || {
                                        group.with(|g| {
                                            g.as_ref()
                                                .map(|g| {
                                                    format!(
                                                        "{} 首 · {} 张专辑",
                                                        g.tracks.len(),
                                                        g.albums()
                                                    )
                                                })
                                                .unwrap_or_default()
                                        })
                                    })
                                    .number_of_lines(1)
                                    .text_size(px(13.0))
                                    .line_height(px(13.0))
                                    .text_color(muted_fg()),
                                ),
                        )
                })
                .columns(columns)
                .estimated_item_size(px(240.0))
                .content_padding_x(px(40.0))
                .content_padding_bottom(px(BAR_SPACE))
                .flex_1();
                view()
                    .flex_1()
                    .child(page_header(
                        || "歌手".into(),
                        move || format!("{} 位歌手", count.get()),
                        None,
                        None,
                    ))
                    .child(grid)
                    .into_element()
            }
        }
    }))
}

// --- 专辑 ---------------------------------------------------------------------

#[component]
pub fn Albums() -> impl IntoElement {
    let s = store();
    let selected = signal(None::<String>);
    let groups = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(group_by(&tracks, |t| {
            let artist = if t.album_artist.is_empty() {
                t.artist.clone()
            } else {
                t.album_artist.clone()
            };
            vec![(format!("{artist}\u{0}{}", t.album), t.album.clone(), artist)]
        }))
    });
    view().flex_1().child(dynamic(move || {
        if s.tracks.with(|t| t.is_empty()) {
            return no_library().into_element();
        }
        match selected.get() {
            Some(key) => {
                let group = untracked(|| groups.with(|g| g.iter().find(|g| g.key == key).cloned()));
                let Some(group) = group else {
                    selected.set(None);
                    return view().into_element();
                };
                let items = computed({
                    let list = group.tracks.clone();
                    move || list.clone()
                });
                let cover_path = group.cover.clone();
                let cover_src = computed(move || cover_path.as_deref().and_then(cover_of));
                let meta = [
                    group.year.map(|y| y.to_string()),
                    Some(format!("{} 首", group.tracks.len())),
                    Some(format_total(total_duration(&group.tracks))),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
                let header = hero(
                    "专辑",
                    cover(move || cover_src.get(), Some(200.0), 12.0),
                    group.title.clone(),
                    group.subtitle.clone(),
                    meta,
                    Rc::new(move || selected.set(None)),
                    move || items.with_untracked(|v| play_list(v, 0)),
                )
                .child(track_columns(ALBUM_COLS).mx(px(-12.0)));
                view()
                    .flex_1()
                    .child(track_list(items, true, ALBUM_COLS, header.into_element()))
                    .into_element()
            }
            None => {
                let count = computed(move || groups.with(|g| g.len()));
                let columns = move || {
                    let width = window().width.0 - sidebar_width() - 40.0;
                    ((width + 8.0) / (184.0 + 8.0)).floor().clamp(2.0, 10.0) as u32
                };
                let grid = virtual_list(count, move |at| {
                    let group = computed(move || groups.with(|g| g.get(at.index()).cloned()));
                    let field = move |f: fn(&Group) -> String| {
                        move || group.with(|g| g.as_ref().map(f).unwrap_or_default())
                    };
                    let cover_path =
                        move || group.with(|g| g.as_ref().and_then(|g| g.cover.clone()));
                    let title = field(|g| g.title.clone());
                    nsh_base::button()
                        .p(px(8.0))
                        .gap(px(10.0))
                        .rounded(px(14.0))
                        .accessibility_label(move || SharedString::from(title()))
                        .hover(|st| st.bg(hover()))
                        .active(|st| st.opacity(0.75))
                        .on_press(move |_: &()| selected.set(group.get_untracked().map(|g| g.key)))
                        .child(
                            cover(move || cover_path().and_then(|p| cover_of(&p)), None, 10.0)
                                .shadow_md(),
                        )
                        .child(
                            view()
                                .gap(px(2.0))
                                .px(px(2.0))
                                .child(
                                    text(field(|g| g.title.clone()))
                                        .number_of_lines(1)
                                        .text_size(px(15.0))
                                        .line_height(px(15.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(fg()),
                                )
                                .child(
                                    text(field(|g| g.subtitle.clone()))
                                        .number_of_lines(1)
                                        .text_size(px(13.0))
                                        .line_height(px(13.0))
                                        .text_color(muted_fg()),
                                ),
                        )
                })
                .columns(columns)
                .estimated_item_size(px(240.0))
                .content_padding_x(px(20.0))
                .content_padding_bottom(px(BAR_SPACE))
                .flex_1();
                let grid_ref = element_ref();
                super::super::BENCH_GRID.with(|g| g.set(Some(grid_ref)));
                let grid = grid.element_ref(&grid_ref);
                view()
                    .flex_1()
                    .child(page_header(
                        || "专辑".into(),
                        move || format!("{} 张专辑", count.get()),
                        None,
                        None,
                    ))
                    .child(grid)
                    .into_element()
            }
        }
    }))
}

// --- 标签 ---------------------------------------------------------------------

#[component]
pub fn Genres() -> impl IntoElement {
    let s = store();
    let selected = signal(None::<String>);
    let groups = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(group_by(&tracks, |t| {
            t.genres
                .iter()
                .map(|g| (g.clone(), g.clone(), String::new()))
                .collect()
        }))
    });
    view().flex_1().child(dynamic(move || {
        if s.tracks.with(|t| t.is_empty()) {
            return no_library().into_element();
        }
        match selected.get() {
            Some(key) => {
                let group = untracked(|| groups.with(|g| g.iter().find(|g| g.key == key).cloned()));
                let Some(group) = group else {
                    selected.set(None);
                    return view().into_element();
                };
                let items = computed({
                    let list = group.tracks.clone();
                    move || list.clone()
                });
                let title = format!("# {}", group.title);
                let subtitle = format!(
                    "{} 首 · {}",
                    group.tracks.len(),
                    format_total(total_duration(&group.tracks))
                );
                let header = view()
                    .child(page_header(
                        move || title.clone(),
                        move || subtitle.clone(),
                        Some(Rc::new(move || selected.set(None))),
                        Some(
                            pill_button(Icon::Play, "播放", move || {
                                items.with_untracked(|v| play_list(v, 0))
                            })
                            .into_element(),
                        ),
                    ))
                    .child(track_columns(Columns::ALL));
                view()
                    .flex_1()
                    .child(track_list(
                        items,
                        false,
                        Columns::ALL,
                        header.into_element(),
                    ))
                    .into_element()
            }
            None => {
                let list = groups.get_untracked();
                let total = list.len();
                if total == 0 {
                    return empty_state(
                        Icon::Tag,
                        "没有流派标签",
                        "曲目的流派标签会出现在这里（按 ; 拆分）。",
                    )
                    .into_element();
                }
                let chips = list.iter().map(|g| {
                    let key = g.key.clone();
                    let art = stored(g.cover.clone());
                    nsh_base::button()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .h(px(44.0))
                        .pl(px(6.0))
                        .pr(px(16.0))
                        .rounded_full()
                        .bg(muted())
                        .accessibility_label(g.title.clone())
                        .hover(|st| st.bg(hover()))
                        .active(|st| st.opacity(0.7))
                        .on_press(move |_: &()| selected.set(Some(key.clone())))
                        .child(cover_with(
                            move || art.get().and_then(|p| cover_of(&p)),
                            32.0,
                            16.0,
                            Icon::Tag,
                        ))
                        .child(
                            text(g.title.clone())
                                .text_size(px(14.0))
                                .line_height(px(14.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(fg()),
                        )
                        .child(
                            text(g.tracks.len().to_string())
                                .text_size(px(12.0))
                                .line_height(px(12.0))
                                .tabular_nums()
                                .text_color(faint()),
                        )
                });
                view()
                    .flex_1()
                    .child(page_header(
                        || "标签".into(),
                        move || format!("{total} 个流派"),
                        None,
                        None,
                    ))
                    .child(
                        scroll().flex_1().child(
                            view()
                                .flex_row()
                                .flex_wrap()
                                .gap(px(10.0))
                                .px(px(32.0))
                                .pt(px(4.0))
                                .pb(px(BAR_SPACE))
                                .children(chips),
                        ),
                    )
                    .into_element()
            }
        }
    }))
}
