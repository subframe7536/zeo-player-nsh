//! 全屏播放的歌词列：跟随时间滚动（当前行停在 [`ANCHOR`] 处），用户滚动
//! 后 3 秒恢复跟随；点击行 seek；距离当前行越远越暗越模糊；边缘淡出按行算。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use nsh_core::prelude::*;

use nsh_base::popover;

use super::super::components::button::{RADIUS_MD, on_dark};
use super::super::components::icons::{Icon, icon};
use super::super::components::slider::{SliderStyle, slider};
use super::super::store::{
    format_time, menu_control, nudge_lyrics_offset, seek_to, set_lyrics_offset, store,
};
use super::super::theme::*;
use super::{Reveal, Swap};
use crate::config::LyricsAlign;

/// 歌词列最大宽度。
const LYRICS: f32 = 520.0;
/// 当前歌词行停在视口从上往下的这个比例处（偏上，下方留出更多后续歌词）。
const ANCHOR: f32 = 0.35;

// --- 歌词 ---------------------------------------------------------------------

/// 是否显示歌词区：没在播放或这首没有歌词时不显示。换歌后歌词载入前
/// （`lyrics` 为 `None`）保持上一首的结果，不在两种布局之间来回跳。
pub(super) fn lyrics_shown() -> Signal<bool> {
    let s = store();
    let decide = move || -> Option<bool> {
        if s.track.with(|t| t.path.is_none()) {
            return Some(false);
        }
        s.lyrics
            .with(|l| l.as_ref().map(|l| !l.output.lines.is_empty()))
    };
    let shown = signal(untracked(decide).unwrap_or(true));
    effect(move || {
        if let Some(next) = decide()
            && next != shown.get_untracked()
        {
            shown.set(next);
        }
    });
    shown
}

/// `+0.5s` / `−0.5s`（一位小数、带符号；负号用 `−` U+2212）。
fn fmt_offset(ms: i64) -> String {
    let sign = if ms < 0 { "−" } else { "+" };
    format!("{sign}{:.1}s", ms.unsigned_abs() as f64 / 1000.0)
}

