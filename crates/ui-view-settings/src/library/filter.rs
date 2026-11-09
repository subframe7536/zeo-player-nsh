use super::*;

/// 过滤：低于时长 / 大小阈值的音频不进曲库（改后自动重新扫描）。
pub(crate) fn filter_section() -> View {
    let s = store();
    const DURATIONS: &[(u32, &str)] = &[
        (0, "不过滤"),
        (5, "5 秒"),
        (10, "10 秒"),
        (30, "30 秒"),
        (60, "1 分钟"),
    ];
    const SIZES: &[(u32, &str)] = &[
        (0, "不过滤"),
        (50, "50 KB"),
        (100, "100 KB"),
        (500, "500 KB"),
        (1024, "1 MB"),
    ];
    let row = |label: &'static str,
               options: &'static [(u32, &'static str)],
               field: fn(&zeo_types::config::LibraryConfig) -> u32,
               set: fn(u32)| {
        view()
            .flex_row()
            .items_center()
            .flex_wrap()
            .gap(px(10.0))
            .child(
                text(label)
                    .w(px(80.0))
                    .flex_shrink_0()
                    .text_size(px(14.0))
                    .line_height(px(14.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg()),
            )
            .child(
                zeo_ui_components::radio::group_row().children(options.iter().map(
                    |&(value, name)| {
                        zeo_ui_components::radio::item(
                            move || s.library.with(|l| field(&l.config) == value),
                            name,
                            move || set(value),
                        )
                    },
                )),
            )
            .into_element()
    };
    setting_section(
        "音频过滤",
        "低于时长或大小阈值的音频不进曲库（采样、铃声、碎片文件等），修改后自动重新扫描。",
        None,
    )
    .child(row(
        "最小时长",
        DURATIONS,
        |c| c.min_duration_secs,
        zeo_ui_store::set_min_duration_secs,
    ))
    .child(row(
        "最小大小",
        SIZES,
        |c| c.min_size_kb,
        zeo_ui_store::set_min_size_kb,
    ))
}
