use super::*;

/// 歌词列最大宽度。
pub(crate) const LYRICS: f32 = 520.0;
/// 当前歌词行停在视口从上往下的这个比例处（偏上，下方留出更多后续歌词）。
pub(crate) const ANCHOR: f32 = 0.35;
/// 歌词列（滚动区 + 偏移栏）的命名 group：偏移栏悬停歌词区才浮现。
/// 用命名 group 跳过歌词行自己的 `.group()`。
pub(crate) const LYRICS_GROUP: &str = "lyrics";
/// 偏移栏占位宽度（窄条，含与窗口右缘的间距）。
const RAIL_W: f32 = 48.0;
/// 数值格宽度：10 pt 字放得下最长的「−30.0s」一行（不换行），数值变化时
/// 栏宽不变；只比 ± 按钮（28）略宽，按钮两侧不显得空。
const TRIGGER_W: f32 = 34.0;

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

/// 右边缘的偏移栏（占位，悬停歌词区或弹层打开时浮现）：竖排 [+] 数值 [−]，
/// 数值悬停变铅笔、点开竖向滑条弹层（同音量弹层的路子）；± 按 0.5 秒步进，
/// 弹层里只有滑条与重置（数值就在旁边的栏上），全局偏移非零时多两行小字。
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
    // 同队列 / 音量按钮）。悬停时数值淡出、换成铅笔图标（两者叠在同一格
    // 里，栏宽不变）；弹层打开后换回数值，拖滑条时能看到当前值。
    let hovered = signal(false);
    let pen = move || hovered.get() && !open.get();
    let trigger = view()
        .w(px(TRIGGER_W))
        .h(px(28.0))
        .items_center()
        .justify_center()
        .role(AccessibilityRole::Button)
        .label("调整歌词时间偏移")
        .active(|st| st.opacity(0.8))
        .on_mouse_enter(move |_: &MouseEvent| hovered.set(true))
        .on_mouse_leave(move |_: &MouseEvent| hovered.set(false))
        .child(
            text(move || value())
                .whitespace_nowrap()
                .text_size(px(10.0))
                .line_height(px(10.0))
                .tabular_nums()
                .text_color(white_a(0.8))
                .opacity(move || if pen() { 0.0_f32 } else { 1.0 })
                .transition_opacity()
                .duration(150.0),
        )
        .child(
            view()
                .absolute()
                .inset_0()
                .items_center()
                .justify_center()
                .pointer_events(PointerEvents::None)
                .opacity(move || if pen() { 1.0_f32 } else { 0.0 })
                .transition_opacity()
                .duration(150.0)
                .child(icon(Icon::Edit, 14.0, || white_a(0.85))),
        );
    // 弹层：窄竖条，上提前、下延后（同栏上 + / − 的方向），底部重置；
    // 重置常驻（偏移为 0 时禁用），弹层尺寸不随数值变。
    let sheet = view()
        .items_center()
        .px(px(8.0))
        .pt(px(12.0))
        .pb(px(6.0))
        .gap(px(6.0))
        .child(
            slider(
                SliderStyle::VERTICAL,
                "歌词偏移",
                fraction,
                input(),
                fg(),
                fg(),
                apply,
                apply,
            )
            .h(px(140.0)),
        )
        .child(tipped(
            icon_button(
                || Icon::Refresh,
                12.0,
                "重置歌词偏移",
                muted_fg,
                || nudge_lyrics_offset(None),
            )
            .disabled(move || s.lyrics_offset.get() == 0),
            "重置",
        ))
        .child(dynamic(move || {
            let g = s.library.with(|l| l.config.global_lyrics_offset_ms);
            // 分两行，不把窄竖条撑宽。
            (g != 0).then(|| {
                let line = |t: String| {
                    text(t)
                        .whitespace_nowrap()
                        .text_size(px(10.0))
                        .line_height(px(10.0))
                        .tabular_nums()
                        .text_color(muted_fg())
                };
                view()
                    .items_center()
                    .gap(px(3.0))
                    .pb(px(4.0))
                    .child(line("全局".to_owned()))
                    .child(line(fmt_offset(g)))
                    .into_element()
            })
        }));
    let panel = view()
        .items_center()
        .gap(px(2.0))
        .py(px(3.0))
        .px(px(3.0))
        .rounded(px(10.0))
        .bg(white_a(0.06))
        // 平时隐藏，悬停歌词区（或键盘焦点在栏内）浮现；弹层打开时指针在
        // 弹层上、不在歌词区里，靠 `open` 保持可见。
        .opacity(0.0)
        .group_hover_named(LYRICS_GROUP, |st| st.opacity(1.0))
        .group_focus_within_named(LYRICS_GROUP, |st| st.opacity(1.0))
        .style_when(move || open.get(), |st| st.opacity(1.0))
        .transition_opacity()
        .duration(200.0)
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
                .child(sheet),
        )
        .child(
            on_dark(28.0)
                .accessibility_label("歌词延后")
                .on_press(|_: &()| nudge_lyrics_offset(Some(-500)))
                .child(icon(Icon::Minus, 14.0, || white_a(0.85))),
        );
    view()
        .h_full()
        .w(px(RAIL_W))
        .flex_shrink_0()
        .justify_center()
        .items_center()
        .child(panel)
        .into_element()
}
