//! 底部浮动播放条：封面与曲名、播放控制与进度、音量与展开。

use std::rc::Rc;

use nsh_base::{PopoverEvent, button, popover};
use nsh_core::element::ComponentBuilder;
use nsh_core::prelude::*;

use super::super::components::button::{RADIUS_MD, Size, Variant, icon_button, shell, tipped};
use super::super::components::cover::cover;
use super::super::components::icons::{Icon, icon_dyn};
use super::super::components::slider::{slider, vslider};
use super::super::store::{
    format_time, player, progress_fraction, seek_to, set_volume, shown_time, store, toggle_mute,
};
use super::super::theme::*;
use crate::engine::audio::LoopMode;

/// 播放条左右两段的宽度：窗口变窄时一起收缩（等宽，播放控制保持居中），
/// 给中间的进度条留地方。
fn side_width() -> f32 {
    let w = window().width.0;
    if w >= 1080.0 {
        260.0
    } else if w >= 900.0 {
        180.0
    } else {
        112.0
    }
}

/// 音量滑条只在两边够宽时显示（窄了只剩静音图标）。
fn fits_volume() -> bool {
    window().width.0 >= 1080.0
}

/// 进度条两侧的时间（窄了只留滑条）。
fn fits_times() -> bool {
    window().width.0 >= 720.0
}

