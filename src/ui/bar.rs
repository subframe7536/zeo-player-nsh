//! 底部浮动播放条：封面与曲名、播放控制与进度、音量与展开。

use std::rc::Rc;

use nsh_core::prelude::*;

use super::icons::Icon;
use super::parts::{cover, icon_button, progress_fraction, shown_time, slider};
use super::store::{format_time, player, seek_to, set_volume, store, toggle_mute};
use super::theme::*;
use crate::audio::LoopMode;

pub fn player_bar(on_expand: Rc<dyn Fn()>, queue_open: Signal<bool>) -> View {
    let s = store();
    let has_track = move || s.track.with(|t| t.path.is_some());
    let artwork = move || s.track.with(|t| t.artwork.clone());
    let expand = on_expand.clone();

    let now = view()
        .w(px(260.0))
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .min_w(px(0.0))
        .child(
            view()
                .role(AccessibilityRole::Button)
                .label("展开播放页")
                .rounded(px(8.0))
                .active(|st| st.opacity(0.7))
                .on_press(move || expand())
                .child(cover(artwork, Some(48.0), 8.0).shadow_sm()),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    text(move || s.track.with(|t| if t.path.is_some() { t.title.clone() } else { "未在播放".into() }))
                        .number_of_lines(1)
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg()),
                )
                .child(
                    text(move || {
                        s.track.with(|t| match (&t.path, &t.error) {
                            (_, Some(error)) => error.clone(),
                            (Some(_), None) => t.artist.clone(),
                            (None, None) => "从曲库选一首开始".into(),
                        })
                    })
                    .number_of_lines(1)
                    .text_size(px(12.0))
                    .text_color(muted_fg()),
                ),
        );

    // 与全屏页的控制一致：线框的上一首 / 下一首，播放键没有底色、图标在
    // 播放与暂停之间形变（`icons::play_pause`）。
    let play = view()
        .size(px(40.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .role(AccessibilityRole::Button)
        .label("播放/暂停")
        .hover(|st| st.bg(hover()))
        .active(|st| st.scale(0.92))
        .transition_transform()
        .duration(120.0)
        .on_press(|| player().toggle())
        .child(super::icons::play_pause(28.0, move || s.playback.with(|p| p.playing), fg));

    let mode_icon = move || match s.playback.with(|p| p.loop_mode) {
        LoopMode::List => Icon::Repeat,
        LoopMode::Single => Icon::RepeatOne,
        LoopMode::Random => Icon::Shuffle,
    };
    // 播放键最大，上一首 / 下一首次之，两侧的循环模式与喜欢最小。
    let controls = view()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .child(icon_button(
            mode_icon,
            14.0,
            "循环模式（顺序 / 单曲 / 随机）",
            move || if s.playback.with(|p| p.loop_mode) == LoopMode::List { muted_fg() } else { primary() },
            super::store::cycle_loop_mode,
        ))
        .child(icon_button(|| Icon::PrevLine, 15.0, "上一首", fg, || player().previous()))
        .child(play)
        .child(icon_button(|| Icon::NextLine, 15.0, "下一首", fg, || player().next()))
        .child(icon_button(
            || if super::store::current_liked() { Icon::HeartFilled } else { Icon::Heart },
            14.0,
            "喜欢",
            like_red,
            super::store::toggle_like_current,
        ));

    let duration = move || s.playback.with(|p| p.duration);
    let progress = view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .w_full()
        .max_w(px(560.0))
        .child(
            text(move || format_time(shown_time()))
                .w(px(40.0))
                .text_right()
                .text_size(px(11.0))
                .tabular_nums()
                .text_color(muted_fg()),
        )
        .child(
            slider(
                progress_fraction,
                muted(),
                fg(),
                fg(),
                move |f| s.seek_preview.set(Some(f * duration())),
                move |f| {
                    if has_track() {
                        seek_to(f * duration())
                    } else {
                        s.seek_preview.set(None)
                    }
                },
            )
            .flex_1(),
        )
        .child(
            text(move || format_time(duration()))
                .w(px(40.0))
                .text_size(px(11.0))
                .tabular_nums()
                .text_color(muted_fg()),
        );

    let center = view()
        .flex_1()
        .min_w(px(0.0))
        .items_center()
        .gap(px(2.0))
        .child(controls)
        .child(progress);

    let volume = move || s.playback.with(|p| p.volume);
    let right = view()
        .w(px(260.0))
        .flex_row()
        .items_center()
        .justify_end()
        .gap(px(6.0))
        .child(icon_button(
            move || if volume() <= 0.0 { Icon::Mute } else { Icon::Volume },
            16.0,
            "静音",
            muted_fg,
            toggle_mute,
        ))
        .child(slider(volume, muted(), muted_fg(), fg(), set_volume, set_volume).w(px(96.0)))
        .child(super::queue::queue_button(queue_open))
        .child(icon_button(|| Icon::Expand, 14.0, "全屏播放", muted_fg, move || on_expand()));

    view()
        .absolute()
        .left(px(16.0))
        .right(px(16.0))
        .bottom(px(16.0))
        .h(px(76.0))
        .flex_row()
        .items_center()
        .gap(px(16.0))
        .px(px(14.0))
        .rounded(px(16.0))
        .bg(bar())
        .border_1()
        .border_color(border())
        .shadow_lg()
        .backdrop_blur(px(24.0))
        .child(now)
        .child(center)
        .child(right)
}
