use super::*;

pub(crate) fn volume_row() -> View {
    let s = store();
    let volume = move || s.playback.with(|p| p.volume);
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
                        if volume() <= 0.0 {
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
                set_volume,
                set_volume,
            )
            .flex_1(),
        )
        .child(
            text(move || format!("{}", (volume() * 100.0).round()))
                .w(px(26.0))
                .text_right()
                .text_size(px(11.0))
                .line_height(px(11.0))
                .tabular_nums()
                .text_color(white_a(0.55)),
        )
}
