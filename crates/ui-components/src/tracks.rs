//! 曲目表套件：列定义、定宽单元格、列头与曲目行；文件夹 / 浏览 / 歌单页共用。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use nsh_core::prelude::*;

use super::artists::artist_links;
use super::cover::{cover_initial, initial_letter};
use zeo_icons::{Icon, icon_dyn};
use zeo_types::config::TrackEntry;
use zeo_ui_store::theme::*;
use zeo_ui_store::{
    cover_for, covers_on, format_time, is_liked, shown_playing, store, toggle_like,
};

/// 曲目列表显示哪些列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    /// 序号列（`#`）显示标签曲目号：只有专辑详情开；`index` 开时
    /// 显示列表位置（1 起）。
    pub number: bool,
    /// 序号列（`#`）显示列表位置（1 起）；`number` 显示标签曲目号，
    /// 只有专辑详情用。
    pub index: bool,
    pub cover: bool,
    pub album: bool,
    pub size: bool,
    /// 音频格式（MP3 / FLAC / WAV / AIFF / M4A / OGG 等）。
    pub format: bool,
}

impl Columns {
    pub const ALL: Columns = Columns {
        number: false,
        index: false,
        cover: true,
        album: true,
        size: false,
        format: false,
    };
    /// 音乐库：列表位置的序号列 + 大小列。
    pub const LIBRARY: Columns = Columns {
        number: false,
        index: true,
        cover: true,
        album: true,
        size: true,
        format: false,
    };
}

/// 列宽（列头、曲目行、文件夹行共用，保证对齐）。
pub const W_NUMBER: f32 = 32.0;
pub const W_COVER: f32 = 40.0;
pub const W_ALBUM: f32 = 220.0;
pub const W_SIZE: f32 = 72.0;
pub const W_FORMAT: f32 = 48.0;
pub const W_TIME: f32 = 56.0;
pub const W_LIKE: f32 = 32.0;

/// 内容区的宽度断点取值：实时 `layout_width` 减去固定侧栏。
pub fn content_break() -> f32 {
    layout_width() - SIDEBAR
}

/// 窄窗口里依次收起：专辑列（< 760 pt）、大小列（< 600 pt）、序号列
/// （< 420 pt；序号列只有专辑详情与音乐库有）。
pub fn fits_album() -> bool {
    content_break() >= 760.0
}
pub fn fits_size() -> bool {
    content_break() >= 600.0
}
pub fn fits_number() -> bool {
    content_break() >= 420.0
}

/// 文件的音频格式（扩展名大写）：MP3、FLAC、WAV、AIFF、M4A、OGG 等。
pub fn audio_format(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_uppercase())
        .unwrap_or_default()
}

/// 格式小标签（定宽列内左对齐）；值为空时不画框。
pub fn format_cell(value: impl Fn() -> String + Copy + 'static) -> View {
    view().w(px(W_FORMAT)).items_start().child(when(
        move || !value().is_empty(),
        move || {
            view()
                .h(px(18.0))
                .px(px(5.0))
                .justify_center()
                .rounded(px(4.0))
                .border_1()
                .border_color(border())
                .child(
                    text(value)
                        .text_size(px(10.0))
                        .line_height(px(10.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .letter_spacing(px(0.4))
                        .text_color(muted_fg()),
                )
        },
    ))
}

/// 音质小徽章：歌手名前的彩色描边小标（HQ 绿 / SQ 蓝 / Hi-Res 黄，
/// 见 [`quality_color`]），9pt 粗体、无底色；放不下时不收缩。
pub fn quality_badge(tag: &'static str) -> View {
    let color = quality_color(tag);
    view()
        .h(px(14.0))
        .px(px(4.0))
        .flex_shrink_0()
        .justify_center()
        .rounded(px(3.0))
        .border_1()
        .border_color(color)
        .child(
            text(tag)
                .text_size(px(9.0))
                .line_height(px(9.0))
                .font_weight(FontWeight::SEMIBOLD)
                .letter_spacing(px(0.3))
                .text_color(color),
        )
}

/// 字节数 → 1.2 GB / 34.5 MB / 820 KB。
pub fn format_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.2} GB", b / 1024.0 / 1024.0 / 1024.0)
    } else if b >= 1024.0 * 1024.0 {
        format!("{:.1} MB", b / 1024.0 / 1024.0)
    } else {
        format!("{} KB", (b / 1024.0).round() as u64)
    }
}

