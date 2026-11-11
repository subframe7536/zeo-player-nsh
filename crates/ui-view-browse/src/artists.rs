use super::*;

#[component]
pub fn Artists() -> impl IntoElement {
    let s = store();
    // 选中的歌手存在 store 上：首页「热门艺术家」可以深链进详情。
    let selected = s.artist_sel;
    let groups = computed(move || {
        let tracks = s.tracks.get();
        let seps = s.library.with(|l| l.config.artist_separators.clone());
        Rc::new(group_by(&tracks, move |t| {
            let artist = t.display_artist();
            let parts = zeo_library::config::split_artists(artist, &seps);
            if parts.is_empty() {
                vec![(artist.to_owned(), artist.to_owned(), String::new())]
            } else {
                parts
                    .into_iter()
                    .map(|a| (a.clone(), a, String::new()))
                    .collect()
            }
        }))
    });
    view().flex_1().child(dynamic(move || {
        if s.tracks.with(|t| t.is_empty()) {
            return no_library().into_element();
        }
        match selected.get() {
            Some(key) => {
                let group = untracked(|| {
                    groups.with(|g| {
                        g.iter()
                            .find(|g| g.key == key)
                            .or_else(|| {
                                let seps = s.library.with(|l| l.config.artist_separators.clone());
                                let parts = zeo_library::config::split_artists(&key, &seps);
                                parts
                                    .into_iter()
                                    .find_map(|part| g.iter().find(|item| item.key == part))
                            })
                            .cloned()
                    })
                });
                let Some(group) = group else {
                    selected.set(None);
                    return view().into_element();
                };
                let items = computed({
                    let list = group.tracks.clone();
                    move || list.clone()
                });
                let cover_path = stored(group.cover.clone());
                let art_name = stored(group.title.clone());
                let header = hero(
                    "艺术家",
                    when(covers_on, move || {
                        avatar(
                            move || cover_path.get(),
                            move || art_name.get(),
                            Some(168.0),
                        )
                        .shadow_xl()
                    }),
                    group.title.clone(),
                    format!("{} 首 · {} 张专辑", group.tracks.len(), group.albums()),
                    format_total(total_duration(&group.tracks)),
                    Rc::new(move || selected.set(None)),
                    move || items.with_untracked(|v| play_list(v, 0)),
                )
                .child(track_columns(Columns::ALL).mx(px(-12.0)));
                view()
                    .flex_1()
                    .child(track_list(items, Columns::ALL, header.into_element()))
                    .into_element()
            }
            None => {
                let count = computed(move || groups.with(|g| g.len()));
                let columns = move || {
                    let width = content_break() - 80.0;
                    ((width + 8.0) / (184.0 + 8.0)).floor().clamp(2.0, 10.0) as u32
                };
                let grid = virtual_list(count, move |at| {
                    let group = computed(move || groups.with(|g| g.get(at.index()).cloned()));
                    let cover = move || group.with(|g| g.as_ref().and_then(|g| g.cover.clone()));
                    let name = move || {
                        group.with(|g| g.as_ref().map(|g| g.title.clone()).unwrap_or_default())
                    };
                    nsh_base::button()
                        .p(px(8.0))
                        .gap(px(10.0))
                        .rounded(px(14.0))
                        .accessibility_label(move || SharedString::from(name()))
                        .hover(|st| st.bg(hover()))
                        .active(|st| st.opacity(0.75))
                        .on_press(move |_: &()| selected.set(group.get_untracked().map(|g| g.key)))
                        .child(when(covers_on, move || avatar(cover, name, None)))
                        .child(
                            view()
                                .gap(px(2.0))
                                .px(px(2.0))
                                .child(
                                    text(name)
                                        .number_of_lines(1)
                                        .text_size(px(15.0))
                                        .line_height(px(15.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(fg()),
                                )
                                .child(
                                    text(move || {
                                        group.with(|g| {
                                            g.as_ref()
                                                .map(|g| {
                                                    format!(
                                                        "{} 首 · {} 张专辑",
                                                        g.tracks.len(),
                                                        g.albums()
                                                    )
                                                })
                                                .unwrap_or_default()
                                        })
                                    })
                                    .number_of_lines(1)
                                    .text_size(px(13.0))
                                    .line_height(px(13.0))
                                    .text_color(muted_fg()),
                                ),
                        )
                })
                .columns(columns)
                .estimated_item_size(px(240.0))
                .content_padding_x(px(40.0))
                .content_padding_bottom(px(BAR_SPACE))
                .flex_1();
                view()
                    .flex_1()
                    .child(page_header(
                        || "艺术家".into(),
                        move || format!("{} 位艺术家", count.get()),
                        None,
                        None,
                    ))
                    .child(grid)
                    .into_element()
            }
        }
    }))
}

// --- 专辑 ---------------------------------------------------------------------
