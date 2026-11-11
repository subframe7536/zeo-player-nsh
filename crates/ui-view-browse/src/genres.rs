use super::*;

#[component]
pub fn Genres() -> impl IntoElement {
    let s = store();
    let selected = s.genre_sel;
    let groups = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(group_by(&tracks, |t| {
            t.genres
                .iter()
                .map(|g| (g.clone(), g.clone(), String::new()))
                .collect()
        }))
    });
    view().flex_1().child(dynamic(move || {
        if s.tracks.with(|t| t.is_empty()) {
            return no_library().into_element();
        }
        let groups_ref = groups.get();
        if groups_ref.is_empty() {
            return empty_state(
                Icon::Tag,
                "没有流派标签",
                "曲目的流派标签会出现在这里（按 ; 拆分）。",
            )
            .into_element();
        }
        let cur_key = match selected.get() {
            Some(k) => k,
            None => {
                let first = groups_ref[0].key.clone();
                selected.set(Some(first.clone()));
                first
            }
        };
        let group = groups_ref.iter().find(|g| g.key == cur_key).cloned();
        let Some(group) = group else {
            selected.set(None);
            return view().into_element();
        };
        let items = computed({
            let list = group.tracks.clone();
            move || list.clone()
        });
        let title = format!("# {}", group.title);
        let subtitle = format!(
            "{} 首 · {}",
            group.tracks.len(),
            format_total(total_duration(&group.tracks))
        );
        let header = view()
            .child(page_header(
                move || title.clone(),
                move || subtitle.clone(),
                None,
                Some(
                    pill_button(Icon::Play, "播放", move || {
                        items.with_untracked(|v| play_list(v, 0))
                    })
                    .into_element(),
                ),
            ))
            .child(track_columns(Columns::ALL));
        view()
            .flex_1()
            .child(track_list(items, Columns::ALL, header.into_element()))
            .into_element()
    }))
}
