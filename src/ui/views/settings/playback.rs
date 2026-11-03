//! 设置 →「播放」分类的各节。

use nsh_core::prelude::*;

use super::super::super::components::form::input_field;
use super::super::super::store::store;
use super::super::super::theme::*;
use super::setting_section;

/// 播放：淡入淡出时长（毫秒，数字输入；0 为关闭，上限 5000）。
pub(super) fn playback_section() -> View {
    let s = store();
    let draft = signal(s.library.with(|l| l.config.fade_ms).to_string());
    // 外部改了 fade_ms（另一个实例、测试）时同步草稿。
    effect(move || {
        let ms = s.library.with(|l| l.config.fade_ms);
        draft.set(ms.to_string());
    });
    // 提交（回车 / 失焦）：解析失败回退为当前值，成功则钳到 5000 并规范化草稿。
    let commit = move || match draft.get().trim().parse::<u32>() {
        Ok(ms) => {
            let ms = ms.min(5000);
            super::super::super::store::set_fade_ms(ms);
            draft.set(ms.to_string());
        }
        Err(_) => draft.set(s.library.with(|l| l.config.fade_ms).to_string()),
    };
    let field = input_field(true)
        .w(px(88.0))
        .label("淡入淡出时长")
        .keyboard_type(KeyboardType::Number)
        .value(move || draft.get())
        .on_change(move |t: &str| draft.set(t.to_owned()))
        .on_submit(move |_: &str| commit())
        .on_blur(move || commit());
    setting_section(
        "淡入淡出",
        "播放、暂停、拖动进度和切歌时，音量在这段时间里渐变，不会突然响起或戛然而止。一首自然播完接下一首时不淡变。",
        None,
    )
    .child(
        view()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(16.0))
            .child(
                view()
                    .flex_1()
                    .min_w(px(200.0))
                    .gap(px(2.0))
                    .child(
                        text("淡入淡出时长")
                            .text_size(px(14.0))
                            .line_height(px(14.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg()),
                    )
                    .child(
                        text("0 为关闭，最大 5000 毫秒。")
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .text_color(muted_fg()),
                    ),
            )
            .child(field)
            .child(
                text("毫秒")
                    .text_size(px(13.0))
                    .line_height(px(13.0))
                    .text_color(muted_fg()),
            ),
    )
}
