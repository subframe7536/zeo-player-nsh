use super::*;

/// 「删除文件」节：曲目右键「删除文件…」是否经过回收站。
pub(crate) fn delete_section() -> View {
    let s = store();
    setting_section("删除文件", "曲目右键菜单「删除文件…」的行为。", None).child(pref_row(
        "直接删除文件",
        "开启后删除文件不经过回收站（不可恢复）；关闭时文件先移到回收站。",
        move || s.library.with(|l| l.config.delete_permanently),
        zeo_ui_store::set_delete_permanently,
    ))
}
/// 文件夹监视开关与状态。
pub(crate) fn watch_row() -> View {
    let s = store();
    let on = move || s.library.with(|l| l.config.watch);
    let status = move || {
        s.library.with(|l| {
            let w = &l.watcher;
            if !l.config.watch {
                return "关闭时，文件夹里的变化要手动「扫描更新」才会出现。".to_owned();
            }
            if let Some(error) = &w.error {
                return error.clone();
            }
            let mut text = format!(
                "正在监视 {} 个文件夹，新增、修改、删除或改名音频文件后自动扫描",
                w.roots
            );
            if let Some(at) = w.last_rescan {
                let ago = zeo_types::config::unix_now().saturating_sub(at);
                let when = match ago {
                    0..60 => "刚刚".to_owned(),
                    60..3600 => format!("{} 分钟前", ago / 60),
                    _ => format!("{} 小时前", ago / 3600),
                };
                text = format!("{text}。上次：{when}，{} 处变化", w.last_changes);
            }
            text
        })
    };
    let toggle = nsh_base::switch()
        .checked(on)
        .accessibility_label("监视文件夹变化")
        .on_change(move |next: &bool| zeo_ui_store::set_watch(*next))
        .track_style(|st| {
            st.w(px(42.0))
                .h(px(24.0))
                .p(px(2.0))
                .rounded_full()
                .bg(muted())
                .border_1()
                .border_color(border())
                .transition_colors()
                .duration(150.0)
                .checked(|st| st.bg(move || primary()).border_color(move || primary()))
        })
        .thumb_style(|st| {
            st.size(px(18.0))
                .rounded_full()
                .bg(AdaptiveColor::rgb(0xffffff, 0xffffff))
                .shadow_sm()
                .transition_transform()
                .duration(150.0)
                .checked(|st| st.translate_x(px(18.0)))
        });
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
                    text("监视文件夹变化")
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg()),
                )
                .child(
                    text(status)
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(move || {
                            if s.library
                                .with(|l| l.config.watch && l.watcher.error.is_some())
                            {
                                nsh_base::theme().destructive
                            } else {
                                muted_fg()
                            }
                        }),
                ),
        )
        .child(toggle)
}
pub(crate) fn stat(value: impl Fn() -> String + 'static, label: &'static str) -> View {
    view()
        .gap(px(2.0))
        .child(
            text(value)
                .text_size(px(22.0))
                .line_height(px(22.0))
                .font_weight(FontWeight::BOLD)
                .tabular_nums()
                .text_color(fg()),
        )
        .child(
            text(label)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg()),
        )
}
