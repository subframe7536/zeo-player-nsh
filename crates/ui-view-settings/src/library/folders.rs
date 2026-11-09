use super::*;

pub(crate) fn folders_section() -> View {
    let s = store();
    let message = signal(String::new());
    let dragging = signal(false);
    let picking = signal(false);

    let pick = move || {
        if picking.get_untracked() {
            return;
        }
        picking.set(true);
        spawn(async move {
            if let Some(result) = zeo_ui_store::pick_folders().await {
                message.set(result);
            }
            picking.set(false);
        });
    };

    let counts = computed(move || {
        let roots = s.library.with(|l| l.config.roots.clone());
        let tracks = s.tracks.get();
        roots
            .iter()
            .map(|r| {
                let prefix = format!("{}{SEP}", r.trim_end_matches(SEP));
                tracks
                    .iter()
                    .filter(|t| t.path.starts_with(&prefix))
                    .count()
            })
            .collect::<Vec<_>>()
    });

    let rows = dynamic(move || {
        let roots = s.library.with(|l| l.config.roots.clone());
        if roots.is_empty() {
            return vec![
                view()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .h(px(120.0))
                    .rounded(px(12.0))
                    .border_2()
                    .border_dashed()
                    .border_color(move || if dragging.get() { primary() } else { border() })
                    .bg(move || {
                        if dragging.get() {
                            hover()
                        } else {
                            AdaptiveColor::TRANSPARENT
                        }
                    })
                    .child(icon(Icon::Folder, 24.0, faint))
                    .child(
                        text(move || {
                            if dragging.get() {
                                "松开以添加"
                            } else {
                                "还没有文件夹。点「添加文件夹…」，或把文件夹拖到这里"
                            }
                        })
                        .text_size(px(13.0))
                        .text_color(muted_fg()),
                    )
                    .into_element(),
            ];
        }
        roots
            .into_iter()
            .enumerate()
            .map(|(i, root)| {
                let remove = root.clone();
                let name = last_segment(&root).to_owned();
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(12.0))
                    .h(px(56.0))
                    .px(px(12.0))
                    .rounded(px(10.0))
                    .bg(primary().opacity(0.08))
                    .child(
                        view()
                            .size(px(36.0))
                            .rounded(px(9.0))
                            .bg(move || bg())
                            .items_center()
                            .justify_center()
                            .child(icon(Icon::Folder, 18.0, primary)),
                    )
                    .child(
                        view()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap(px(2.0))
                            .child(
                                text(name)
                                    .number_of_lines(1)
                                    .text_size(px(14.0))
                                    .line_height(px(14.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(fg()),
                            )
                            .child(
                                text(root.clone())
                                    .number_of_lines(1)
                                    .text_size(px(12.0))
                                    .line_height(px(12.0))
                                    .text_color(muted_fg()),
                            ),
                    )
                    .child(
                        text(move || {
                            counts.with(|c| c.get(i).map(|n| format!("{n} 首")).unwrap_or_default())
                        })
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .tabular_nums()
                        .text_color(faint()),
                    )
                    .child(icon_button(
                        || Icon::Close,
                        14.0,
                        "移除",
                        muted_fg,
                        move || {
                            library_store().remove_root(&remove);
                            message.set(format!("已移除 {remove}"));
                            scan();
                        },
                    ))
                    .into_element()
            })
            .collect()
    });

    let add = view()
        .style_when(move || picking.get(), |st| st.opacity(0.5))
        .child(pill_button(Icon::Plus, "添加文件夹…", pick));

    setting_section(
        "音乐文件夹",
        "Zeo Player 会扫描这些文件夹里的 MP3、FLAC、WAV、AIFF、M4A（AAC / ALAC）、OGG，包括子文件夹。也可以把文件夹拖到这一节上。",
        Some(add.into_element()),
    )
    // 拖放目标的描边：平时透明，文件拖入时亮主题色。
    .rounded(px(10.0))
    .border_1()
    .border_color(move || {
        if dragging.get() {
            primary()
        } else {
            AdaptiveColor::TRANSPARENT
        }
    })
    .transition_colors()
    .duration(300.0)
    .on_file_drag_enter(move || dragging.set(true))
    .on_file_drag_leave(move || dragging.set(false))
    .on_file_drop(move |event: &FileDropEvent| {
        dragging.set(false);
        let mut added = false;
        let results: Vec<String> = event
            .uris
            .iter()
            .map(|uri| {
                let dropped = add_dropped(&url_to_path(uri));
                added |= dropped.added;
                dropped.note
            })
            .collect();
        if added {
            scan();
        }
        message.set(results.join("\n"));
    })
    .child(view().gap(px(8.0)).child(rows))
    .child(note(message))
}
