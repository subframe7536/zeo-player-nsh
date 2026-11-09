//! 全屏播放（参照设计稿）：模糊封面铺底、整体压暗；左半区居中一列——
//! 封面、曲名与「更多」、歌手 / 专辑、标签、细波形进度与音频信息、线框
//! 控制按钮、音量；右半区居中的歌词，当前行亮、其余按距离变暗变模糊。
//! 没有歌词时不显示歌词区，左列在整页居中。
//!
//! `mod.rs` 是页面装配与切歌过渡，`column` 左列，`lyrics` 歌词列。

mod column;
mod lyrics;

use std::cell::RefCell;
use std::rc::Rc;

use nsh_core::prelude::*;

use self::column::left_column;
use self::lyrics::{LYRICS_BOTTOM, lyrics_column, lyrics_shown};
use zeo_ui_components::button::{on_dark, tipped};
use zeo_ui_components::icons::{Icon, icon};
use zeo_ui_store::theme::*;
use zeo_ui_store::{player, set_volume, store};

/// 每次打开全屏播放加一：已有的歌词据此回到高亮行（见 `lyrics` 的滚屏）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reveal(pub u32);

/// 打开：淡入并从下方滑入，先快后慢。
pub const IN_MS: f32 = 380.0;
pub const IN_EASE: Easing = Easing::Bezier(0.22, 1.0, 0.36, 1.0);
/// 收起：淡出并向下滑出，先慢后快，比打开短。
pub const OUT_MS: f32 = 240.0;
pub const OUT_EASE: Easing = Easing::Bezier(0.4, 0.0, 1.0, 1.0);
/// 开合时滑动的距离（点）。
const SHEET_SLIDE: f32 = 56.0;

/// 全屏播放页。`fade` 为 0→1 的开合进度（同时驱动透明度与上下滑动）；
/// `on_close` 收起。
pub fn full_player(fade: AnimatedValue, on_close: Rc<dyn Fn()>) -> View {
    let s = store();

    let close = on_close.clone();
    on_key("escape", move |_| close());
    on_key("left", |_| player().seek_by(-5.0));
    on_key("right", |_| player().seek_by(5.0));
    on_key("up", move |_| {
        set_volume(s.playback.with_untracked(|p| p.volume) + 0.05)
    });
    on_key("down", move |_| {
        set_volume(s.playback.with_untracked(|p| p.volume) - 0.05)
    });

    let swap = Swap::install();
    let lyrics_shown = lyrics_shown();

    // 底：深灰，封面放大模糊，再压一层暗色，保证白字对比。背景图复用
    // 左列大封面的 data-URI（`t.artwork`，已在内存里——以前挂列表封面
    // 的 96px 文件路径，升档 400px 后小文件被 prune，路径会悬空）。上下
    // 两层：`under` 是上一张，`top` 是当前这张（见 `Swap::backdrop`）。
    //
    // 模糊分平台挂两处：macOS 后端不画容器 view 的 `filter: blur()`，
    // 只在 image 元素上烘 `blur_radius`；Windows 反过来——`blur_radius`
    // 的位图效果路径画不出铺满（见 AGENTS.md 待上游），而容器 view 的
    // 滤镜会走离屏合成（decor::draw_layered），能画。
    let layer = |path: Signal<Option<SharedString>>, alpha: &AnimatedValue| {
        view()
            .absolute()
            .inset(px(-40.0))
            .bind_opacity(alpha)
            // Windows 的模糊挂在层上（macOS 忽略容器滤镜，无副作用）。
            .style_with(|st| st.blur(px(32.0)))
            .child(when(
                move || path.with(Option::is_some),
                move || {
                    let art = image(move || path.get().unwrap_or_default())
                        .absolute()
                        // 多出窗口一圈：模糊在图层边缘会淡出成透明，外扩后
                        // 淡出发生在屏幕外。
                        .inset(px(-40.0))
                        .object_fit(ResizeMode::Cover)
                        .opacity(0.9);
                    // macOS 的模糊烘在 image 位图上。
                    #[cfg(not(target_os = "windows"))]
                    let art = art.blur_radius(px(32.0));
                    art
                },
            ))
    };
    let backdrop = view()
        .absolute()
        .inset(px(0.0))
        .bg(AdaptiveColor::rgb(0x2a2a2c, 0x2a2a2c))
        .child(layer(swap.under, &swap.under_alpha))
        .child(layer(swap.top, &swap.backdrop))
        .child(view().absolute().inset(px(0.0)).bg(black_a(0.5)));

    // 收起按钮在标题栏这一行里垂直居中、靠右：Windows 上紧挨窗口按钮
    // 左边，macOS 上（右侧没有系统按钮）就在窗口右边缘。
    let close_button = view()
        .absolute()
        .top(move || px((top_inset() - 32.0) / 2.0))
        .right(move || px(controls_right() + 8.0))
        .window_drag(false)
        .child(tipped(
            on_dark(32.0)
                .accessibility_label("收起")
                .on_press(move |_: &()| on_close())
                .child(icon(Icon::ChevronDown, 18.0, || white_a(0.7))),
            "收起",
        ));

    view()
        .absolute()
        .inset(px(0.0))
        .z_index(50)
        .overflow_hidden()
        .bind_opacity(&fade)
        .bind_translate_y(&fade.interpolate(&[0.0, 1.0], &[SHEET_SLIDE, 0.0]))
        .child(backdrop)
        .child(
            view()
                .flex_1()
                .flex_row()
                .child(
                    view()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .px(px(32.0))
                        .child(left_column(&swap)),
                )
                // 没有歌词时不留歌词区，左列（封面、元数据、控制）在整页居中。
                .child(when(
                    move || lyrics_shown.get(),
                    move || {
                        view()
                            .flex_1()
                            .items_center()
                            .pl(px(16.0))
                            .pr(px(0.0))
                            .pt(move || px(top_inset()))
                            .pb(px(LYRICS_BOTTOM))
                            .child(lyrics_column(&swap))
                    },
                )),
        )
        .child(
            view()
                .absolute()
                .top(px(0.0))
                .left(px(0.0))
                .right(px(0.0))
                .h(move || px(top_inset()))
                .window_drag(true),
        )
        .child(close_button)
}