/// 右对齐的数值列。
pub fn value_cell(width: f32, value: impl Fn() -> String + 'static) -> View {
    view().w(px(width)).items_end().child(
        text(value)
            .number_of_lines(1)
            .text_size(px(13.0))
            .line_height(px(13.0))
            .tabular_nums()
            .text_color(muted_fg()),
    )
}

/// 曲目列表的列头。
pub fn track_columns(cols: Columns) -> View {
    let label = |s: &'static str| {
        text(s)
            .text_size(px(11.0))
            .line_height(px(11.0))
            .font_weight(FontWeight::SEMIBOLD)
            .letter_spacing(px(0.6))
            .text_color(faint())
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .h(px(32.0))
        .px(px(12.0))
        .mx(px(20.0))
        .border_b_1()
        .border_b_color(border())
        .child((cols.number || cols.index).then(|| {
            when(fits_number, move || {
                view().w(px(W_NUMBER)).items_center().child(label("#"))
            })
        }))
        .child(
            cols.cover
                .then(|| when(covers_on, move || view().w(px(W_COVER)))),
        )
        .child(view().flex_1().min_w(px(60.0)).child(label("标题")))
        .child(cols.album.then(|| {
            when(fits_album, move || {
                view().w(px(W_ALBUM)).child(label("专辑"))
            })
        }))
        .child(cols.format.then(|| {
            when(fits_size, move || {
                view().w(px(W_FORMAT)).child(label("格式"))
            })
        }))
        .child(cols.size.then(|| {
            when(fits_size, move || {
                view().w(px(W_SIZE)).items_end().child(label("大小"))
            })
        }))
        .child(view().w(px(W_TIME)).items_end().child(label("时长")))
        // 喜欢列（曲目行的爱心按钮）。
        .child(view().w(px(W_LIKE)))
}

/// 曲目行高。
pub const ROW: f32 = 56.0;

/// 双击窗口（行内按压只选中曲目所在的行，双击才播放）。
const DOUBLE_CLICK: Duration = Duration::from_millis(450);

/// 一行曲目：序号（`cols.number` 显示标签曲目号、`cols.index` 显示列表
/// 位置；当前曲目为音波，悬停变播放 / 暂停钮）、封面、曲名 + 歌手、
/// 专辑（可点进专辑页）、时长、喜欢按钮。双击播放（`on_play`）；
/// `track` 随单元格复用而变化。
pub fn track_row(
    track: Memo<Option<TrackEntry>>,
    number: Memo<String>,
    cols: Columns,
    on_play: impl Fn() + 'static,
) -> View {
    track_row_selectable_inner(track, number, cols, None, |_| {}, on_play)
}

/// 支持外部控制选中态与点击事件的曲目行变体。
pub fn track_row_selectable(
    track: Memo<Option<TrackEntry>>,
    number: Memo<String>,
    cols: Columns,
    selected: impl Fn() -> bool + Clone + 'static,
    on_press: impl Fn(&PressEvent) + 'static,
    on_play: impl Fn() + 'static,
) -> View {
    track_row_selectable_inner(
        track,
        number,
        cols,
        Some(Rc::new(selected)),
        on_press,
        on_play,
    )
}