/// 右边缘的偏移栏（占位、常显）：竖排 [+] 数值 [−]，数值点开横向滑条
/// 弹层（同音量弹层的路子）；± 按 0.5 秒步进，全局偏移与「重置」在弹层里。
fn offset_rail() -> AnyElement {
    let s = store();
    let open = signal(false);
    let gate = menu_control();
    effect(gate.evicted(open));
    let value = move || {
        let v = s.lyrics_offset.get();
        if v == 0 {
            "0s".to_owned()
        } else {
            fmt_offset(v)
        }
    };
    // ±30 秒折成 0..1，吸附到 0.1 秒。
    let fraction = move || (s.lyrics_offset.get() as f64 + 30000.0) / 60000.0;
    let apply =
        move |f: f64| set_lyrics_offset(((f * 60000.0 - 30000.0) / 100.0).round() as i64 * 100);
    // 触发器用普通 view（nsh-base 的 button 会吃掉按压，popover 收不到，
    // 同队列 / 音量按钮）。
    let trigger = view()
        .h(px(28.0))
        .px(px(4.0))
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_MD))
        .role(AccessibilityRole::Button)
        .label("调整歌词时间偏移")
        .style_when(move || open.get(), |st| st.bg(white_a(0.10)))
        .hover(|st| st.bg(white_a(0.08)))
        .active(|st| st.opacity(0.8))
        .transition_colors()
        .duration(150.0)
        .child(
            text(move || value())
                .text_size(px(11.0))
                .line_height(px(11.0))
                .tabular_nums()
                .text_color(white_a(0.8)),
        );
    let panel = view()
        .items_center()
        .gap(px(2.0))
        .py(px(4.0))
        .px(px(4.0))
        .rounded(px(10.0))
        .bg(white_a(0.06))
        .child(
            on_dark(28.0)
                .accessibility_label("歌词提前")
                .on_press(|_: &()| nudge_lyrics_offset(Some(500)))
                .child(icon(Icon::Plus, 14.0, || white_a(0.85))),
        )
        .child(
            popover()
                .open(gate.open_with(open))
                .on_open_change(gate.popover_change(open))
                .placement(Placement::Left)
                .align(Align::Center)
                .offset(px(12.0))
                .margin(px(8.0))
                .popup_style(|st| {
                    st.rounded(px(RADIUS_MD))
                        .border_1()
                        .border_color(border())
                        .bg(nsh_base::theme().popover)
                        .shadow_md()
                        .overflow_hidden()
                })
                .trigger(trigger)
                .child(
                    view()
                        .w(px(232.0))
                        .px(px(14.0))
                        .py(px(12.0))
                        .gap(px(8.0))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    text(move || value())
                                        .text_size(px(13.0))
                                        .line_height(px(13.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .tabular_nums()
                                        .text_color(fg()),
                                )
                                .child(dynamic(move || {
                                    let g = s.library.with(|l| l.config.global_lyrics_offset_ms);
                                    (g != 0).then(|| {
                                        text(format!("全局 {}", fmt_offset(g)))
                                            .text_size(px(11.0))
                                            .line_height(px(11.0))
                                            .text_color(muted_fg())
                                            .into_element()
                                    })
                                }))
                                .child(view().flex_1())
                                .child(dynamic(move || {
                                    (s.lyrics_offset.get() != 0).then(|| {
                                        nsh_base::button()
                                            .h(px(24.0))
                                            .px(px(8.0))
                                            .flex_shrink_0()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px(6.0))
                                            .transition_colors()
                                            .duration(150.0)
                                            .hover(|st| st.bg(hover()))
                                            .active(|st| st.opacity(0.7))
                                            .accessibility_label("重置歌词偏移")
                                            .on_press(|_: &()| nudge_lyrics_offset(None))
                                            .child(
                                                text("重置")
                                                    .text_size(px(11.0))
                                                    .line_height(px(11.0))
                                                    .text_color(muted_fg()),
                                            )
                                            .into_element()
                                    })
                                })),
                        )
                        .child(
                            slider(
                                SliderStyle::HORIZONTAL,
                                "歌词偏移",
                                fraction,
                                input(),
                                fg(),
                                fg(),
                                apply,
                                apply,
                            )
                            .w_full(),
                        )
                        .child(
                            view()
                                .flex_row()
                                .child(
                                    text("延后 30s")
                                        .text_size(px(10.0))
                                        .line_height(px(10.0))
                                        .text_color(muted_fg()),
                                )
                                .child(view().flex_1())
                                .child(
                                    text("提前 30s")
                                        .text_size(px(10.0))
                                        .line_height(px(10.0))
                                        .text_color(muted_fg()),
                                ),
                        ),
                ),
        )
        .child(
            on_dark(28.0)
                .accessibility_label("歌词延后")
                .on_press(|_: &()| nudge_lyrics_offset(Some(-500)))
                .child(icon(Icon::Minus, 14.0, || white_a(0.85))),
        );
    view()
        .h_full()
        .w(px(48.0))
        .flex_shrink_0()
        .justify_center()
        .items_center()
        .child(panel)
        .into_element()
}

/// 右半区：有歌词时滚动列表，载入中一行提示；有时间轴的歌词在右边缘
/// 放偏移栏（占位、常显，不被歌词盖住）。
pub(super) fn lyrics_column(swap: &Swap) -> View {
    let s = store();
    let has_lines = move || {
        s.lyrics
            .with(|l| l.as_ref().is_some_and(|l| !l.output.lines.is_empty()))
    };
    let timed = move || {
        s.lyrics
            .with(|l| l.as_ref().is_some_and(|l| l.output.timed))
    };
    view()
        .w_full()
        .h_full()
        .flex_row()
        .bind_opacity(&swap.content)
        .bind_translate_x(&swap.lyrics_shift)
        .child(view().flex_1().h_full().min_w(px(0.0)).child(
            when(has_lines, lyrics_scroller).otherwise(move || {
                view().flex_1().items_center().justify_center().child(
                    text(move || {
                        if s.lyrics.with(Option::is_none) && s.track.with(|t| t.path.is_some()) {
                            "正在载入歌词…"
                        } else {
                            "暂无歌词"
                        }
                    })
                    .text_size(px(15.0))
                    .line_height(px(15.0))
                    .text_color(white_a(0.4)),
                )
            }),
        ))
        .child(when(timed, offset_rail))
}

