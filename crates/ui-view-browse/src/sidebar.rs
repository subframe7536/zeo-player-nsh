use super::*;

/// 侧栏标签（流派）单行。
pub(crate) fn tag_sidebar_row(
    label: String,
    count: usize,
    selected: impl Fn() -> bool + Clone + 'static,
    on_press: impl Fn() + 'static,
) -> AnyElement {
    let sel_bg = selected.clone();
    let sel_state = selected.clone();
    let sel_icon = selected.clone();
    let sel_text = selected.clone();
    view()
        .w_full()
        .h(px(34.0))
        .flex_row()
        .items_center()
        .pl(px(10.0))
        .pr(px(10.0))
        .gap(px(10.0))
        .rounded(px(8.0))
        .overflow_hidden()
        .cursor(Cursor::Pointer)
        .role(AccessibilityRole::Link)
        .label(label.clone())
        .accessibility_state(move || AccessibilityState {
            selected: sel_state(),
            ..AccessibilityState::default()
        })
        .bg(move || {
            if sel_bg() {
                primary().opacity(0.12)
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .hover(|st| st.bg(move || primary().opacity(0.08)))
        .on_press(move |_: &()| on_press())
        .child(
            view()
                .size(px(20.0))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .child(icon(Icon::Tag, 16.0, move || {
                    if sel_icon() { primary() } else { sidebar_fg() }
                })),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(label)
                        .number_of_lines(1)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || if sel_text() { fg() } else { sidebar_fg() }),
                )
                .child(
                    text(count.to_string())
                        .text_size(px(11.0))
                        .line_height(px(11.0))
                        .tabular_nums()
                        .text_color(faint()),
                ),
        )
        .into_element()
}

/// 侧栏标签（流派）列表。
pub fn tags_sidebar_list() -> impl IntoElement {
    let s = store();
    let nav = global::<zeo_ui_store::Nav>();
    let groups = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(group_by(&tracks, |t| {
            t.genres
                .iter()
                .map(|g| (g.clone(), g.clone(), String::new()))
                .collect()
        }))
    });
    scroll()
        .flex_1()
        .child(
            view()
                .px(px(10.0))
                .pb(px(8.0))
                .gap(px(2.0))
                .child(dynamic(move || {
                    let list = groups.get();
                    if list.is_empty() {
                        return vec![
                            text("暂无标签")
                                .text_size(px(12.0))
                                .line_height(px(18.0))
                                .text_color(faint())
                                .px(px(10.0))
                                .py(px(4.0))
                                .into_element(),
                        ];
                    }
                    list.iter()
                        .map(|g| {
                            let key = g.key.clone();
                            let title = g.title.clone();
                            let count = g.tracks.len();
                            let active = {
                                let key = key.clone();
                                move || {
                                    nav.get() == zeo_ui_store::Nav::Genres
                                        && s.genre_sel.with(|sel| sel.as_deref() == Some(&key))
                                }
                            };
                            tag_sidebar_row(title, count, active, move || {
                                s.genre_sel.set(Some(key.clone()));
                                zeo_ui_store::navigate(zeo_ui_store::Nav::Genres);
                            })
                        })
                        .collect()
                })),
        )
}
