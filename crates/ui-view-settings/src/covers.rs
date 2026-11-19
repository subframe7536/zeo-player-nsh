//! 设置 →「封面」分类的各节。

use nsh_core::prelude::*;

use super::setting_section;
use zeo_ui_store::store;

/// 本地同名图片优先于内嵌封面。
pub(crate) fn local_first_section() -> View {
    let s = store();
    setting_section(
        "本地图片优先",
        "同目录的同名图片（jpg / jpeg / png / webp）优先于内嵌封面。",
        Some(
            zeo_ui_components::form::switch(
                move || s.library.with(|l| l.config.prefer_local_covers),
                "本地图片优先",
                zeo_ui_store::set_prefer_local_covers,
            )
            .into_element(),
        ),
    )
}

/// 列表封面开关：关掉不加载封面，大曲库滚动更省内存。
pub(crate) fn list_covers_section() -> View {
    let s = store();
    setting_section(
        "在列表中显示封面",
        "关闭后列表不加载封面（显示首字占位），大曲库滚动更省内存。",
        Some(
            zeo_ui_components::form::switch(
                move || s.library.with(|l| l.config.show_covers),
                "在列表中显示封面",
                zeo_ui_store::set_show_covers,
            )
            .into_element(),
        ),
    )
}
