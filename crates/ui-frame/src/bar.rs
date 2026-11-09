//! 底部浮动播放条：封面与曲名、播放控制与进度、音量与展开。

use std::rc::Rc;

use nsh_base::{button, popover};
use nsh_core::element::ComponentBuilder;
use nsh_core::prelude::*;

use zeo_engine::audio::LoopMode;
use zeo_ui_components::button::{RADIUS_MD, Size, Variant, icon_button, shell, tipped};
use zeo_ui_components::cover::cover;
use zeo_ui_components::icons::{Icon, icon_dyn};
use zeo_ui_components::slider::{SliderStyle, slider};
use zeo_ui_store::theme::*;
use zeo_ui_store::{
    cover_for_always, format_time, player, progress_fraction, seek_to, set_volume, shown_time,
    store, toggle_mute,
};

/// 播放条左右两段的宽度：窗口变窄时一起收缩（等宽，播放控制保持居中），
/// 给中间的进度条留地方。断点走 `layout_width`——窗口宽度一变即换档。
fn side_width() -> f32 {
    let w = layout_width();
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
    layout_width() >= 1080.0
}

/// 进度条两侧的时间（窄了只留滑条）。
fn fits_times() -> bool {
    layout_width() >= 720.0
}

pub fn player_bar(on_expand: Rc<dyn Fn()>, queue_open: Signal<bool>) -> View {
    let s = store();
    let has_track = move || s.track.with(|t| t.path.is_some());
    // 48pt 用封面的小档（96px）：720px 大封面留给全屏页；「列表显示
    // 封面」开关只管列表，播放条一直显示当前曲目封面。
    let artwork = move || {
        s.track
            .with(|t| t.path.as_deref().and_then(|p| cover_for_always(p, 48.0)))
    };
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
        .on_press(|_: &()| zeo_ui_store::toggle())
        .child(zeo_ui_components::icons::play_pause(
            28.0,
            zeo_ui_store::shown_playing,
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
                zeo_ui_store::cycle_loop_mode,
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
                    if zeo_ui_store::current_liked() {
                        Icon::HeartFilled
                    } else {
                        Icon::Heart
                    }
                },
                14.0,
                "喜欢",
                like_red,
                zeo_ui_store::toggle_like_current,
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
                SliderStyle::HORIZONTAL,
                "播放进度",
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
                .child(
                    slider(
                        SliderStyle::HORIZONTAL,
                        "音量",
                        volume,
                        input(),
                        muted_fg(),
                        fg(),
                        set_volume,
                        set_volume,
                    )
                    .w(px(96.0)),
                )
        }))
        .child(when(
            move || !fits_volume(),
            move || volume_popover(volume_open),
        ))
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
    // 与所有弹出层互斥（菜单、另一个 popover）：令牌被夺走时本地 `open`
    // 复位，触发器高亮随之熄灭。
    let gate = zeo_ui_store::menu_control();
    effect(gate.evicted(open));
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
        .open(gate.open_with(open))
        .on_open_change(gate.popover_change(open))
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
                .child(
                    slider(
                        SliderStyle::VERTICAL,
                        "音量",
                        volume,
                        input(),
                        muted_fg(),
                        fg(),
                        set_volume,
                        set_volume,
                    )
                    .h(px(112.0)),
                )
                .child(icon_button(glyph, 15.0, "静音", muted_fg, toggle_mute)),
        )
        .into_element()
}
