//! 首次启动的欢迎页：盖住整个窗口（侧栏、内容、播放条）的分步引导页——
//! 欢迎 / 添加音乐文件夹 / 主题色 / 功能一览。是否显示由 `Store::welcome`
//! 决定（`install` 按配置算好），结束——走完、右上角「跳过」、Esc——一律走
//! `zeo_ui_store::finish_welcome` 记下已看过。
//!
//! 外壳照全屏播放页的做法：普通的绝对定位整窗视图（不是 `dialog` 的原生
//! 层——那样窗口拖动区与 Windows 的窗口按钮都不好摆），应用外壳留在下面
//! 不拆，重开不丢状态；顶部一条 `top_inset()` 高的拖动区，右上角的「跳过」
//! 让开 `controls_right()`（Windows 的窗口按钮由 NSH 画，盖过本页）。
//! 步骤号是本页的本地信号，每次打开（`when` 重新挂载）从头开始。

mod features;
mod steps;

use nsh_core::prelude::*;

use zeo_ui_components::button::{Size, Variant, text_button};
use zeo_ui_store::theme::*;
use zeo_ui_store::{finish_welcome, store};

/// 步数：欢迎、添加音乐文件夹、主题色、功能一览。
const STEPS: usize = 4;
/// 内容列的最大宽度。
const COLUMN_W: f32 = 640.0;
/// 底部导航条的高度。
const FOOTER_H: f32 = 88.0;
/// 导航条两侧占位的宽度（上一步 / 下一步按钮各占一侧，指示点居中）。
const SLOT_W: f32 = 150.0;
/// 淡入 / 淡出时长（毫秒）。
const IN_MS: f32 = 240.0;
const OUT_MS: f32 = 180.0;

/// 欢迎页（Esc / 右上角「跳过」都算跳过）。
pub fn page() -> AnyElement {
    let s = store();
    let step = signal(0_usize);
    // 文件夹一步的状态放在页面层：来回切步骤不丢结果说明，选择器的
    // 防重入标记也不会随步骤视图销毁。
    let message = signal(String::new());
    let picking = signal(false);
    let has_roots = computed(move || s.library.with(|l| !l.config.roots.is_empty()));

    // 淡入；离开时先淡出再 `finish_welcome`（它让根组件的 `when` 卸载本页）。
    let fade = animated_value(if untracked(reduce_motion) { 1.0 } else { 0.0 });
    effect(move || {
        if !untracked(reduce_motion) {
            let _ = fade.timing(1.0, IN_MS, Easing::EaseOut);
        }
    });
    let leaving = signal(false);
    let leave = move || {
        if leaving.get_untracked() {
            return;
        }
        leaving.set(true);
        if untracked(reduce_motion) {
            finish_welcome();
            return;
        }
        let end = fade.timing(0.0, OUT_MS, Easing::EaseIn);
        spawn(async move {
            let _ = end.await;
            finish_welcome();
        });
    };
    // 本页是最新绑定，盖过全屏播放页的 Esc；根组件在欢迎页打开期间停用
    // 其余窗口内快捷键（见 `keys()`）。
    on_key("escape", move |_| leave());

    // 步骤体：只在步骤号变时重建；构建本身包进 `untracked`，里面顺手读的
    // 信号（如主题色草稿）不会让整页重建。
    let body = dynamic(move || {
        let n = step.get();
        untracked(move || {
            match n {
                0 => steps::welcome(),
                1 => steps::folders(message, picking),
                2 => steps::accent(),
                _ => features::overview(),
            }
            .into_element()
        })
    });

    // 内容列：窗口够高时垂直居中，不够高时整列滚动。
    let main = scroll().flex_1().child(
        view()
            .min_h(move || px((window().height.0 - top_inset() - FOOTER_H).max(0.0)))
            .items_center()
            .justify_center()
            .px(px(PAGE_X))
            .py(px(24.0))
            .child(view().w_full().max_w(px(COLUMN_W)).child(body)),
    );

    let footer = dynamic(move || {
        let n = step.get();
        let label = match n {
            0 => "开始",
            // 还没添加文件夹时，这一步可以放心跳过。
            1 if !has_roots.get() => "稍后添加",
            1 | 2 => "下一步",
            _ => "开始使用",
        };
        let next = text_button(Variant::Default, Size::Default, None, label, move || {
            if n + 1 >= STEPS {
                leave();
            } else {
                step.set(n + 1);
            }
        });
        let back = (n > 0).then(|| {
            text_button(
                Variant::Outline,
                Size::Default,
                None,
                "上一步",
                move || step.set(n.saturating_sub(1)),
            )
        });
        view()
            .h(px(FOOTER_H))
            .flex_shrink_0()
            .items_center()
            .px(px(PAGE_X))
            .child(
                view()
                    .w_full()
                    .max_w(px(COLUMN_W))
                    .h_full()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(view().w(px(SLOT_W)).flex_row().child(back))
                    .child(dots(step))
                    .child(view().w(px(SLOT_W)).flex_row().justify_end().child(next)),
            )
            .into_element()
    });

    // 顶部一条拖动区（与标题栏同高，macOS 交通灯在它里面）；「跳过」在这一行
    // 里垂直居中、靠右，Windows 上让开窗口按钮。
    let drag = view()
        .absolute()
        .top(px(0.0))
        .left(px(0.0))
        .right(px(0.0))
        .h(move || px(top_inset()))
        .window_drag(true);
    let skip = view()
        .absolute()
        .top(move || px((top_inset() - 32.0) / 2.0))
        .right(move || px(controls_right() + 12.0))
        .window_drag(false)
        .child(text_button(Variant::Ghost, Size::Sm, None, "跳过", leave));

    view()
        .test_id("welcome")
        .absolute()
        .inset(px(0.0))
        .z_index(60)
        .bg(move || bg())
        .bind_opacity(&fade)
        .child(view().h(move || px(top_inset())).flex_shrink_0())
        .child(main)
        .child(footer)
        .child(drag)
        .child(skip)
        .into_element()
}

/// 每步的页头：大标题（`PAGE_TITLE`）+ 一句说明。
pub(crate) fn step_header(title: &'static str, desc: &'static str) -> View {
    view()
        .gap(px(10.0))
        .child(
            text(title)
                .text_size(px(PAGE_TITLE))
                .line_height(px(PAGE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg()),
        )
        .child(text(desc).text_size(px(15.0)).text_color(muted_fg()))
}

/// 步骤指示点：当前步是主题色的长胶囊，其余是描边色的小圆点。
fn dots(step: Signal<usize>) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .children((0..STEPS).map(move |i| {
            view()
                .h(px(8.0))
                .w(move || px(if step.get() == i { 24.0 } else { 8.0 }))
                .rounded_full()
                .bg(move || if step.get() == i { primary() } else { border() })
                .transition_colors()
                .duration(300.0)
        }))
}