fn track_row_selectable_inner(
    track: Memo<Option<TrackEntry>>,
    number: Memo<String>,
    cols: Columns,
    selected: Option<Rc<dyn Fn() -> bool>>,
    on_press: impl Fn(&PressEvent) + 'static,
    on_play: impl Fn() + 'static,
) -> View {
    let s = store();
    let current = computed(move || {
        track.with(|t| {
            let Some(t) = t else { return false };
            s.track
                .with(|now| now.path.as_deref() == Some(t.path.as_str()))
        })
    });
    let selected: Rc<dyn Fn() -> bool> = selected.unwrap_or_else(|| Rc::new(move || current.get()));
    let playing = move || current.get() && shown_playing();
    let field = move |f: fn(&TrackEntry) -> String| {
        move || track.with(|t| t.as_ref().map(f).unwrap_or_default())
    };
    let title = field(|t| t.display_title().to_owned());
    let path = move || track.with_untracked(|t| t.as_ref().map(|t| t.path.clone()));
    let liked_memo =
        computed(move || track.with(|t| t.as_ref().is_some_and(|t| is_liked(&t.path))));
    let liked = move || liked_memo.get();
    let hovering = signal(false);
    // 单击不播放：记下曲目与时间，同一首在 `DOUBLE_CLICK` 内再按一次才播。
    let on_play = Rc::new(on_play);
    let on_play_cell = on_play.clone();
    let last = Rc::new(RefCell::new(None::<(String, Instant)>));
    let sel_bg = {
        let s = selected.clone();
        move || s()
    };
    let sel_hover = {
        let s = selected.clone();
        move || s()
    };
    let sel_state = move || selected();
    view()
        .h(px(ROW))
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .px(px(12.0))
        .rounded(px(10.0))
        .role(AccessibilityRole::Button)
        .label(move || SharedString::from(title()))
        .accessibility_state(move || AccessibilityState {
            selected: sel_state(),
            ..AccessibilityState::default()
        })
        .bg(move || {
            if sel_bg() {
                primary().opacity(0.12)
            } else if current.get() {
                hover()
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .hover(move |st| {
            st.bg(move || {
                if sel_hover() {
                    primary().opacity(0.18)
                } else {
                    hover()
                }
            })
        })
        .on_mouse_enter(move |_: &MouseEvent| hovering.set(true))
        .on_mouse_leave(move |_: &MouseEvent| hovering.set(false))
        .on_press(move |e: &PressEvent| {
            let now = Instant::now();
            let is_plain =
                !e.modifiers.cmd && !e.modifiers.ctrl && !e.modifiers.shift && !e.modifiers.alt;
            let hit = is_plain
                && last.borrow().as_ref().is_some_and(|(p, t)| {
                    path().as_deref() == Some(p.as_str()) && now.duration_since(*t) <= DOUBLE_CLICK
                });
            on_press(e);
            if hit {
                *last.borrow_mut() = None;
                on_play();
            } else if is_plain {
                *last.borrow_mut() = path().map(|p| (p, now));
            } else {
                *last.borrow_mut() = None;
            }
        })
        .child((cols.number || cols.index).then(|| {
            // 序号格常是可按的播放钮：内层 press 不到行，单击只播一次。
            when(fits_number, move || {
                let on_play_cell = on_play_cell.clone();
                view()
                    .w(px(W_NUMBER))
                    .h(px(28.0))
                    .items_center()
                    .justify_center()
                    .rounded(px(6.0))
                    .role(AccessibilityRole::Button)
                    .label(move || {
                        SharedString::from(if playing() {
                            format!("暂停「{}」", title())
                        } else {
                            format!("播放「{}」", title())
                        })
                    })
                    .hover(|st| st.bg(hover()))
                    .active(|st| st.opacity(0.6))
                    .on_press(move |_: &()| {
                        if current.get() {
                            zeo_ui_store::toggle();
                        } else {
                            on_play_cell();
                        }
                    })
                    .child(
                        when(
                            move || hovering.get(),
                            move || {
                                icon_dyn(
                                    move || if playing() { Icon::Pause } else { Icon::Play },
                                    14.0,
                                    fg,
                                )
                            },
                        )
                        .otherwise(move || {
                            when(
                                move || current.get(),
                                move || {
                                    icon_dyn(
                                        move || {
                                            if playing() { Icon::Wave } else { Icon::Play }
                                        },
                                        14.0,
                                        primary,
                                    )
                                },
                            )
                            .otherwise(move || {
                                text(move || number.get())
                                    .text_size(px(13.0))
                                    .line_height(px(13.0))
                                    .tabular_nums()
                                    .text_color(faint())
                            })
                        }),
                    )
            })
        }))
        .child(cols.cover.then(|| {
            let src = move || track.with(|t| t.as_ref().and_then(|t| cover_for(&t.path, 40.0)));
            let letter = move || {
                track.with(|t| {
                    t.as_ref()
                        .map(|t| initial_letter(t.display_title(), &t.path))
                        .unwrap_or_default()
                })
            };
            when(covers_on, move || {
                cover_initial(src, letter, Some(40.0), 6.0)
            })
        }))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(STACK_GAP))
                .child(
                    text(field(|t| t.display_title().to_owned()))
                        .number_of_lines(1)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || if current.get() { primary() } else { fg() })
                        .transition_colors()
                        .duration(300.0),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .min_w(px(0.0))
                        .child(when(
                            move || {
                                track
                                    .with(|t| t.as_ref().and_then(|t| t.meta.quality_tag()))
                                    .is_some()
                            },
                            move || {
                                dynamic(move || {
                                    track
                                        .with(|t| t.as_ref().and_then(|t| t.meta.quality_tag()))
                                        .map(quality_badge)
                                })
                            },
                        ))
                        .child(artist_links(
                            move || {
                                track.with(|t| {
                                    t.as_ref()
                                        .map(|t| {
                                            if t.artists.is_empty() {
                                                vec![t.display_artist().to_owned()]
                                            } else {
                                                t.artists.clone()
                                            }
                                        })
                                        .unwrap_or_default()
                                })
                            },
                            12.0,
                        )),
                ),
        )
        .child(cols.album.then(|| {
            // 专辑名是进专辑详情的链接，只包住文字本身（同 `artist_links`）。
            when(fits_album, move || {
                let album_name = field(|t| t.display_album().to_owned());
                let hov = signal(false);
                view().w(px(W_ALBUM)).items_start().child(
                    view()
                        .max_w(percentage(1.0))
                        .cursor(Cursor::Pointer)
                        .role(AccessibilityRole::Button)
                        .label(move || SharedString::from(album_name()))
                        .on_mouse_enter(move |_: &MouseEvent| hov.set(true))
                        .on_mouse_leave(move |_: &MouseEvent| hov.set(false))
                        .on_press(move |_: &()| {
                            track.with_untracked(|t| {
                                if let Some(t) = t.as_ref() {
                                    zeo_ui_store::open_album(t);
                                }
                            });
                        })
                        .child(
                            text(field(|t| t.display_album().to_owned()))
                                .number_of_lines(1)
                                .text_size(px(13.0))
                                .line_height(px(13.0))
                                .text_color(move || if hov.get() { fg() } else { muted_fg() })
                                .transition_colors(),
                        ),
                )
            })
        }))
        .child(cols.format.then(|| {
            when(fits_size, move || {
                format_cell(field(|t| audio_format(&t.path)))
            })
        }))
        .child(cols.size.then(|| {
            when(fits_size, move || {
                value_cell(W_SIZE, field(|t| format_bytes(t.size_bytes)))
            })
        }))
        .child(value_cell(W_TIME, field(|t| format_time(t.duration))))
        .child(
            view().w(px(W_LIKE)).items_center().justify_center().child(
                view()
                    .size(px(26.0))
                    .items_center()
                    .justify_center()
                    .rounded(px(6.0))
                    .role(AccessibilityRole::Button)
                    .label(move || if liked() { "取消喜欢" } else { "喜欢" })
                    .hover(|st| st.bg(hover()))
                    .active(|st| st.opacity(0.6))
                    .on_press(move |_: &()| {
                        if let Some(p) = path() {
                            toggle_like(&p);
                        }
                    })
                    .child(when(
                        move || liked() || hovering.get(),
                        move || {
                            icon_dyn(
                                move || {
                                    if liked() {
                                        Icon::HeartFilled
                                    } else {
                                        Icon::Heart
                                    }
                                },
                                15.0,
                                move || if liked() { primary() } else { faint() },
                            )
                        },
                    )),
            ),
        )
}
