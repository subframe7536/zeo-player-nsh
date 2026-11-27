use super::*;

#[component]
pub fn Artists() -> impl IntoElement {
    let s = store();
    // 选中的歌手存在 store 上：首页「热门艺术家」可以深链进详情。
    let selected = s.artist_sel;
    let groups = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(group_by(&tracks, move |t| {
            if t.artists.is_empty() {
                let artist = t.display_artist();
                vec![(artist.to_owned(), artist.to_owned(), String::new())]
            } else {
                t.artists
                    .iter()
                    .map(|a| (a.clone(), a.clone(), String::new()))
                    .collect()
            }
        }))
    });
    let empty = computed(move || s.tracks.with(|t| t.is_empty()));
    view().flex_1().child(dynamic(move || {
        if empty.get() {
            return no_library().into_element();
        }
        match selected.get() {
            Some(key) => {
                let group = untracked(|| {
                    groups.with(|g| {
                        g.iter()
                            .find(|g| g.key == key)
                            .or_else(|| {
                                let seps = s.artist_seps.get();
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
                            Some(200.0),
                        )
                        .shadow_xl()
                    }),
                    group.title.clone(),
                    format!("{} 首 · {} 张专辑", group.tracks.len(), group.albums()),
                    format_total(total_duration(&group.tracks)),
                    Rc::new(move || selected.set(None)),
                    move || items.with_untracked(|v| play_list(v, 0)),
                )
                // 详情列表与音乐库同列（序号 / 大小，见 `Columns::LIBRARY`）。
                .child(track_columns(Columns::LIBRARY).mx(px(-12.0)));
                view()
                    .flex_1()
                    .child(track_list(items, Columns::LIBRARY, header.into_element()))
                    .into_element()
            }
            None => {
                let count = computed(move || groups.with(|g| g.len()));
                let grid = virtual_list(count, move |at| {
                    let group = computed(move || groups.with(|g| g.get(at.index()).cloned()));
                    let cover = move || group.with(|g| g.as_ref().and_then(|g| g.cover.clone()));
                    let name = move || {
                        group.with(|g| g.as_ref().map(|g| g.title.clone()).unwrap_or_default())
                    };
                    nsh_base::button()
                        .p(px(CARD_PAD))
                        .gap(px(CARD_ART_GAP))
                        .rounded(px(CARD_RADIUS))
                        .accessibility_label(move || SharedString::from(name()))
                        .hover(|st| st.bg(hover()))
                        .active(|st| st.opacity(0.75))
                        .on_press(move |_: &()| selected.set(group.get_untracked().map(|g| g.key)))
                        .child(when(covers_on, move || avatar(cover, name, None)))
                        .child(
                            view()
                                .gap(px(STACK_GAP))
                                .px(px(2.0))
                                .child(
                                    text(name)
                                        .number_of_lines(1)
                                        .text_size(px(14.0))
                                        .line_height(px(14.0))
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
                                    .text_size(px(12.0))
                                    .line_height(px(12.0))
                                    .text_color(muted_fg()),
                                ),
                        )
                })
                .columns(grid_columns)
                .estimated_item_size(px(240.0))
                .content_padding_x(px(PAGE_X - CARD_PAD))
                .content_padding_bottom(px(BAR_SPACE))
                .flex_1();
                view()
                    .flex_1()
                    .test_id("artist-grid")
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
