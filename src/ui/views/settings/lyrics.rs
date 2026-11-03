//! 设置 →「歌词」分类的各节。

use nsh_core::prelude::*;

use super::super::super::store::store;
use super::{pref_row, setting_section};

/// 全屏歌词的对齐方式。
pub(super) fn lyrics_section() -> View {
    use crate::config::LyricsAlign;
    let s = store();
    const OPTIONS: &[(LyricsAlign, &str)] = &[
        (LyricsAlign::Left, "左对齐"),
        (LyricsAlign::Center, "居中"),
        (LyricsAlign::Right, "右对齐"),
    ];
    setting_section("歌词对齐", "全屏播放页里歌词的对齐方式。", None).child(
        super::super::super::components::radio::group_row().children(OPTIONS.iter().map(
            |&(align, label)| {
                super::super::super::components::radio::item(
                    move || s.library.with(|l| l.config.lyrics_align == align),
                    label,
                    move || super::super::super::store::set_lyrics_align(align),
                )
            },
        )),
    )
}
/// 歌词来源：本地同名 .lrc / 内嵌歌词的优先序。
pub(super) fn lyrics_source_section() -> View {
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
        super::super::super::store::set_prefer_local_lyrics,
    ))
}
