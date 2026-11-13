//! 第 4 步：功能一览——图标 + 标题 + 一句话说明的卡片网格。

use nsh_core::prelude::*;

use crate::step_header;
use zeo_ui_components::icons::{Icon, icon};
use zeo_ui_store::KeyAction;
use zeo_ui_store::theme::*;

/// 卡片的最小宽度：内容列 640 宽时一行三张，窄窗口自动折成两张 / 一张。
const CARD_MIN_W: f32 = 180.0;

pub(crate) fn overview() -> View {
    view()
        .gap(px(24.0))
        .child(step_header(
            "功能一览",
            "再看几个常用功能，之后都能在界面和设置里找到。",
        ))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap(px(12.0))
                .child(card(Icon::Search, "命令面板", palette_desc))
                .child(card(Icon::Expand, "全屏播放与歌词", || {
                    "封面色背景、波形进度条与滚动歌词，歌词偏移可按单曲微调。".to_owned()
                }))
                .child(card(Icon::Edit, "元数据表与批量改名", || {
                    "查看并批量修改 24 个标签字段，按模板重命名文件。".to_owned()
                }))
                .child(card(Icon::Smart, "智能歌单", || {
                    "按标题、艺术家、专辑、标签等条件自动汇集曲目。".to_owned()
                }))
                .child(card(Icon::Wave, "十段均衡器与 ReplayGain", || {
                    "均衡器带防削波，ReplayGain 让各曲音量一致。".to_owned()
                }))
                .child(card(Icon::Keyboard, "托盘与全局快捷键", || {
                    "关闭窗口可最小化到托盘继续播放；快捷键可在设置中重新绑定。".to_owned()
                })),
        )
}

/// 命令面板的说明：键位读生效的键位表（平台修饰键与换绑都跟着变）；
/// 被解绑时不提键位。
fn palette_desc() -> String {
    let key = zeo_ui_store::key_label_of(KeyAction::Palette);
    if key.is_empty() {
        "搜索曲目、跳转页面与设置（可在 设置 › 快捷键 里绑定）。".to_owned()
    } else {
        format!("按 {key} 打开，搜索曲目、跳转页面与设置。")
    }
}

/// 一张功能卡片：图标块在上，标题与说明在下。
fn card(glyph: Icon, title: &'static str, desc: impl Fn() -> String + 'static) -> View {
    view()
        .flex_1()
        .min_w(px(CARD_MIN_W))
        .gap(px(12.0))
        .p(px(16.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(border())
        .child(
            view()
                .size(px(36.0))
                .rounded(px(9.0))
                .bg(primary().opacity(0.08))
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .child(icon(glyph, 18.0, primary)),
        )
        .child(
            view()
                .gap(px(6.0))
                .child(
                    text(title)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg()),
                )
                .child(text(desc).text_size(px(12.0)).text_color(muted_fg())),
        )
}