// --- 切歌过渡 -----------------------------------------------------------------

/// 切歌时的过渡：内容淡入并从切歌方向滑入（下一首从右、上一首从左），
/// 背景的模糊封面慢一些交叉淡入。系统开了「减弱动态效果」时不动画。
///
/// 背景不随换歌立即清掉：引擎在缓冲填起来之后才生成新封面，这段时间
/// `artwork` 为 `None`，直接跟着走会露出底色，整页一瞬间变黑。所以等
/// `covers_ready` 了再换：新图放在上层（`top`）淡入，旧图留在下层
/// （`under`）垫底；新歌没有封面时旧图淡出。
#[derive(Clone, Copy)]
struct Swap {
    content: AnimatedValue,
    shift: AnimatedValue,
    lyrics_shift: AnimatedValue,
    /// 上层背景的不透明度。
    backdrop: AnimatedValue,
    /// 下层（上一张）背景的不透明度。
    under_alpha: AnimatedValue,
    top: Signal<Option<SharedString>>,
    under: Signal<Option<SharedString>>,
}

/// 滑入距离（点）。
const SLIDE: f32 = 36.0;

/// 切歌方向：队列位置变小为上一首（-1），否则为下一首（1）。
fn direction(before: i64, now: i64) -> f32 {
    if now < before { -1.0 } else { 1.0 }
}

impl Swap {
    fn install() -> Swap {
        let s = store();
        let swap = Swap {
            content: animated_value(1.0),
            shift: animated_value(0.0),
            lyrics_shift: animated_value(0.0),
            backdrop: animated_value(1.0),
            under_alpha: animated_value(1.0),
            top: signal(untracked(move || s.track.with(|t| t.artwork.clone()))),
            under: signal(None),
        };
        let (content, shift, lyrics_shift) = (swap.content, swap.shift, swap.lyrics_shift);
        let (backdrop, under_alpha, top, under) =
            (swap.backdrop, swap.under_alpha, swap.top, swap.under);
        effect(move || {
            let (ready, next) = s
                .track
                .with(|t| (t.covers_ready || t.error.is_some(), t.artwork.clone()));
            let current = top.get_untracked();
            if !ready || current == next {
                return;
            }
            let calm = untracked(reduce_motion);
            under.set(current);
            top.set(next.clone());
            if next.is_some() {
                under_alpha.set(1.0);
                backdrop.set(if calm { 1.0 } else { 0.0 });
                if !calm {
                    let _ = backdrop.timing(1.0, 700.0, Easing::EaseOut);
                }
            } else if calm {
                under_alpha.set(0.0);
            } else {
                let _ = under_alpha.timing(0.0, 700.0, Easing::EaseOut);
            }
        });
        let key = computed(move || s.track.with(|t| (t.path.clone(), t.index)));
        let previous = RefCell::new(None::<(Option<String>, i64)>);
        effect(move || {
            let now = key.get();
            let before = previous.replace(Some(now.clone()));
            // 打开全屏时不动画；只在曲目真的换了时动画。
            let Some(before) = before else { return };
            if before.0 == now.0 || now.0.is_none() || untracked(reduce_motion) {
                return;
            }
            let dir = direction(before.1, now.1);
            let ease = Easing::Bezier(0.22, 1.0, 0.36, 1.0);
            content.set(0.0);
            shift.set(dir * SLIDE);
            lyrics_shift.set(dir * SLIDE * 0.6);
            let _ = content.timing(1.0, 320.0, Easing::EaseOut);
            let _ = shift.timing(0.0, 420.0, ease);
            let _ = lyrics_shift.timing(0.0, 480.0, ease);
        });
        swap
    }
}

// --- 文件位置 -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_direction_follows_the_queue() {
        assert_eq!(direction(3, 4), 1.0, "下一首从右滑入");
        assert_eq!(direction(4, 3), -1.0, "上一首从左滑入");
        assert_eq!(direction(2, 2), 1.0, "队列不变（重新打开）当作下一首");
    }
}
