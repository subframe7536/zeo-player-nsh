//! 曲目表套件：列定义、定宽单元格、列头与曲目行；文件夹 / 浏览 / 歌单页共用。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use nsh_core::prelude::*;

use super::super::store::{
    cover_for, covers_on, format_time, is_liked, shown_playing, store, toggle_like,
};
use super::super::theme::*;
use super::cover::{cover_initial, initial_letter};
use super::icons::{Icon, icon_dyn};
use crate::config::TrackEntry;

/// 曲目列表显示哪些列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    /// 序号列（`#`）：只有专辑详情显示（曲目号取自标签）；其它列表没有。
    pub number: bool,
    pub cover: bool,
    pub album: bool,
    pub size: bool,
    /// 音频格式（MP3 / FLAC / WAV / AIFF / M4A / OGG 等）。
    pub format: bool,
}

impl Columns {
    pub const ALL: Columns = Columns {
        number: false,
        cover: true,
        album: true,
        size: false,
        format: false,
    };
}

/// 列宽（列头、曲目行、文件夹行共用，保证对齐）。
pub const W_NUMBER: f32 = 28.0;
pub const W_COVER: f32 = 40.0;
pub const W_ALBUM: f32 = 220.0;
pub const W_SIZE: f32 = 72.0;
pub const W_FORMAT: f32 = 48.0;
pub const W_TIME: f32 = 56.0;
pub const W_LIKE: f32 = 32.0;

/// 内容区宽度（窗口减侧栏；窄窗口侧栏收成图标栏）。读取即订阅窗口大小。
pub(crate) fn content_width() -> f32 {
    window().width.0 - sidebar_width()
}

/// 窄窗口里依次收起：专辑列（< 760 pt）、大小列（< 600 pt）、序号列
/// （< 420 pt；序号列只在专辑详情存在）。
pub fn fits_album() -> bool {
    content_width() >= 760.0
}
pub fn fits_size() -> bool {
    content_width() >= 600.0
}
pub fn fits_number() -> bool {
    content_width() >= 420.0
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
        .child(cols.number.then(|| {
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

/// 一行曲目：序号（当前曲目为音波，仅 `cols.number` 的列表有此列）、
/// 封面、曲名 + 歌手、专辑、时长、喜欢按钮。双击播放（`on_play`）；
/// `track` 随单元格复用而变化。
pub fn track_row(
    track: Memo<Option<TrackEntry>>,
    number: Memo<String>,
    cols: Columns,
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
    track_row_selectable(track, number, cols, move || current.get(), |_| {}, on_play)
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
    let s = store();
    let current = computed(move || {
        track.with(|t| {
            let Some(t) = t else { return false };
            s.track
                .with(|now| now.path.as_deref() == Some(t.path.as_str()))
        })
    });
    let playing = move || current.get() && shown_playing();
    let field = move |f: fn(&TrackEntry) -> String| {
        move || track.with(|t| t.as_ref().map(f).unwrap_or_default())
    };
    let title = field(|t| t.title.clone());
    let path = move || track.with_untracked(|t| t.as_ref().map(|t| t.path.clone()));
    let liked = move || track.with(|t| t.as_ref().is_some_and(|t| is_liked(&t.path)));
    let hovering = signal(false);
    // 单击不播放：记下曲目与时间，同一首在 `DOUBLE_CLICK` 内再按一次才播。
    let on_play = Rc::new(on_play);
    let last = Rc::new(RefCell::new(None::<(String, Instant)>));
    let sel_bg = selected.clone();
    let sel_hover = selected.clone();
    let sel_state = selected.clone();
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
        .child(cols.number.then(|| {
            when(fits_number, move || {
                view()
                    .w(px(W_NUMBER))
                    .items_center()
                    .justify_center()
                    .child(
                        when(
                            move || current.get(),
                            move || {
                                icon_dyn(
                                    move || if playing() { Icon::Wave } else { Icon::Play },
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
                        }),
                    )
            })
        }))
        .child(cols.cover.then(|| {
            let src = move || track.with(|t| t.as_ref().and_then(|t| cover_for(&t.path, 40.0)));
            let letter = move || {
                track.with(|t| {
                    t.as_ref()
                        .map(|t| initial_letter(&t.title, &t.path))
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
                .gap(px(2.0))
                .child(
                    text(field(|t| t.title.clone()))
                        .number_of_lines(1)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || if current.get() { primary() } else { fg() })
                        .transition_colors()
                        .duration(300.0),
                )
                .child(
                    text(field(|t| t.artist.clone()))
                        .number_of_lines(1)
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(cols.album.then(|| {
            when(fits_album, move || {
                view().w(px(W_ALBUM)).child(
                    text(field(|t| t.album.clone()))
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(muted_fg()),
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
