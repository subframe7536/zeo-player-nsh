use super::*;

/// 右半区：有歌词时滚动列表，载入中一行提示；有时间轴的歌词在右边缘
/// 放偏移栏（占位、常显，不被歌词盖住）。
pub(crate) fn lyrics_column(swap: &Swap) -> View {
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
pub(crate) fn lyrics_scroller() -> View {
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
