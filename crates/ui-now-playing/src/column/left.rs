use super::*;

pub(crate) fn left_column(swap: &Swap) -> View {
    let s = store();
    let artwork = move || s.track.with(|t| t.artwork.clone());

    let art = view()
        .size(move || px(column_size()))
        .rounded(px(10.0))
        .overflow_hidden()
        .bg(white_a(0.08))
        .shadow_xl()
        .items_center()
        .justify_center()
        .child(icon(Icon::Note, 96.0, || white_a(0.3)))
        .child(when(
            move || artwork().is_some(),
            move || {
                image(move || artwork().unwrap_or_default())
                    .absolute()
                    .inset(px(0.0))
                    .object_fit(ResizeMode::Cover)
            },
        ));

    let title_row = view()
        .h(px(28.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text(move || {
                s.track.with(|t| {
                    if t.title.is_empty() {
                        "Zeo Player".into()
                    } else {
                        t.title.clone()
                    }
                })
            })
            .flex_1()
            .number_of_lines(1)
            .text_size(px(20.0))
            .line_height(px(20.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(white_a(0.95)),
        )
        .child(more_menu());

    let meta_line = |glyph: Icon, value: fn(&zeo_ui_store::NowPlaying) -> String| {
        view()
            .h(px(18.0))
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .child(icon(glyph, 13.0, || white_a(0.6)))
            .child(
                text(move || s.track.with(value))
                    .flex_1()
                    .number_of_lines(1)
                    .text_size(px(13.0))
                    .line_height(px(13.0))
                    .text_color(white_a(0.7)),
            )
    };

    let tags = dynamic(move || {
        // 流派按设置里的分隔符拆开（和标签页一致），分隔符一改就跟着变。
        let separators = s.library.with(|l| l.config.genre_separators.clone());
        let list: Vec<(String, bool)> = s.meta.with(|m| {
            let Some(m) = m else { return Vec::new() };
            let mut out = Vec::new();
            if let Some(q) = quality_tag(m) {
                out.push((q, true));
            }
            if let Some(year) = m.year {
                out.push((year.to_string(), false));
            }
            let genres = zeo_library::config::split_genres(&m.genres, &separators);
            out.extend(genres.into_iter().take(3).map(|g| (g, false)));
            if let Some(codec) = &m.codec {
                out.push((codec.clone(), false));
            }
            out
        });
        (!list.is_empty()).then(|| {
            view()
                .flex_row()
                .gap(px(8.0))
                .children(list.into_iter().map(|(label, strong)| {
                    view()
                        .h(px(20.0))
                        .px(px(7.0))
                        .justify_center()
                        .rounded(px(4.0))
                        .bg(if strong {
                            AdaptiveColor::rgb(0x7fae8a, 0x7fae8a).opacity(0.45)
                        } else {
                            white_a(0.14)
                        })
                        .child(
                            text(label)
                                .number_of_lines(1)
                                .text_size(px(11.0))
                                .line_height(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(white_a(0.85)),
                        )
                }))
        })
    });

    // 与播放条一致：两侧是循环模式与喜欢（最小），中间是上一首 / 播放 /
    // 下一首，播放键最大。
    let skip = |glyph: Icon, label: &'static str, press: fn()| {
        tipped(
            on_dark(44.0)
                .accessibility_label(label)
                .on_press(move |_: &()| press())
                .child(icon(glyph, 18.0, || white_a(0.85))),
            label,
        )
    };
    let side = |glyph: Box<dyn Fn() -> Icon>,
                label: &'static str,
                tip: &'static str,
                tint: Box<dyn Fn() -> AdaptiveColor>,
                press: fn()| {
        tipped(
            on_dark(36.0)
                .accessibility_label(label)
                .on_press(move |_: &()| press())
                .child(icon_dyn(glyph, 16.0, tint)),
            tip,
        )
    };
    let play = on_dark(56.0)
        .accessibility_label("播放/暂停")
        .active(|st| st.scale(0.92))
        .transition_transform()
        .duration(120.0)
        .on_press(|_: &()| zeo_ui_store::toggle())
        .child(zeo_ui_components::icons::play_pause(
            34.0,
            zeo_ui_store::shown_playing,
            || white_a(0.95),
        ));
    let mode_icon = move || match s.playback.with(|p| p.loop_mode) {
        LoopMode::List => Icon::Repeat,
        LoopMode::Single => Icon::RepeatOne,
        LoopMode::Random => Icon::Shuffle,
    };
    let controls = view()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(18.0))
        .child(side(
            Box::new(mode_icon),
            "循环模式（顺序 / 单曲 / 随机）",
            "循环模式",
            Box::new(move || {
                white_a(if s.playback.with(|p| p.loop_mode) == LoopMode::List {
                    0.55
                } else {
                    0.95
                })
            }),
            zeo_ui_store::cycle_loop_mode,
        ))
        .child(skip(Icon::PrevLine, "上一首", || player().previous()))
        .child(play)
        .child(skip(Icon::NextLine, "下一首", || player().next()))
        .child(side(
            Box::new(|| {
                if zeo_ui_store::current_liked() {
                    Icon::HeartFilled
                } else {
                    Icon::Heart
                }
            }),
            "喜欢",
            "喜欢",
            Box::new(like_red),
            zeo_ui_store::toggle_like_current,
        ));

    view()
        .w(move || px(column_size()))
        .flex_shrink_0()
        // 换曲时封面与曲目信息一起淡入、从切歌方向滑入；进度与控制不动。
        .child(
            view()
                .bind_opacity(&swap.content)
                .bind_translate_x(&swap.shift)
                .child(art)
                .child(view().h(px(24.0)))
                .child(title_row)
                .child(
                    view()
                        .gap(px(6.0))
                        .pt(px(14.0))
                        .child(meta_line(Icon::Person, |t| t.artist.clone()))
                        .child(meta_line(Icon::Disc, |t| t.album.clone())),
                )
                // 标签只占一行（多了裁掉），没有标签时也留出这一行：下面的
                // 进度条位置不随元数据载入而跳动。
                .child(
                    view()
                        .mt(px(14.0))
                        .h(px(20.0))
                        .overflow_hidden()
                        .child(tags),
                ),
        )
        .child(view().pt(px(28.0)).child(waveform_seek()))
        .child(view().pt(px(20.0)).child(controls))
        .child(view().pt(px(18.0)).child(volume_row()))
}
