use super::*;

pub(crate) fn library_section() -> View {
    let s = store();
    let progress = move || s.library.with(|l| l.progress.clone());
    let scanning = move || s.library.with(|l| l.progress.scanning);
    let fraction = move || {
        let p = progress();
        if p.total > 0 {
            p.done as f64 / p.total as f64
        } else {
            0.0
        }
    };
    let actions = view()
        .flex_row()
        .gap(px(8.0))
        .style_when(scanning, |st| st.opacity(0.4))
        .child(outline_button(Icon::Refresh, "扫描更新", scan))
        .child(outline_button(
            Icon::Disc,
            "重建曲库",
            zeo_ui_store::rebuild,
        ));

    let stats = move || {
        view()
            .flex_row()
            .items_end()
            .gap(px(24.0))
            .child(stat(
                move || s.tracks.with(|t| t.len()).to_string(),
                "首曲目",
            ))
            .child(stat(
                move || {
                    s.tracks.with(|t| {
                        t.iter()
                            .map(|t| (t.album_artist.as_str(), t.album.as_str()))
                            .collect::<HashSet<_>>()
                            .len()
                            .to_string()
                    })
                },
                "张专辑",
            ))
            .child(stat(
                move || {
                    s.tracks.with(|t| {
                        t.iter()
                            .map(|t| t.artist.as_str())
                            .collect::<HashSet<_>>()
                            .len()
                            .to_string()
                    })
                },
                "位歌手",
            ))
            .child(stat(
                move || format_total(s.tracks.with(|t| total_duration(t))),
                "总时长",
            ))
    };

    let bar = move || {
        view()
            .gap(px(8.0))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        text(move || {
                            let p = progress();
                            if p.total == 0 {
                                "正在查找音频文件…".to_owned()
                            } else {
                                format!("正在读取 {} / {}", p.done, p.total)
                            }
                        })
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .tabular_nums()
                        .text_color(fg()),
                    )
                    .child(view().flex_1())
                    .child(
                        text(move || format!("{:.0}%", fraction() * 100.0))
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .tabular_nums()
                            .text_color(muted_fg()),
                    ),
            )
            .child(
                nsh_base::progress()
                    .accessibility_label("扫描进度")
                    .value(move || (fraction() * 100.0) as f32)
                    .track_style(|st| {
                        st.h(px(6.0))
                            .w_full()
                            .rounded_full()
                            .bg(muted())
                            .overflow_hidden()
                    })
                    .indicator_style(|st| {
                        st.h_full()
                            .rounded_full()
                            .bg(move || primary())
                            .transition_all()
                            .duration(150.0)
                    }),
            )
            .child(
                text(move || progress().current)
                    .number_of_lines(1)
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(faint()),
            )
    };

    setting_section(
        "曲库",
        "索引保存在本机，启动时自动扫描。「重建曲库」会清空标签和封面缓存，从文件重新读取全部曲目。",
        Some(actions.into_element()),
    )
    .child(when(scanning, bar).otherwise(stats))
    .child(view().h(px(1.0)).bg(border()))
    .child(watch_row())
}