pub fn player_bar(on_expand: Rc<dyn Fn()>, queue_open: Signal<bool>) -> View {
    let s = store();
    let has_track = move || s.track.with(|t| t.path.is_some());
    let artwork = move || s.track.with(|t| t.artwork.clone());
    let expand = on_expand.clone();

    let now = view()
        .w(move || px(side_width()))
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .min_w(px(0.0))
        .child(
            button()
                .accessibility_label("展开播放页")
                .rounded(px(RADIUS_MD))
                .active(|st| st.opacity(0.7))
                .on_press(move |_: &()| expand())
                .child(cover(artwork, Some(48.0), RADIUS_MD).shadow_sm()),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(3.0))
                .child(
                    text(move || {
                        s.track.with(|t| {
                            if t.path.is_some() {
                                t.title.clone()
                            } else {
                                "未在播放".into()
                            }
                        })
                    })
                    .number_of_lines(1)
                    .text_size(px(14.0))
                    .line_height(px(17.0))
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
                    .line_height(px(15.0))
                    .text_color(muted_fg()),
                ),
        );

    // 与全屏页的控制一致：线框的上一首 / 下一首，播放键没有底色、图标在
    // 播放与暂停之间形变（`icons::play_pause`）。
    let play = shell(Variant::Ghost, Size::IconLg)
        .accessibility_label("播放/暂停")
        .active(|st| st.scale(0.92))
        .transition_transform()
        .duration(120.0)
        .on_press(|_: &()| player().toggle())
        .child(super::super::components::icons::play_pause(
            28.0,
            move || s.playback.with(|p| p.playing),
            fg,
        ));

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
        .child(tipped(
            icon_button(
                mode_icon,
                14.0,
                "循环模式（顺序 / 单曲 / 随机）",
                move || {
                    if s.playback.with(|p| p.loop_mode) == LoopMode::List {
                        muted_fg()
                    } else {
                        primary()
                    }
                },
                super::super::store::cycle_loop_mode,
            ),
            "循环模式",
        ))
        .child(tipped(
            icon_button(
                || Icon::PrevLine,
                15.0,
                "上一首",
                fg,
                || player().previous(),
            ),
            "上一首",
        ))
        .child(play)
        .child(tipped(
            icon_button(|| Icon::NextLine, 15.0, "下一首", fg, || player().next()),
            "下一首",
        ))
        .child(tipped(
            icon_button(
                || {
                    if super::super::store::current_liked() {
                        Icon::HeartFilled
                    } else {
                        Icon::Heart
                    }
                },
                14.0,
                "喜欢",
                like_red,
                super::super::store::toggle_like_current,
            ),
            "喜欢",
        ));

    let duration = move || s.playback.with(|p| p.duration);
    let progress = view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .w_full()
        .max_w(px(560.0))
        .child(when(fits_times, move || {
            text(move || format_time(shown_time()))
                .w(px(40.0))
                .text_right()
                .text_size(px(11.0))
                .line_height(px(11.0))
                .tabular_nums()
                .text_color(muted_fg())
        }))
        .child(
            slider(
                progress_fraction,
                input(),
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
        .child(when(fits_times, move || {
            text(move || format_time(duration()))
                .w(px(40.0))
                .text_size(px(11.0))
                .line_height(px(11.0))
                .tabular_nums()
                .text_color(muted_fg())
        }));

    let center = view()
        .flex_1()
        .min_w(px(0.0))
        .items_center()
        .gap(px(2.0))
        .child(controls)
        .child(progress);

    let volume = move || s.playback.with(|p| p.volume);
    let volume_open = signal(false);
    let right = view()
        .w(move || px(side_width()))
        .flex_row()
        .items_center()
        .justify_end()
        .gap(px(6.0))
        .child(when(fits_volume, move || {
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(tipped(
                    icon_button(
                        move || {
                            if volume() <= 0.0 {
                                Icon::Mute
                            } else {
                                Icon::Volume
                            }
                        },
                        16.0,
                        "静音",
                        muted_fg,
                        toggle_mute,
                    ),
                    "静音",
                ))
                .child(slider(volume, input(), muted_fg(), fg(), set_volume, set_volume).w(px(96.0)))
        }))
        .child(when(move || !fits_volume(), move || {
            volume_popover(volume_open)
        }))
        .child(tipped(super::queue::queue_button(queue_open), "播放队列"))
        .child(tipped(
            icon_button(
                || Icon::Expand,
                14.0,
                "全屏播放",
                muted_fg,
                move || on_expand(),
            ),
            "全屏播放",
        ));

    let bar_view = view()
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
        .bg(move || bar())
        .transition_colors()
        .duration(300.0)
        .border_1()
        .border_color(border())
        .shadow_lg();
    // Windows 上不加背景模糊：nsh-platform-windows 0.3.0 的局部重绘只清空并
    // 重画变化的区域，背景模糊却会采样区域外上一帧的像素（已经盖着播放条
    // 自己的半透明底色），于是进度、时间、按钮悬停等每次局部重绘都在条上
    // 留下一块更亮的矩形。见 README「待上游」。
    #[cfg(not(target_os = "windows"))]
    let bar_view = bar_view.backdrop_blur(px(24.0));
    bar_view.child(now).child(center).child(right)
}

/// 窄窗口（音量滑条放不下）时的音量：按钮打开与队列一样的 `popover`，
/// 里面是竖向音量滑条与静音键。触发器用普通 view（同队列按钮：
/// nsh-base 的 button 会吃掉按压，popover 收不到）。
fn volume_popover(open: Signal<bool>) -> AnyElement {
    let s = store();
    let volume = move || s.playback.with(|p| p.volume);
    let glyph = move || {
        if volume() <= 0.0 {
            Icon::Mute
        } else {
            Icon::Volume
        }
    };
    let trigger = view()
        .size(px(32.0))
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_MD))
        .role(AccessibilityRole::Button)
        .label("音量")
        .style_when(move || open.get(), |st| st.bg(hover()))
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.8))
        .transition_colors()
        .duration(150.0)
        .child(icon_dyn(glyph, 16.0, move || {
            if open.get() { primary() } else { muted_fg() }
        }));
    popover()
        .open(move || Some(open.get()))
        .on_open_change(move |e: &PopoverEvent| {
            if let PopoverEvent::OpenChange(next, _) = e {
                open.set(*next);
            }
        })
        .placement(Placement::Top)
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
                .items_center()
                .px(px(8.0))
                .py(px(10.0))
                .gap(px(8.0))
                .child(vslider(volume, input(), muted_fg(), fg(), set_volume, set_volume))
                .child(icon_button(glyph, 15.0, "静音", muted_fg, toggle_mute)),
        )
        .into_element()
}