/// 行级歌词：跟随时间滚动（当前行停在 [`ANCHOR`] 处），用户滚动后 3 s
/// 恢复跟随；点击行 seek；距离当前行越远越暗越模糊。
fn lyrics_scroller() -> View {
    let s = store();
    let scroller = element_ref();
    let viewport = signal(0.0_f32);
    let laid = signal(0_usize);
    let follow = signal(true);
    // 指针在歌词区域内：所有行去掉模糊（看清再点）。`clear` 是去模糊的
    // 程度（0 按距离模糊，1 全部清晰），移入移出时逐帧过渡：NSH 的样式
    // 过渡不包括滤镜，只能自己推进。
    let clear = signal(0.0_f32);
    let clear_task: Rc<RefCell<Option<Spawned<()>>>> = Rc::default();
    let fade_blur = Rc::new(move |to: f32| drive(clear, &clear_task, to, BLUR_FADE_MS));
    let line_ys: Rc<RefCell<Vec<f32>>> = Rc::default();
    // 滚动位置与各行的位置、高度，用来让靠近上下边缘的行淡出（见 `edge_fade`）。
    let scroll_y = signal(0.0_f32);
    let rows_box = signal(Vec::<(f32, f32)>::new());
    let target: Rc<RefCell<Option<(f32, std::time::Instant)>>> = Rc::default();
    let resume: Rc<RefCell<Option<Spawned<()>>>> = Rc::default();
    let last_followed = Rc::new(RefCell::new(-1_i64));
    // 点击某行后等高亮换到这一行再跟随：seek 生效前 `active_line` 还是
    // 旧行，跟随它会先滚回旧行再滚到目标行。
    let pending: Rc<RefCell<Option<(i64, std::time::Instant)>>> = Rc::default();
    let prev_time = Rc::new(RefCell::new(-1.0_f64));
    let lines = untracked(move || s.lyrics.with(|l| l.as_ref().map(|l| l.output.clone())))
        .expect("lyrics_scroller is shown with lyrics");
    let timed = lines.timed;
    let count = lines.lines.len();
    let align = move || s.library.with(|l| l.config.lyrics_align);

    // 打开全屏（或歌词刚载入）时直接定位到高亮行，不带动画；定位好之前
    // 歌词隐藏，不会先显示开头再滚过去。
    //
    // 行的位置量的是它在歌词列（上方留白之下的那一层）里的 y：上方留白
    // 正好是 `viewport * ANCHOR`，所以把第 i 行放到锚点的滚动偏移就是它的
    // y，与视口高度无关。留白随视口高度变化时不会用到过期的位置。
    let placed = signal(false);
    let offset_of = {
        let line_ys = line_ys.clone();
        move |index: i64| {
            usize::try_from(index)
                .ok()
                .and_then(|i| line_ys.borrow().get(i).copied())
        }
    };
    let scroll_to_line = {
        let target = target.clone();
        let offset_of = offset_of.clone();
        Rc::new(move |index: i64| {
            let Some(y) = offset_of(index) else { return };
            *target.borrow_mut() = Some((y, std::time::Instant::now()));
            scroller.scroll_to(point(px(0.0), px(y)), true);
        })
    };

    // 首次定位：组件自己的任务（不放在跟随的 effect 里：那个 effect 随
    // 播放时间重跑，里面起的任务会跟着结束，歌词就一直隐藏）。等布局就绪
    // 后逐帧不带动画地定位，连续三帧（每次重新读行的位置，内容尺寸可能
    // 晚几帧提交）后显示；最多等 600 ms，到时无论如何都显示。
    {
        let target = target.clone();
        let offset_of = offset_of.clone();
        let last_followed = last_followed.clone();
        spawn(async move {
            let start = std::time::Instant::now();
            let mut settled = 0;
            loop {
                next_frame().await;
                let ready = viewport.get_untracked() > 0.0 && laid.get_untracked() >= count;
                if ready {
                    let index = s.active_line.get_untracked();
                    if index < 0 {
                        break;
                    }
                    if let Some(y) = offset_of(index) {
                        *last_followed.borrow_mut() = index;
                        *target.borrow_mut() = Some((y, std::time::Instant::now()));
                        scroller.scroll_to(point(px(0.0), px(y)), false);
                        scroll_y.set(y);
                        settled += 1;
                        if settled >= 3 {
                            break;
                        }
                    }
                }
                if start.elapsed() >= Duration::from_millis(600) {
                    break;
                }
            }
            placed.set(true);
        });
    }

    // 时间推进：行变化或 seek 跳变时滚动跟随。
    {
        let last_followed = last_followed.clone();
        let prev_time = prev_time.clone();
        let pending = pending.clone();
        let scroll_to_line = scroll_to_line.clone();
        effect(move || {
            let ready = viewport.get() > 0.0 && laid.get() >= count;
            let time = s.playback.with(|p| p.time);
            let index = s.active_line.get();
            // 首次定位由上面的任务做；定位好之前不跟随。
            if !ready || !placed.get() {
                return;
            }
            let jumped = {
                let mut prev = prev_time.borrow_mut();
                let jumped = *prev >= 0.0 && (time - *prev).abs() > 1.5;
                *prev = time;
                jumped
            };
            {
                let mut wait = pending.borrow_mut();
                if let Some((line, at)) = *wait {
                    if index != line && at.elapsed() < PENDING_WAIT {
                        return;
                    }
                    *wait = None;
                }
            }
            if !follow.get() || index < 0 || (index == *last_followed.borrow() && !jumped) {
                return;
            }
            *last_followed.borrow_mut() = index;
            scroll_to_line(index);
        });
    }

    // 收起途中又打开时歌词还在（没有重建）：同样直接回到高亮行，恢复跟随。
    {
        let last_followed = last_followed.clone();
        let resume = resume.clone();
        let pending = pending.clone();
        let target = target.clone();
        let offset_of = offset_of.clone();
        let reveal = global::<Reveal>();
        let first = std::cell::Cell::new(true);
        effect(move || {
            reveal.get();
            if first.replace(false) {
                return;
            }
            untracked(|| {
                if let Some(task) = resume.borrow_mut().take() {
                    task.cancel();
                }
                *pending.borrow_mut() = None;
                follow.set(true);
                let index = s.active_line.get();
                *last_followed.borrow_mut() = index;
                // 歌词一直在显示，不隐藏：直接跳过去（不带动画）。
                if let Some(y) = offset_of(index) {
                    *target.borrow_mut() = Some((y, std::time::Instant::now()));
                    scroller.scroll_to(point(px(0.0), px(y)), false);
                    scroll_y.set(y);
                }
            });
        });
    }

    let interrupt = {
        let target = target.clone();
        let resume = resume.clone();
        let last_followed = last_followed.clone();
        Rc::new(move || {
            *target.borrow_mut() = None;
            follow.set(false);
            if let Some(task) = resume.borrow_mut().take() {
                task.cancel();
            }
            let last_followed = last_followed.clone();
            *resume.borrow_mut() = Some(set_timeout(Duration::from_secs(3), move || {
                *last_followed.borrow_mut() = s.active_line.get_untracked();
                follow.set(true);
            }));
        })
    };

    let rows = lines.lines.iter().enumerate().map(|(index, line)| {
        let i = index as i64;
        let seekable = line.time >= 0.0;
        let time = line.time;
        let content = line.raw_content.trim().to_owned();
        let content = if content.is_empty() {
            "· · ·".to_owned()
        } else {
            content
        };
        let metadata = line.metadata;
        let distance = move || {
            if timed {
                (i - s.active_line.get()).unsigned_abs() as f32
            } else {
                0.0
            }
        };
        let line_ys = line_ys.clone();
        let resume = resume.clone();
        let last_followed = last_followed.clone();
        let pending = pending.clone();
        let scroll_to_line = scroll_to_line.clone();
        let content_view = view()
            .max_w(px(LYRICS))
            .style_with(move |st| match align() {
                LyricsAlign::Left => st.self_start().items_start(),
                LyricsAlign::Center => st.self_center().items_center(),
                LyricsAlign::Right => st.self_end().items_end(),
            })
            .py(px(14.0))
            .px(px(20.0))
            .rounded(px(12.0))
            .gap(px(4.0))
            // 高亮只包住文字本身，触发区是整行（外层是 group）。
            .group_hover(|st| st.bg(white_a(0.07)))
            .transition_colors()
            .duration(150.0)
            .child(
                text(content)
                    .style_with(move |st| text_align(st, align()))
                    .text_size(px(if metadata { 16.0 } else { LYRIC_SIZE }))
                    .line_height(px(if metadata { 24.0 } else { LYRIC_SIZE * 1.3 }))
                    .font_weight(if metadata {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::BOLD
                    })
                    .text_color(white_a(1.0))
                    // 高亮换行的明暗渐变：文字颜色不在样式过渡里，画成纯白、
                    // 用不透明度走原生过渡（未高亮 IDLE_ALPHA、高亮
                    // ACTIVE_ALPHA）；时长与缓动同程序滚动。
                    .opacity(move || original_opacity(timed, distance() == 0.0))
                    .transition_opacity()
                    .ease_out()
                    .duration(SCROLL_MS),
            )
            .child(line.trans_content.clone().map(|trans| {
                text(trans)
                    .style_with(move |st| text_align(st, align()))
                    .text_size(px(TRANSLATION_SIZE))
                    .line_height(px(TRANSLATION_SIZE * 1.4))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(white_a(TRANSLATION_ALPHA))
            }));
        // 时间戳：占位在行尾（右对齐时在行首）、垂直居中，悬停浮现。槽位
        // 在布局里占位置，长歌词盖不到它；开关关掉时槽都不留，也不挡点击。
        let stamp = move |on_left: bool| {
            dynamic(move || {
                (seekable
                    && s.library.with(|l| {
                        l.config.lyrics_timestamps
                            && (l.config.lyrics_align == LyricsAlign::Right) == on_left
                    }))
                .then(|| {
                    view()
                        .flex_shrink_0()
                        .self_center()
                        .style_with(move |st| {
                            if on_left {
                                st.pr(px(6.0))
                            } else {
                                st.pl(px(6.0))
                            }
                        })
                        .pointer_events(PointerEvents::None)
                        .opacity(0.0)
                        .group_hover(|st| st.opacity(1.0))
                        .transition_opacity()
                        .duration(150.0)
                        .child(
                            view()
                                .h(px(22.0))
                                .px(px(8.0))
                                .justify_center()
                                .rounded(px(6.0))
                                .bg(AdaptiveColor::rgb(0x1c2230, 0x1c2230).opacity(0.75))
                                .child(
                                    text(format_time(time))
                                        .text_size(px(12.0))
                                        .line_height(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .tabular_nums()
                                        .text_color(white_a(0.8)),
                                ),
                        )
                        .into_element()
                })
            })
        };
        let fade = move || {
            let viewport = viewport.get();
            let y = scroll_y.get();
            rows_box.with(|b| {
                b.get(index).map_or(1.0, |&(top, h)| {
                    edge_fade(viewport * ANCHOR + top + h / 2.0 - y, viewport)
                })
            })
        };
        // `on_layout` 只能挂在裸 view 上（`Interactive` 不在组件元素上）：
        // 外层量位置、包按钮。按钮管按压与无障碍，时间戳槽在按钮内容行里。
        view()
            .w_full()
            .group()
            .opacity(fade)
            .child(
                nsh_base::button()
                    .w_full()
                    .accessibility_label(if seekable {
                        format!("跳到 {}", format_time(time))
                    } else {
                        String::new()
                    })
                    // 当前行与前后一句清晰，再远越模糊（见 `lyric_blur`）。颜色
                    // 在文字上：原文随高亮变化，译文始终不变。时间戳槽在这层之外，
                    // 不跟着模糊。
                    .child(
                        view()
                            .w_full()
                            .flex_row()
                            .items_center()
                            .child(stamp(true))
                            .child(
                                view()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .style_with(move |st| {
                                        // 鼠标在歌词区时不模糊（移入移出有过渡）；否则按与
                                        // 高亮行的距离模糊。
                                        let blur =
                                            lyric_blur(timed, distance()) * (1.0 - clear.get());
                                        st.blur(px(blur))
                                    })
                                    .transition_all()
                                    .duration(300.0)
                                    .child(content_view),
                            )
                            .child(stamp(false)),
                    )
                    .on_press(move |_: &()| {
                        if seekable {
                            if let Some(task) = resume.borrow_mut().take() {
                                task.cancel();
                            }
                            // 直接滚到点的这一行；跟随在高亮换过来之前不动。
                            *pending.borrow_mut() = Some((i, std::time::Instant::now()));
                            *last_followed.borrow_mut() = i;
                            // 按生效偏移（每曲 + 全局）跳：正值提前，行时间减去它。
                            let offset = (s.lyrics_offset.get_untracked()
                                + s.library.with(|l| l.config.global_lyrics_offset_ms))
                                as f64
                                / 1000.0;
                            seek_to((time - offset).max(0.0));
                            scroll_to_line(i);
                            follow.set(true);
                        }
                    }),
            )
            .on_layout(move |frame: &Bounds<Points>| {
                let mut ys = line_ys.borrow_mut();
                if ys.len() <= index {
                    ys.resize(index + 1, 0.0);
                }
                ys[index] = frame.origin.y.0;
                drop(ys);
                let row = (frame.origin.y.0, frame.size.height.0);
                if rows_box.with_untracked(|b| b.get(index) != Some(&row)) {
                    rows_box.update(|b| {
                        if b.len() <= index {
                            b.resize(index + 1, (0.0, 0.0));
                        }
                        b[index] = row;
                    });
                }
                if index + 1 > laid.get_untracked() {
                    laid.set(index + 1);
                }
            })
    });

    let user_scroll = interrupt.clone();
    let drag = interrupt.clone();
    let (blur_in, blur_out) = (fade_blur.clone(), fade_blur);
    view()
        .flex_1()
        .w_full()
        .on_mouse_enter(move |_: &MouseEvent| blur_in(1.0))
        .on_mouse_leave(move |_: &MouseEvent| blur_out(0.0))
        .child((!timed).then(|| {
            text("歌词无时间信息")
                .text_center()
                .text_size(px(11.0))
                .line_height(px(11.0))
                .letter_spacing(px(0.6))
                .text_color(white_a(0.4))
                .pb(px(8.0))
        }))
        .child(
            scroll()
                .flex_1()
                .opacity(move || if placed.get() { 1.0_f32 } else { 0.0 })
                .scrollbar(false)
                .element_ref(&scroller)
                .on_layout(move |frame: &Bounds<Points>| viewport.set(frame.size.height.0))
                .on_scroll(move |e: &ScrollEvent| {
                    let y = e.offset.y.0;
                    scroll_y.set(y);
                    // 程序滚动：动画进行中（约 0.7 s 内）或已到目标附近。
                    let ours = target.borrow().is_some_and(|(t, at)| {
                        at.elapsed().as_millis() < 700 || (y - t).abs() < 64.0
                    });
                    if !ours && follow.get_untracked() {
                        user_scroll();
                    }
                })
                .on_scroll_drag_start(move |_: &Point<Points>| drag())
                .child(
                    view()
                        .w_full()
                        .child(view().h(move || px(viewport.get() * ANCHOR)))
                        .child(view().w_full().children(rows))
                        .child(view().h(move || px(viewport.get() * (1.0 - ANCHOR)))),
                ),
        )
}

/// 歌词行的模糊半径（点）：高亮行和前后一句清晰，再远每多一行加 1.5，
/// 最多 8（与 ap-spec 的上限相同）。无时间轴的歌词不模糊。
fn lyric_blur(timed: bool, distance: f32) -> f32 {
    if timed {
        ((distance - 1.0) * 1.5).clamp(0.0, 8.0)
    } else {
        0.0
    }
}

/// 歌词行在视口里的不透明度：行中心进入顶部 [`EDGE_TOP`] 或底部
/// [`EDGE_BOTTOM`]（占视口高度的比例）后线性淡出，到边缘为 0。NSH 的
/// macOS / Windows 后端还不画 `maskImage`，只能按行算。
fn edge_fade(center: f32, viewport: f32) -> f32 {
    if viewport <= 0.0 {
        return 1.0;
    }
    let top = center / (viewport * EDGE_TOP);
    let bottom = (viewport - center) / (viewport * EDGE_BOTTOM);
    top.min(bottom).clamp(0.0, 1.0)
}

/// 顶部淡出区（视口高度的比例），小于 [`ANCHOR`]，高亮行不受影响。
const EDGE_TOP: f32 = 0.18;
/// 底部淡出区。
const EDGE_BOTTOM: f32 = 0.28;
/// 点击某行后，最多等这么久让高亮换到这一行（之后照常跟随）。
const PENDING_WAIT: Duration = Duration::from_millis(1500);

fn text_align(
    st: nsh_core::element::StyleRefinement,
    align: LyricsAlign,
) -> nsh_core::element::StyleRefinement {
    match align {
        LyricsAlign::Left => st.text_left(),
        LyricsAlign::Center => st.text_center(),
        LyricsAlign::Right => st.text_right(),
    }
}

/// 歌词原文字号；有译文时译文是它的 2/3。
const LYRIC_SIZE: f32 = 30.0;
/// 译文字号。
const TRANSLATION_SIZE: f32 = LYRIC_SIZE * 2.0 / 3.0;
/// 未高亮原文的颜色（白色透明度）。
const IDLE_ALPHA: f32 = 0.3;
/// 高亮原文的颜色。
const ACTIVE_ALPHA: f32 = 0.92;
/// 译文的颜色，任何时候都不变，位于高亮与未高亮原文之间。
const TRANSLATION_ALPHA: f32 = 0.5;
/// 鼠标移入 / 移出歌词区时，模糊去掉 / 恢复的过渡时长。
const BLUR_FADE_MS: f32 = 300.0;
/// 高亮过渡的时长：与程序滚动一致（NSH 0.3.0 滚动动画 `ANIMATION` 为
/// 300 ms）。
const SCROLL_MS: f32 = 300.0;
/// 歌词区上下的留白：滚动的歌词不进入标题栏（窗口按钮、收起按钮那一行），
/// 也不贴着窗口底边。
pub(super) const LYRICS_BOTTOM: f32 = 32.0;

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// 逐帧把 `value` 推到 `to`（`ms` 毫秒，ease-out）：顶替进行中的上一次，
/// 「减弱动态效果」时直接到位。
fn drive(value: Signal<f32>, task: &Rc<RefCell<Option<Spawned<()>>>>, to: f32, ms: f32) {
    if let Some(t) = task.borrow_mut().take() {
        t.cancel();
    }
    let from = value.get_untracked();
    if untracked(reduce_motion) || (to - from).abs() < 1e-3 {
        value.set(to);
        return;
    }
    let start = std::time::Instant::now();
    *task.borrow_mut() = Some(spawn(async move {
        loop {
            next_frame().await;
            let t = (start.elapsed().as_secs_f32() * 1000.0 / ms).min(1.0);
            value.set(from + (to - from) * ease_out_cubic(t));
            if t >= 1.0 {
                break;
            }
        }
    }));
}

/// 原文的不透明度：画成纯白，高亮行 [`ACTIVE_ALPHA`]、其余
/// [`IDLE_ALPHA`]。无时间轴的歌词没有高亮，按高亮行显示，便于阅读。
fn original_opacity(timed: bool, active: bool) -> f32 {
    if active || !timed {
        ACTIVE_ALPHA
    } else {
        IDLE_ALPHA
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_spares_the_neighbours_and_grows_after() {
        assert_eq!(lyric_blur(true, 0.0), 0.0);
        assert_eq!(lyric_blur(true, 1.0), 0.0, "前后一句不模糊");
        let steps: Vec<f32> = (2..=10).map(|d| lyric_blur(true, d as f32)).collect();
        assert!(steps[0] > 0.0);
        assert!(
            steps.windows(2).all(|w| w[0] < w[1] || w[1] == 8.0),
            "{steps:?}"
        );
        assert_eq!(lyric_blur(true, 20.0), 8.0);
        assert_eq!(lyric_blur(false, 5.0), 0.0);
    }

    #[test]
    fn translation_sits_between_idle_and_active_originals() {
        assert!(
            original_opacity(true, false) < TRANSLATION_ALPHA,
            "未高亮：原文比译文淡"
        );
        assert!(
            original_opacity(true, true) > TRANSLATION_ALPHA,
            "高亮：原文比译文亮"
        );
    }

    #[test]
    fn edges_fade_out() {
        assert_eq!(edge_fade(0.0, 1000.0), 0.0);
        assert_eq!(edge_fade(1000.0, 1000.0), 0.0);
        assert_eq!(edge_fade(ANCHOR * 1000.0, 1000.0), 1.0, "高亮行不淡出");
        assert!((edge_fade(90.0, 1000.0) - 0.5).abs() < 1e-4);
        assert!((edge_fade(860.0, 1000.0) - 0.5).abs() < 1e-4);
    }
}
