//! 设置 →「歌词」分类的各节。

use nsh_core::prelude::*;

use super::{pref_row, setting_section};
use zeo_ui_components::form::input_field;
use zeo_ui_store::store;
use zeo_ui_store::theme::*;

/// 全屏歌词的对齐方式。
pub(crate) fn lyrics_section() -> View {
    use zeo_types::config::LyricsAlign;
    let s = store();
    const OPTIONS: &[(LyricsAlign, &str)] = &[
        (LyricsAlign::Left, "左对齐"),
        (LyricsAlign::Center, "居中"),
        (LyricsAlign::Right, "右对齐"),
    ];
    setting_section("歌词对齐", "全屏播放页里歌词的对齐方式。", None).child(
        zeo_ui_components::radio::group_row().children(OPTIONS.iter().map(|&(align, label)| {
            zeo_ui_components::radio::item(
                move || s.library.with(|l| l.config.lyrics_align == align),
                label,
                move || zeo_ui_store::set_lyrics_align(align),
            )
        })),
    )
}
/// 歌词来源：本地同名 .lrc / 内嵌歌词的优先序。
pub(crate) fn lyrics_source_section() -> View {
    let s = store();
    setting_section(
        "歌词来源",
        "歌词可以来自曲目同目录的同名 .lrc 文件，或音频文件的内嵌歌词。",
        None,
    )
    .child(pref_row(
        "本地歌词优先",
        "使用同目录的同名 .lrc 文件作为歌词；关闭则优先内嵌歌词。",
        move || s.library.with(|l| l.config.prefer_local_lyrics),
        zeo_ui_store::set_prefer_local_lyrics,
    ))
}

/// 歌词行时间戳：全屏页歌词行悬停时在行尾显示该行时间（占位，不被长歌词盖住）。
pub(crate) fn lyrics_timestamps_section() -> View {
    let s = store();
    setting_section(
        "歌词行时间戳",
        "全屏播放页的歌词行悬停时，行尾显示该行的时间；点击行跳到那里。",
        None,
    )
    .child(pref_row(
        "显示行时间戳",
        "悬停歌词行时显示该行时间戳；关闭后不再显示。",
        move || s.library.with(|l| l.config.lyrics_timestamps),
        zeo_ui_store::set_lyrics_timestamps,
    ))
}

/// 全局歌词偏移：所有曲目统一提前 / 延后（毫秒；与每首曲目的手动偏移相加生效）。
pub(crate) fn lyrics_offset_section() -> View {
    let s = store();
    let draft = signal(
        s.library
            .with(|l| l.config.global_lyrics_offset_ms)
            .to_string(),
    );
    // 外部改了配置（另一个实例、测试）时同步草稿；computed 只在值变了
    // 才触发——无关的 s.library 写不能把输入中的草稿顶掉。
    let lib_ms = computed(move || s.library.with(|l| l.config.global_lyrics_offset_ms));
    effect(move || draft.set(lib_ms.get().to_string()));
    // 提交（回车 / 失焦）：解析失败回退为当前值，成功则钳到 ±30000 并规范化草稿。
    // 弹窗拆到一半输入框还会收一次失焦，那时 draft 已随 owner 销毁。
    let owner = Owner::current();
    let commit = move || {
        if !owner.is_live() {
            return;
        }
        match draft.get().trim().trim_start_matches('+').parse::<i64>() {
            Ok(ms) => {
                let ms = ms.clamp(-30000, 30000);
                zeo_ui_store::set_global_lyrics_offset(ms);
                draft.set(ms.to_string());
            }
            Err(_) => draft.set(
                s.library
                    .with(|l| l.config.global_lyrics_offset_ms)
                    .to_string(),
            ),
        }
    };
    let field = input_field(true, |i| {
        i.label("全局歌词偏移")
            .keyboard_type(KeyboardType::Number)
            .value(move || draft.get())
            .on_change(move |t: &str| draft.set(t.to_owned()))
            .on_submit(move |_: &str| commit())
            .on_blur(move || commit())
    })
    .w(px(88.0));
    setting_section(
        "全局歌词偏移",
        "对所有曲目生效：蓝牙耳机等固定延迟用它补偿。正值提前显示，负值延后；与全屏歌词区里每首曲目的偏移相加生效。",
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
                        text("全局歌词偏移")
                            .text_size(px(14.0))
                            .line_height(px(14.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg()),
                    )
                    .child(
                        text("毫秒，±30000 内；正值让歌词提前。")
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
