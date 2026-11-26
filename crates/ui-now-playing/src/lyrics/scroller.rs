use super::*;

/// 右半区：有歌词时滚动列表，载入中一行提示；有时间轴的歌词在右边缘
/// 放偏移栏（占位，不被歌词盖住；悬停整个右半区才浮现）。
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
    let blur_on = computed(move || s.library.with(|l| l.config.lyrics_blur));
    view()
        .w_full()
        .h_full()
        .flex_row()
        .group_named(LYRICS_GROUP)
        .bind_opacity(&swap.content)
        .bind_translate_x(&swap.lyrics_shift)
        .child(
            view().flex_1().h_full().min_w(px(0.0)).child(
                when(has_lines, move || {
                    dynamic(move || lyrics_scroller(blur_on.get()).into_element())
                })
                .otherwise(move || {
                    view().flex_1().items_center().justify_center().child(
                        text(move || {
                            if s.lyrics.with(Option::is_none) && s.track.with(|t| t.path.is_some())
                            {
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
            ),
        )
        .child(when(timed, offset_rail))
}

/// 行级歌词：跟随时间滚动（当前行停在 [`ANCHOR`] 处），用户滚动后保持
/// 手动位置，鼠标移出歌词区 3 s 后回到高亮行；点击行 seek；未高亮的行
/// 更暗；开启模糊时用 [`blur_effect`]，关闭模糊或无时间轴时彻底绕开
/// `shader_effect`（避免 macOS 每帧 `renderInContext:` 软光栅化），直接
/// 走原生 `scroll` + `mask_linear` 边缘淡出。
pub(crate) fn lyrics_scroller(blur_enabled: bool) -> View {
    let s = store();
    let scroller = element_ref();
    let viewport = signal(0.0_f32);
    let laid_ready = signal(false);
    let follow = signal(true);
    let hovered = signal(false);
    let dragging = Rc::new(std::cell::Cell::new(false));
    let lines = untracked(move || s.lyrics.with(|l| l.as_ref().map(|l| l.output.clone())))
        .expect("lyrics_scroller is shown with lyrics");
    let timed = lines.timed;
    let count = lines.lines.len();
    let use_blur = timed && blur_enabled;
    // macOS 走原生 CoreAnimation `PlainBlur`；Windows 走原生 Direct2D 行级
    // `st.blur` + 边缘 `.opacity`；其余平台（如 Linux）走 `shader_effect`。
    let use_shader = use_blur && cfg!(not(any(target_os = "macos", target_os = "windows")));
    let cull_offscreen = use_shader || cfg!(target_os = "windows");
    // 指针在歌词区域内：去掉模糊（看清再点）。macOS / shader 平台用平台原生
    // AnimatedValue 直接驱动模糊不透明度 / shader uniform（1 模糊，0 全部清晰）。
    #[cfg(not(target_os = "windows"))]
    let blur_amount = animated_value(if use_blur { 1.0 } else { 0.0 });
    #[cfg(not(target_os = "windows"))]
    let fade_blur = Rc::new(move |to: f32| {
        if !use_blur {
            return;
        }
        if untracked(reduce_motion) {
            blur_amount.set(to);
        } else {
            let _ = blur_amount.timing(to, BLUR_FADE_MS, Easing::EaseOut);
        }
    });
    let line_ys: Rc<RefCell<Vec<f32>>> = Rc::new(RefCell::new(vec![0.0; count]));
    #[cfg(target_os = "windows")]
    let line_hs: Rc<RefCell<Vec<f32>>> = Rc::new(RefCell::new(vec![0.0; count]));
    let laid_seen: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(vec![false; count]));
    let laid_count = Rc::new(std::cell::Cell::new(0_usize));
    let last_scroll_y = Rc::new(std::cell::Cell::new(0.0_f32));
    // Windows / shader 平台：按滚动位置维护视口内（含上下缓冲）的可见区间，
    // 把远处行设为 `invisible()`（保留布局高度、跳过绘制）；Windows 顺带只为
    // 视口内可见行更新边缘淡出与行级模糊，避免遍历整首歌词。
    let vis_flags: Vec<Signal<bool>> = if cull_offscreen {
        (0..count).map(|_| signal(true)).collect()
    } else {
        Vec::new()
    };
    #[cfg(target_os = "windows")]
    let row_fades: Vec<Signal<f32>> = (0..count).map(|_| signal(1.0_f32)).collect();
    #[cfg(target_os = "windows")]
    let row_blurs: Vec<Signal<f32>> = if use_blur {
        (0..count).map(|_| signal(0.0_f32)).collect()
    } else {
        Vec::new()
    };
    let vis_range = Rc::new(std::cell::Cell::new((0_usize, count)));
    let update_visible = {
        let line_ys = line_ys.clone();
        #[cfg(target_os = "windows")]
        let line_hs = line_hs.clone();
        let vis_flags = vis_flags.clone();
        #[cfg(target_os = "windows")]
        let row_fades = row_fades.clone();
        #[cfg(target_os = "windows")]
        let row_blurs = row_blurs.clone();
        let vis_range = vis_range.clone();
        Rc::new(move |scroll_y: f32| {
            if !cull_offscreen || count == 0 || !laid_ready.get_untracked() {
                return;
            }
            let vp = viewport.get_untracked();
            if vp <= 0.0 {
                return;
            }
            let ys = line_ys.borrow();
            let min_y = scroll_y - vp * ANCHOR - 300.0;
            let max_y = scroll_y + vp * (1.0 - ANCHOR) + 250.0;
            let lo = ys.partition_point(|&y| y < min_y).saturating_sub(1);
            let hi = (ys.partition_point(|&y| y <= max_y) + 1).min(count);
            let (old_lo, old_hi) = vis_range.get();
            if (lo, hi) != (old_lo, old_hi) {
                vis_range.set((lo, hi));
                for i in old_lo..old_hi {
                    if i < lo || i >= hi {
                        vis_flags[i].set(false);
                    }
                }
                for i in lo..hi {
                    if i < old_lo || i >= old_hi {
                        vis_flags[i].set(true);
                    }
                }
            }
            #[cfg(target_os = "windows")]
            {
                let hs = line_hs.borrow();
                let is_hovered = hovered.get_untracked();
                for i in lo..hi {
                    let center = vp * ANCHOR + ys[i] + hs[i] * 0.5 - scroll_y;
                    let fade = edge_fade(center, vp);
                    if (row_fades[i].get_untracked() - fade).abs() > 1e-3 {
                        row_fades[i].set(fade);
                    }
                    if let Some(blur_sig) = row_blurs.get(i) {
                        let b = if is_hovered { 0.0 } else { blur_at(center, vp) };
                        if (blur_sig.get_untracked() - b).abs() > 1e-3 {
                            blur_sig.set(b);
                        }
                    }
                }
            }
        })
    };
    let target: Rc<RefCell<Option<(f32, std::time::Instant)>>> = Rc::default();
    let resume: Rc<RefCell<Option<Spawned<()>>>> = Rc::default();
    let last_followed = Rc::new(RefCell::new(-1_i64));
    // 点击某行后等高亮换到这一行再跟随：seek 生效前 `active_line` 还是
    // 旧行，跟随它会先滚回旧行再滚到目标行。
    let pending: Rc<RefCell<Option<(i64, std::time::Instant)>>> = Rc::default();
    let prev_time = Rc::new(RefCell::new(-1.0_f64));
    // 配置拆成只读 memo：播放计数等无关的 `s.library` 更新不会唤醒每一行。
    let align = computed(move || s.library.with(|l| l.config.lyrics_align));
    let show_timestamps = computed(move || s.library.with(|l| l.config.lyrics_timestamps));
    // 每行各自持有一个高亮信号，由单个 effect 仅翻转旧行与新行（O(1)），
    // 避免每次换行唤醒全部行的响应式闭包。
    let initial_active = if timed {
        untracked(|| s.active_line.get())
    } else {
        -1
    };
    let active_rows: Vec<Signal<bool>> = if timed {
        (0..count)
            .map(|idx| signal(idx as i64 == initial_active))
            .collect()
    } else {
        Vec::new()
    };
    if timed {
        let flags = active_rows.clone();
        let prev = std::cell::Cell::new(initial_active);
        effect(move || {
            let next = s.active_line.get();
            let old = prev.replace(next);
            if old == next {
                return;
            }
            if let Ok(i) = usize::try_from(old)
                && let Some(sig) = flags.get(i)
            {
                sig.set(false);
            }
            if let Ok(i) = usize::try_from(next)
                && let Some(sig) = flags.get(i)
            {
                sig.set(true);
            }
        });
    }

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

    // 鼠标移出歌词区后 3 秒恢复跟随：直接滚回当前高亮行（只把
    // `follow` 置回去不会动——高亮行没变时时间推进的 effect 会跳过）。
    let schedule_resume = {
        let resume = resume.clone();
        let last_followed = last_followed.clone();
        let scroll_to_line = scroll_to_line.clone();
        Rc::new(move || {
            if !timed || follow.get_untracked() || resume.borrow().is_some() {
                return;
            }
            let last_followed = last_followed.clone();
            let scroll_to_line = scroll_to_line.clone();
            *resume.borrow_mut() = Some(set_timeout(Duration::from_secs(3), move || {
                let index = s.active_line.get_untracked();
                *last_followed.borrow_mut() = index;
                follow.set(true);
                if index >= 0 {
                    scroll_to_line(index);
                }
            }));
        })
    };
    let schedule_when_idle = {
        let dragging = dragging.clone();
        let schedule_resume = schedule_resume.clone();
        Rc::new(move || {
            if !hovered.get_untracked() && !dragging.get() {
                schedule_resume();
            }
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
        let last_scroll_y = last_scroll_y.clone();
        let update_visible = update_visible.clone();
        spawn(async move {
            let start = std::time::Instant::now();
            let mut settled = 0;
            loop {
                next_frame().await;
                let ready = viewport.get_untracked() > 0.0 && laid_ready.get_untracked();
                if ready {
                    let index = s.active_line.get_untracked();
                    if index < 0 {
                        break;
                    }
                    if let Some(y) = offset_of(index) {
                        *last_followed.borrow_mut() = index;
                        *target.borrow_mut() = Some((y, std::time::Instant::now()));
                        last_scroll_y.set(y);
                        scroller.scroll_to(point(px(0.0), px(y)), false);
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
            update_visible(last_scroll_y.get());
            placed.set(true);
        });
    }

    // 时间推进：行变化或 seek 跳变时滚动跟随。
    {
        let last_followed = last_followed.clone();
        let pending = pending.clone();
        let scroll_to_line = scroll_to_line.clone();
        effect(move || {
            let ready = viewport.get() > 0.0 && laid_ready.get();
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
        let last_scroll_y = last_scroll_y.clone();
        let update_visible = update_visible.clone();
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
                    last_scroll_y.set(y);
                    update_visible(y);
                    scroller.scroll_to(point(px(0.0), px(y)), false);
                }
            });
        });
    }

    let interrupt = {
        let target = target.clone();
        let resume = resume.clone();
        let schedule_when_idle = schedule_when_idle.clone();
        Rc::new(move || {
            *target.borrow_mut() = None;
            follow.set(false);
            if let Some(task) = resume.borrow_mut().take() {
                task.cancel();
            }
            schedule_when_idle();
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
        let active_sig = active_rows.get(index).copied();
        let vis_sig = vis_flags.get(index).copied();
        #[cfg(target_os = "windows")]
        let row_fade_sig = row_fades[index];
        #[cfg(target_os = "windows")]
        let row_blur_sig = row_blurs.get(index).copied();
        let line_ys = line_ys.clone();
        #[cfg(target_os = "windows")]
        let line_hs = line_hs.clone();
        let laid_seen = laid_seen.clone();
        let laid_count = laid_count.clone();
        let resume = resume.clone();
        let last_followed = last_followed.clone();
        let pending = pending.clone();
        let scroll_to_line = scroll_to_line.clone();
        let orig_text = text(content)
            .style_with(move |st| text_align(st, align.get()))
            .text_size(px(if metadata { 16.0 } else { LYRIC_SIZE }))
            .line_height(px(if metadata { 24.0 } else { LYRIC_SIZE * 1.3 }))
            .font_weight(if metadata {
                FontWeight::MEDIUM
            } else {
                FontWeight::BOLD
            })
            .text_color(white_a(1.0));
        // 高亮换行的明暗渐变：文字颜色不在样式过渡里，画成纯白、用不透明度
        // 走原生过渡（未高亮 IDLE_ALPHA、高亮 ACTIVE_ALPHA）；无时间轴歌词
        // 静态高亮，不挂响应式与过渡。
        let orig_text = if let Some(active) = active_sig {
            orig_text
                .opacity(move || original_opacity(true, active.get()))
                .transition_opacity()
                .ease_out()
                .duration(SCROLL_MS)
        } else {
            orig_text.opacity(original_opacity(false, false))
        };
        let content_view = view()
            .max_w(px(LYRICS))
            .style_with(move |st| match align.get() {
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
            .child(orig_text)
            .child(line.trans_content.clone().map(|trans| {
                text(trans)
                    .style_with(move |st| text_align(st, align.get()))
                    .text_size(px(TRANSLATION_SIZE))
                    .line_height(px(TRANSLATION_SIZE * 1.4))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(white_a(TRANSLATION_ALPHA))
            }));
        #[cfg(target_os = "windows")]
        let content_view = if let Some(blur_sig) = row_blur_sig {
            content_view.style_with(move |st| {
                let b = blur_sig.get();
                if b > 0.0 { st.blur(px(b)) } else { st }
            })
        } else {
            content_view
        };
        // 时间戳：占位在行尾（右对齐时借 flex_row_reverse 翻到行首）、垂直居中，
        // 悬停浮现。仅可 seek 的行挂单个槽位；开关关掉时不占槽、不挡点击。
        let stamp = seekable.then(|| {
            when(
                move || show_timestamps.get(),
                move || {
                    view()
                        .flex_shrink_0()
                        .self_center()
                        .h(px(22.0))
                        .px(px(8.0))
                        .style_with(move |st| {
                            if align.get() == LyricsAlign::Right {
                                st.mr(px(6.0))
                            } else {
                                st.ml(px(6.0))
                            }
                        })
                        .justify_center()
                        .rounded(px(6.0))
                        .bg(AdaptiveColor::rgb(0x1c2230, 0x1c2230).opacity(0.75))
                        .pointer_events(PointerEvents::None)
                        .opacity(0.0)
                        .group_hover(|st| st.opacity(1.0))
                        .transition_opacity()
                        .duration(150.0)
                        .child(
                            text(format_time(time))
                                .text_size(px(12.0))
                                .line_height(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .tabular_nums()
                                .text_color(white_a(0.8)),
                        )
                },
            )
        });
        // `on_layout` 只能挂在裸 view 上（`Interactive` 不在组件元素上）：
        // 外层量位置、包按钮。按钮管按压与无障碍，时间戳槽在按钮内容行里。
        let row = view().w_full().group();
        let row = if let Some(vis) = vis_sig {
            row.style_when(move || !vis.get(), |st| st.invisible())
        } else {
            row
        };
        #[cfg(target_os = "windows")]
        let row = row.opacity(move || row_fade_sig.get());
        row.child(
            nsh_base::button()
                .w_full()
                .accessibility_label(if seekable {
                    format!("跳到 {}", format_time(time))
                } else {
                    String::new()
                })
                // 颜色在文字上：原文随高亮变化，译文始终不变。
                .child(
                    view()
                        .w_full()
                        .style_with(move |st| {
                            if align.get() == LyricsAlign::Right {
                                st.flex_row_reverse()
                            } else {
                                st.flex_row()
                            }
                        })
                        .items_center()
                        .child(view().flex_1().min_w(px(0.0)).child(content_view))
                        .child(stamp),
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
            line_ys.borrow_mut()[index] = frame.origin.y.0;
            #[cfg(target_os = "windows")]
            {
                line_hs.borrow_mut()[index] = frame.size.height.0;
            }
            if !laid_ready.get_untracked() {
                let mut seen = laid_seen.borrow_mut();
                if !seen[index] {
                    seen[index] = true;
                    let n = laid_count.get() + 1;
                    laid_count.set(n);
                    drop(seen);
                    if n >= count {
                        laid_ready.set(true);
                    }
                }
            }
        })
    });

    let user_scroll = interrupt.clone();
    let drag = interrupt;
    let drag_start_state = dragging.clone();
    let drag_end_state = dragging.clone();
    let drag_end_schedule = schedule_when_idle.clone();
    let enter_resume = resume.clone();
    let leave_schedule = schedule_when_idle;
    #[cfg(not(target_os = "windows"))]
    let (blur_in, blur_out) = (fade_blur.clone(), fade_blur);
    #[cfg(target_os = "windows")]
    let (win_enter_scroll, win_enter_update) = (last_scroll_y.clone(), update_visible.clone());
    #[cfg(target_os = "windows")]
    let (win_leave_scroll, win_leave_update) = (last_scroll_y.clone(), update_visible.clone());
    let resize_scroll_y = last_scroll_y.clone();
    let resize_update = update_visible.clone();
    let scroll_view = scroll()
        .flex_1()
        .opacity(move || if placed.get() { 1.0_f32 } else { 0.0 })
        .scrollbar(false)
        .element_ref(&scroller)
        .on_layout(move |frame: &Bounds<Points>| {
            let h = frame.size.height.0;
            if (viewport.get_untracked() - h).abs() > 0.5 {
                viewport.set(h);
                resize_update(resize_scroll_y.get());
            }
        })
        .on_scroll(move |e: &ScrollEvent| {
            let y = e.offset.y.0;
            last_scroll_y.set(y);
            update_visible(y);
            // 程序滚动：动画进行中（约 0.7 s 内）或已到目标附近。
            let ours = target
                .borrow()
                .is_some_and(|(t, at)| at.elapsed().as_millis() < 700 || (y - t).abs() < 64.0);
            if !ours && follow.get_untracked() {
                user_scroll();
            }
        })
        .on_scroll_drag_start(move |_: &Point<Points>| {
            drag_start_state.set(true);
            drag();
        })
        .on_scroll_drag_end(move |_: &Point<Points>| {
            drag_end_state.set(false);
            drag_end_schedule();
        })
        .child(
            view()
                .w_full()
                .child(view().h(move || px(viewport.get() * ANCHOR)))
                .child(view().w_full().children(rows))
                .child(view().h(move || px(viewport.get() * (1.0 - ANCHOR)))),
        );
    #[cfg(target_os = "macos")]
    let body: AnyElement = if use_blur {
        native_blur_view(scroll_view, viewport, &blur_amount).into_element()
    } else {
        edge_fade_view(scroll_view).into_element()
    };
    #[cfg(target_os = "windows")]
    let body: AnyElement = scroll_view.into_element();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let body: AnyElement = if use_blur {
        blur_effect(scroll_view, &blur_amount).into_element()
    } else {
        edge_fade_view(scroll_view).into_element()
    };
    view()
        .flex_1()
        .w_full()
        .on_mouse_enter(move |_: &MouseEvent| {
            hovered.set(true);
            if let Some(task) = enter_resume.borrow_mut().take() {
                task.cancel();
            }
            #[cfg(not(target_os = "windows"))]
            blur_in(0.0);
            #[cfg(target_os = "windows")]
            if use_blur {
                win_enter_update(win_enter_scroll.get());
            }
        })
        .on_mouse_leave(move |_: &MouseEvent| {
            hovered.set(false);
            #[cfg(not(target_os = "windows"))]
            blur_out(1.0);
            #[cfg(target_os = "windows")]
            if use_blur {
                win_leave_update(win_leave_scroll.get());
            }
            leave_schedule();
        })
        .child((!timed).then(|| {
            text("歌词无时间信息")
                .text_center()
                .text_size(px(11.0))
                .line_height(px(11.0))
                .letter_spacing(px(0.6))
                .text_color(white_a(0.4))
                .pb(px(8.0))
        }))
        .child(body)
}
