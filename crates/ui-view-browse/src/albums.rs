use super::*;

#[component]
pub fn Albums() -> impl IntoElement {
    let s = store();
    let selected = signal(None::<String>);
    let groups = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(group_by(&tracks, |t| {
            let artist = if t.album_artist.is_empty() {
                t.display_artist().to_owned()
            } else {
                t.album_artist.clone()
            };
            let album = t.display_album().to_owned();
            vec![(format!("{artist}\u{0}{}", album), album, artist)]
        }))
    });
    view().flex_1().child(dynamic(move || {
        if s.tracks.with(|t| t.is_empty()) {
            return no_library().into_element();
        }
        match selected.get() {
            Some(key) => {
                let group = untracked(|| groups.with(|g| g.iter().find(|g| g.key == key).cloned()));
                let Some(group) = group else {
                    selected.set(None);
                    return view().into_element();
                };
                let items = computed({
                    let list = group.tracks.clone();
                    move || list.clone()
                });
                let cover_path = group.cover.clone();
                let cover_src =
                    computed(move || cover_path.as_deref().and_then(|p| cover_for(&p, 200.0)));
                let meta = [
                    group.year.map(|y| y.to_string()),
                    Some(format!("{} 首", group.tracks.len())),
                    Some(format_total(total_duration(&group.tracks))),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
                let first = stored(
                    group
                        .title
                        .chars()
                        .next()
                        .map(String::from)
                        .unwrap_or_default(),
                );
                let header = hero(
                    "专辑",
                    when(covers_on, move || {
                        cover_initial(
                            move || cover_src.get(),
                            move || first.get(),
                            Some(200.0),
                            12.0,
                        )
                        .shadow_xl()
                    }),
                    group.title.clone(),
                    group.subtitle.clone(),
                    meta,
                    Rc::new(move || selected.set(None)),
                    move || items.with_untracked(|v| play_list(v, 0)),
                )
                .child(track_columns(ALBUM_COLS).mx(px(-12.0)));
                view()
                    .flex_1()
                    .child(track_list(items, ALBUM_COLS, header.into_element()))
                    .into_element()
            }
            None => {
                let count = computed(move || groups.with(|g| g.len()));
                let columns = move || {
                    let width = content_break() - 40.0;
                    ((width + 8.0) / (184.0 + 8.0)).floor().clamp(2.0, 10.0) as u32
                };
                let grid = virtual_list(count, move |at| {
                    let group = computed(move || groups.with(|g| g.get(at.index()).cloned()));
                    let field = move |f: fn(&Group) -> String| {
                        move || group.with(|g| g.as_ref().map(f).unwrap_or_default())
                    };
                    let cover_path =
                        move || group.with(|g| g.as_ref().and_then(|g| g.cover.clone()));
                    let title = field(|g| g.title.clone());
                    nsh_base::button()
                        .p(px(8.0))
                        .gap(px(10.0))
                        .rounded(px(14.0))
                        .accessibility_label(move || SharedString::from(title()))
                        .hover(|st| st.bg(hover()))
                        .active(|st| st.opacity(0.75))
                        .on_press(move |_: &()| selected.set(group.get_untracked().map(|g| g.key)))
                        .child(when(covers_on, move || {
                            cover_initial(
                                move || cover_path().and_then(|p| cover_for(&p, 184.0)),
                                move || {
                                    title().chars().next().map(String::from).unwrap_or_default()
                                },
                                None,
                                10.0,
                            )
                            .shadow_md()
                        }))
                        .child(
                            view()
                                .gap(px(2.0))
                                .px(px(2.0))
                                .child(
                                    text(field(|g| g.title.clone()))
                                        .number_of_lines(1)
                                        .text_size(px(15.0))
                                        .line_height(px(15.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(fg()),
                                )
                                .child(
                                    text(field(|g| g.subtitle.clone()))
                                        .number_of_lines(1)
                                        .text_size(px(13.0))
                                        .line_height(px(13.0))
                                        .text_color(muted_fg()),
                                ),
                        )
                })
                .columns(columns)
                .estimated_item_size(px(240.0))
                .content_padding_x(px(20.0))
                .content_padding_bottom(px(BAR_SPACE))
                .flex_1();
                let grid_ref = element_ref();
                #[cfg(any(debug_assertions, test))]
                zeo_ui_store::BENCH_GRID.with(|g| g.set(Some(grid_ref)));
                let grid = grid.element_ref(&grid_ref);
                view()
                    .flex_1()
                    .child(page_header(
                        || "专辑".into(),
                        move || format!("{} 张专辑", count.get()),
                        None,
                        None,
                    ))
                    .child(grid)
                    .into_element()
            }
        }
    }))
}

// --- 标签 ---------------------------------------------------------------------
