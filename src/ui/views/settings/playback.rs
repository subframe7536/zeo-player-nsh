//! 设置 →「播放」分类的各节。

use nsh_core::prelude::*;

use super::super::super::components::form::input_field;
use super::super::super::store::store;
use super::super::super::theme::*;
use super::{pref_row, setting_section};

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

/// 输出设备：拔下耳机或切换系统默认输出设备时的行为。
pub(super) fn device_section() -> View {
    let s = store();
    setting_section(
        "输出设备",
        "拔下耳机或切换系统默认输出设备时的行为。",
        None,
    )
    .child(pref_row(
        "设备变化时暂停",
        "关闭后会在新的默认设备上继续播放。设备全部断开时总会暂停。",
        move || s.library.with(|l| l.config.pause_on_device_change),
        super::super::super::store::set_pause_on_device_change,
    ))
}

/// dB 输入行：标题 + 说明 | 草稿制数字输入 + `dB`（−15…+15，非法回退）。
fn db_row(
    title: &'static str,
    desc: &'static str,
    label: &'static str,
    get: impl Fn() -> f32 + Copy + 'static,
    set: impl Fn(f32) + Copy + 'static,
) -> View {
    let draft = signal(format!("{}", get()));
    // 外部改了配置（另一个实例、测试）时同步草稿。
    effect(move || draft.set(format!("{}", get())));
    let commit = move || match draft.get().trim().parse::<f32>() {
        Ok(db) => {
            let db = db.clamp(-15.0, 15.0);
            set(db);
            draft.set(format!("{db}"));
        }
        Err(_) => draft.set(format!("{}", get())),
    };
    let field = input_field(true)
        .w(px(88.0))
        .label(label)
        .keyboard_type(KeyboardType::Number)
        .value(move || draft.get())
        .on_change(move |t: &str| draft.set(t.to_owned()))
        .on_submit(move |_: &str| commit())
        .on_blur(move || commit());
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
                    text(title)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg()),
                )
                .child(
                    text(desc)
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(field)
        .child(
            text("dB")
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
}

/// 改 ReplayGain 配置：读当前值、应用修改、立即生效并保存。
fn edit_replaygain(f: impl Fn(&mut crate::engine::audio::ReplayGainConfig)) {
    let mut next = store().library.with(|l| l.config.replaygain);
    f(&mut next);
    super::super::super::store::set_replaygain(next);
}

/// ReplayGain：按标签把不同曲目 / 专辑拉到统一响度（默认关闭）。
pub(super) fn replaygain_section() -> View {
    use crate::engine::audio::ReplayGainMode;
    let s = store();
    let cfg = move || s.library.with(|l| l.config.replaygain);
    const MODES: &[(ReplayGainMode, &str)] = &[
        (ReplayGainMode::Off, "关闭"),
        (ReplayGainMode::Track, "按曲目"),
        (ReplayGainMode::Album, "按专辑"),
    ];
    let on = move || cfg().mode != ReplayGainMode::Off;
    setting_section(
        "ReplayGain",
        "按曲目或专辑里的 ReplayGain 标签调整音量，让不同专辑听起来一样响。只读取已有标签，不会扫描计算。",
        None,
    )
    .child(
        super::super::super::components::radio::group_row().children(MODES.iter().map(
            |&(mode, label)| {
                super::super::super::components::radio::item(
                    move || cfg().mode == mode,
                    label,
                    move || edit_replaygain(|c| c.mode = mode),
                )
            },
        )),
    )
    .child(when(on, move || {
        view()
            .gap(px(16.0))
            .child(db_row(
                "预放大",
                "在标签增益上叠加的固定增益。",
                "ReplayGain 预放大",
                move || s.library.with(|l| l.config.replaygain.preamp_db),
                move |db| edit_replaygain(|c| c.preamp_db = db),
            ))
            .child(db_row(
                "无标签曲目",
                "没有 ReplayGain 标签的曲目使用这个增益。",
                "ReplayGain 无标签增益",
                move || s.library.with(|l| l.config.replaygain.fallback_db),
                move |db| edit_replaygain(|c| c.fallback_db = db),
            ))
            .child(pref_row(
                "防止削波",
                "按标签里的峰值限制增益，避免爆音。",
                move || s.library.with(|l| l.config.replaygain.clip_protect),
                move |on| edit_replaygain(|c| c.clip_protect = on),
            ))
    }))
}
