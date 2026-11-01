//! 播放队列：播放条上的按钮打开的 nsh-base `popover`（shadcn Popover 的
//! 外观：`rounded-md border bg-popover shadow-md`），在按钮上方、右边对齐。
//! 列出当前队列，正在播放的一首高亮；点一行跳到那首。点外面或 Esc 关闭。

use std::collections::HashMap;
use std::rc::Rc;

use nsh_base::{PopoverEvent, button, popover};
use nsh_core::prelude::*;

use super::super::components::button::{RADIUS_MD, icon_button};
use super::super::components::cover::cover;
use super::super::components::icons::{Icon, icon, icon_dyn};
use super::super::store::{cover_of, format_time, player, store};
use super::super::theme::*;
use crate::engine::audio::LoopMode;

const ROW: f32 = 52.0;

/// 队列按钮与它打开的弹层。`open` 由应用持有（按钮的高亮也读它）。
pub fn queue_button(open: Signal<bool>) -> AnyElement {
    // 触发器由 popover 接管按压（切换开合、`expanded` 状态），所以是普通
    // view：nsh-base 的 button 自己吃掉按压，popover 收不到。外形同 shadcn
    // ghost `icon-sm`。
    let trigger = view()
        .size(px(32.0))
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_MD))
        .role(AccessibilityRole::Button)
        .label("播放队列")
        .style_when(move || open.get(), |st| st.bg(hover()))
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.8))
        .transition_colors()
        .duration(150.0)
        .child(icon_dyn(
            || Icon::Queue,
            16.0,
            move || if open.get() { primary() } else { muted_fg() },
        ));
    popover()
        .open(move || Some(open.get()))
        .on_open_change(move |e: &PopoverEvent| {
            if let PopoverEvent::OpenChange(next, _) = e {
                open.set(*next);
            }
        })
        .placement(Placement::Top)
        .align(Align::End)
        .offset(px(12.0))
        .margin(px(8.0))
        .popup_style(|st| {
            st.w(px(340.0))
                .rounded(px(RADIUS_MD))
                .border_1()
                .border_color(border())
                .bg(nsh_base::theme().popover)
                .shadow_md()
                .overflow_hidden()
        })
        .trigger(trigger)
        .child(queue_panel(open))
        .into_element()
}

fn queue_panel(open: Signal<bool>) -> View {
    let s = store();
    // 路径 → 曲库里的条目（队列只存路径）。
    let index = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(
            tracks
                .iter()
                .enumerate()
                .map(|(i, t)| (t.path.clone(), i))
                .collect::<HashMap<_, _>>(),
        )
    });
    let count = computed(move || s.queue.with(|q| q.len()));
    let current = computed(move || s.track.with(|t| t.index));
    let start = untracked(move || current.get()).max(0) as usize;

    let list = virtual_list(count, move |at| {
        let path = computed(move || s.queue.with(|q| q.get(at.index()).cloned()));
        let entry = computed(move || {
            let p = path.get()?;
            let i = index.with(|m| m.get(&p).copied())?;
            s.tracks.with(|t| t.get(i).cloned())
        });
        let is_current = move || current.get() == at.index() as i64;
        let playing = move || is_current() && s.playback.with(|p| p.playing);
        let title = move || {
            entry.with(|e| match e {
                Some(e) => e.title.clone(),
                None => path.with(|p| {
                    p.as_deref()
                        .map(|p| super::super::views::last_segment(p).to_owned())
                        .unwrap_or_default()
                }),
            })
        };
        let src = move || path.with(|p| p.as_deref().and_then(cover_of));
        button()
            .h(px(ROW))
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .px(px(10.0))
            .rounded(px(RADIUS_MD))
            .accessibility_label(move || SharedString::from(title()))
            .selected(is_current)
            .selected_style(|st| st.bg(hover()))
            .hover(|st| st.bg(hover()))
            .active(|st| st.opacity(0.7))
            .on_press(move |_: &()| player().jump(untracked(|| at.index())))
            .child(
                view().w(px(24.0)).items_center().child(
                    when(is_current, move || {
                        icon_dyn(
                            move || if playing() { Icon::Wave } else { Icon::Play },
                            13.0,
                            primary,
                        )
                    })
                    .otherwise(move || {
                        text(move || (at.index() + 1).to_string())
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .tabular_nums()
                            .text_color(faint())
                    }),
                ),
            )
            .child(cover(src, Some(36.0), 6.0))
            .child(
                view()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(3.0))
                    .child(
                        text(title)
                            .number_of_lines(1)
                            .text_size(px(13.0))
                            .line_height(px(16.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(move || if is_current() { primary() } else { fg() })
                            .transition_colors()
                            .duration(300.0),
                    )
                    .child(
                        text(move || {
                            entry.with(|e| e.as_ref().map(|e| e.artist.clone()).unwrap_or_default())
                        })
                        .number_of_lines(1)
                        .text_size(px(12.0))
                        .line_height(px(15.0))
                        .text_color(muted_fg()),
                    ),
            )
            .child(
                text(move || {
                    entry.with(|e| {
                        e.as_ref()
                            .map(|e| format_time(e.duration))
                            .unwrap_or_default()
                    })
                })
                .text_size(px(12.0))
                .line_height(px(12.0))
                .tabular_nums()
                .text_color(muted_fg()),
            )
    })
    .estimated_item_size(px(ROW))
    .initial_scroll_index(start)
    .content_padding_x(px(8.0))
    .content_padding_bottom(px(8.0))
    .empty(
        view()
            .h(px(160.0))
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .child(icon(Icon::Queue, 26.0, faint))
            .child(
                text("队列是空的")
                    .text_size(px(13.0))
                    .line_height(px(13.0))
                    .text_color(muted_fg()),
            )
            .child(
                text("从曲库播放一首，它所在的列表就成为队列")
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(faint()),
            ),
    )
    .flex_1();

    // 高度：最多 560，窗口矮时让出标题栏与播放条。
    view()
        .w_full()
        .h(move || px((window().height.0 - top_inset() - BAR_SPACE - 16.0).clamp(200.0, 560.0)))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .px(px(16.0))
                .pt(px(12.0))
                .pb(px(8.0))
                .child(
                    view()
                        .flex_1()
                        .gap(px(2.0))
                        .child(
                            text("播放队列")
                                .text_size(px(14.0))
                                .line_height(px(14.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(fg()),
                        )
                        .child(
                            text(move || {
                                let n = count.get();
                                let at = current.get();
                                if n == 0 {
                                    "没有曲目".to_owned()
                                } else {
                                    format!("第 {} / {} 首", at.max(0) + 1, n)
                                }
                            })
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .tabular_nums()
                            .text_color(muted_fg()),
                        ),
                )
                .child(icon_button(
                    move || match s.playback.with(|p| p.loop_mode) {
                        LoopMode::List => Icon::Repeat,
                        LoopMode::Single => Icon::RepeatOne,
                        LoopMode::Random => Icon::Shuffle,
                    },
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
                ))
                .child(icon_button(
                    || Icon::Close,
                    13.0,
                    "关闭播放队列",
                    muted_fg,
                    move || open.set(false),
                )),
        )
        .child(view().h(px(1.0)).bg(border()))
        .child(list)
}
