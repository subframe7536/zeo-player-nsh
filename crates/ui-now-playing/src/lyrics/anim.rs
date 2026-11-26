use super::*;

/// 点击某行后，最多等这么久让高亮换到这一行（之后照常跟随）。
pub(crate) const PENDING_WAIT: Duration = Duration::from_millis(1500);

pub(crate) fn text_align(
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
pub(crate) const LYRIC_SIZE: f32 = 30.0;
/// 译文字号。
pub(crate) const TRANSLATION_SIZE: f32 = LYRIC_SIZE * 2.0 / 3.0;
/// 未高亮原文的颜色（白色透明度）。
pub(crate) const IDLE_ALPHA: f32 = 0.3;
/// 高亮原文的颜色。
pub(crate) const ACTIVE_ALPHA: f32 = 0.92;
/// 译文的颜色，任何时候都不变，位于高亮与未高亮原文之间。
pub(crate) const TRANSLATION_ALPHA: f32 = 0.5;
/// 鼠标移入 / 移出歌词区时，模糊去掉 / 恢复的过渡时长。
pub(crate) const BLUR_FADE_MS: f32 = 300.0;
/// 高亮过渡的时长：与程序滚动一致（NSH 0.3.0 滚动动画 `ANIMATION` 为
/// 300 ms）。
pub(crate) const SCROLL_MS: f32 = 300.0;
/// 歌词区上下的留白：滚动的歌词不进入标题栏（窗口按钮、收起按钮那一行），
/// 也不贴着窗口底边。
pub(crate) const LYRICS_BOTTOM: f32 = 32.0;

pub(crate) fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// 逐帧把 `value` 推到 `to`（`ms` 毫秒，ease-out）：顶替进行中的上一次，
/// 「减弱动态效果」时直接到位。
pub(crate) fn drive(value: Signal<f32>, task: &Rc<RefCell<Option<Spawned<()>>>>, to: f32, ms: f32) {
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
pub(crate) fn original_opacity(timed: bool, active: bool) -> f32 {
    if active || !timed {
        ACTIVE_ALPHA
    } else {
        IDLE_ALPHA
    }
}
