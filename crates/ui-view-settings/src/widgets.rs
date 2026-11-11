use super::*;

pub(crate) fn setting_section(
    title: &'static str,
    body: &'static str,
    action: Option<AnyElement>,
) -> View {
    view().w_full().gap(px(16.0)).child(
        view()
            .flex_row()
            .flex_wrap()
            .items_start()
            .gap(px(16.0))
            .child(
                view()
                    .flex_1()
                    .min_w(px(240.0))
                    .gap(px(4.0))
                    .child(
                        text(title)
                            .text_size(px(15.0))
                            .line_height(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg()),
                    )
                    .child(text(body).text_size(px(13.0)).text_color(muted_fg())),
            )
            .child(action.map(|a| view().flex_row().gap(px(8.0)).child(a))),
    )
}

/// 一行说明文字（操作结果）。
pub(crate) fn note(message: Signal<String>) -> AnyElement {
    dynamic(move || {
        let now = message.get();
        (!now.is_empty()).then(|| {
            text(now)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg())
        })
    })
}

/// 「本地优先」开关行：标题 + 说明 + 右侧开关。
pub(crate) fn pref_row(
    title: &'static str,
    desc: &'static str,
    checked: impl Fn() -> bool + 'static,
    set: fn(bool),
) -> View {
    let toggle = zeo_ui_components::form::switch(checked, title, set);
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
        .child(toggle)
}

// --- 弹窗 -------------------------------------------------------------------
