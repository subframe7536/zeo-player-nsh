use super::*;

pub(crate) fn volume_row() -> View {
    let s = store();
    let volume = move || s.playback.with(|p| p.volume);
    // 拖动中的本地音量：图标与百分比跟手，`s.playback` 与媒体会话
    // 留到抬起 `set_volume` 一次（同均衡器推子的 live_db）；滑条自身
    // 的 thumb / 填充由上游 pending 原生跟手，受控值仍给引擎音量。
    let live = signal(None::<f64>);
    let shown = move || live.get().unwrap_or_else(volume);
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(tipped(
            on_dark(28.0)
                .accessibility_label("静音")
                .on_press(|_: &()| toggle_mute())
                .child(icon_dyn(
                    move || {
                        if shown() <= 0.0 {
                            Icon::Mute
                        } else {
                            Icon::Volume
                        }
                    },
                    15.0,
                    || white_a(0.7),
                )),
            "静音",
        ))
        .child(
            slider(
                SliderStyle::HORIZONTAL,
                "音量",
                volume,
                white_a(0.2),
                white_a(0.9),
                white_a(1.0),
                move |f| {
                    live.set(Some(f));
                    set_volume_live(f);
                },
                move |f| {
                    set_volume(f);
                    live.set(None);
                },
            )
            .flex_1(),
        )
        .child(
            text(move || format!("{}", (shown() * 100.0).round()))
                .w(px(26.0))
                .text_right()
                .text_size(px(11.0))
                .line_height(px(11.0))
                .tabular_nums()
                .text_color(white_a(0.55)),
        )
}
