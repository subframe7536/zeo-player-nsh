use super::*;

/// 歌词列最大宽度。
pub(crate) const LYRICS: f32 = 520.0;
/// 当前歌词行停在视口从上往下的这个比例处（偏上，下方留出更多后续歌词）。
pub(crate) const ANCHOR: f32 = 0.35;

// --- 歌词 ---------------------------------------------------------------------

/// 是否显示歌词区：没在播放或这首没有歌词时不显示。换歌后歌词载入前
/// （`lyrics` 为 `None`）保持上一首的结果，不在两种布局之间来回跳。
pub(crate) fn lyrics_shown() -> Signal<bool> {
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
pub(crate) fn fmt_offset(ms: i64) -> String {
    let sign = if ms < 0 { "−" } else { "+" };
    format!("{sign}{:.1}s", ms.unsigned_abs() as f64 / 1000.0)
}

/// 右边缘的偏移栏（占位、常显）：竖排 [+] 数值 [−]，数值点开横向滑条
/// 弹层（同音量弹层的路子）；± 按 0.5 秒步进，全局偏移与「重置」在弹层里。
pub(crate) fn offset_rail() -> AnyElement {
    let s = store();
    let open = signal(false);
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
                .open(move || Some(open.get()))
                .on_open_change(move |e: &nsh_base::PopoverEvent| {
                    if let nsh_base::PopoverEvent::OpenChange(next, _) = e {
                        open.set(*next);
                    }
                })
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
