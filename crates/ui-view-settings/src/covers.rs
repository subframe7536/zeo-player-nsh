//! 设置 →「封面」分类的各节。

use nsh_core::prelude::*;

use super::{pref_row, setting_section};
use zeo_ui_store::store;

/// 封面来源：本地同名图片 / 内嵌封面的优先序。
pub(crate) fn covers_source_section() -> View {
    let s = store();
    setting_section(
        "封面来源",
        "封面可以来自曲目同目录的同名图片，或音频文件的内嵌封面。",
        None,
    )
    .child(pref_row(
        "本地图片优先",
        "使用同目录的同名图片（jpg / jpeg / png / webp）作为封面；关闭则优先内嵌封面。",
        move || s.library.with(|l| l.config.prefer_local_covers),
        zeo_ui_store::set_prefer_local_covers,
    ))
    .child(pref_row(
        "在列表中显示封面",
        "关闭后曲目行、网格与队列只显示标题首字，不再解码封面，滚动大曲库更省内存。",
        move || s.library.with(|l| l.config.show_covers),
        zeo_ui_store::set_show_covers,
    ))
}
