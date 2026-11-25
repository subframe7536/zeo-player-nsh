//! 文本输入的 shadcn/ui 外观：单行 [`text_input`]（Input）与多行
//! [`text_area`]（Textarea）。行为来自 nsh-base 的 `Input`，尺寸、间距、
//! 圆角同 shadcn，颜色取自主题（浅色 / 深色）。

use nsh_base::{Input, input};
use nsh_core::prelude::*;

use super::button::RADIUS_MD;
use zeo_ui_store::theme::*;

/// 两个组件共用的盒体：`rounded-md border border-input text-sm
/// shadow-xs`，聚焦时 `border-ring ring-3 ring-ring/50`。底色用 `card`
/// 而不是 shadcn 的 `bg-transparent` / `dark:bg-input/30`：面板底色会随
/// 封面染色，透明或半透明的底会把染色带进输入框。
fn chrome() -> Input {
    let t = nsh_base::theme();
    let (input_c, ring) = (t.input, t.ring);
    input()
        .px(px(8.0))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(input_c)
        .shadow_xs()
        .text_size(px(14.0))
        .text_color(fg())
        .bg(t.card)
        .focus(move |st| {
            st.border_color(ring)
                .ring(px(3.0))
                .ring_color(ring.opacity(0.5))
        })
}

/// shadcn Input：`h-9`（`small` 为 `h-8`，放在一行条件里）。
/// 行内 padding、占位、提交与值由调用方按用途加。
pub fn text_input(small: bool) -> Input {
    let field = chrome().h(px(if small { 32.0 } else { 36.0 }));
    // macOS 的单行输入把行盒垂直居在内容盒里，而字形墨水在行盒内偏下：
    // 加 2pt 底边距把内容盒中心抬高约 1pt 抵消（见 upstream.md「单行
    // 输入文字偏下」）。Windows 的 RichEdit 落位已正，同样抬高反而
    // 显得文字偏上，不加。
    #[cfg(target_os = "macos")]
    let field = field.pb(px(2.0));
    field
}

/// 多行输入的高度策略（[`text_area`]）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextAreaHeight {
    /// 固定高度，超出部分内部原生滚动。
    Fixed(f32),
    /// 随文生长：`min` 起，文本超过 `max` 时锁成固定高。
    Grow { min: f32, max: f32 },
}

/// shadcn Textarea：与 [`text_input`] 同款边框 / 聚焦环，`py-2.5`
/// 多行内边距。`Grow` 时输入框 `h(auto)` 的 intrinsic 高度随内容长，
/// `max_h` 只裁元素框、宿主编辑器仍按内容高开视口——内部永远没有可滚
/// 范围；锁成固定 `max` 后编辑器视口被框住，原生滚动恢复，未超限时仍
/// auto 随文生长。22 = py(10)×2 + border_1×2。
pub fn text_area(height: TextAreaHeight) -> Input {
    let field = chrome().multiline(true).py(px(10.0));
    match height {
        TextAreaHeight::Fixed(h) => field.h(px(h)),
        TextAreaHeight::Grow { min, max } => {
            let content_h = signal(0.0f32);
            field
                .on_content_size_change(move |s: &nsh_core::Size<nsh_core::Points>| {
                    content_h.set(s.height.0)
                })
                .h(move || {
                    if content_h.get() + 22.0 > max {
                        Length::from(px(max))
                    } else {
                        auto()
                    }
                })
                .min_h(px(min))
                .max_h(px(max))
        }
    }
}
