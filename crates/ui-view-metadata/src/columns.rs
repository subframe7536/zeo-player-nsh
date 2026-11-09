use super::*;

pub(crate) fn all_columns() -> BTreeSet<String> {
    table::COLUMNS.iter().map(|(k, ..)| k.to_string()).collect()
}

pub(crate) const COLUMN_GROUPS: &[(&str, &[&str])] = &[
    (
        "基本信息",
        &[
            "file_name",
            "title",
            "artist",
            "album",
            "album_artist",
            "genre",
            "year",
            "track_number",
            "track_total",
            "disc_number",
            "discs_total",
        ],
    ),
    (
        "人员与版权",
        &[
            "composer",
            "conductor",
            "lyricist",
            "publisher",
            "copyright",
        ],
    ),
    ("封面与歌词", &["rating", "pictures", "lyrics", "comment"]),
    (
        "ReplayGain",
        &[
            "track_replay_gain",
            "track_replay_peak",
            "album_replay_gain",
            "album_replay_peak",
        ],
    ),
];

/// 默认列：包含「基本信息」与「封面与歌词」集合中的全部列（共 15 列）。
pub(crate) fn default_columns() -> BTreeSet<String> {
    let mut cols = BTreeSet::new();
    for (group_name, keys) in COLUMN_GROUPS {
        if *group_name == "基本信息" || *group_name == "封面与歌词" {
            for key in *keys {
                cols.insert((*key).to_string());
            }
        }
    }
    cols
}

pub(crate) fn column_selector(visible_cols: Signal<BTreeSet<String>>) -> AnyElement {
    let open = signal(false);
    let gate = zeo_ui_store::menu_control();
    effect(gate.evicted(open));

    let trigger = view()
        .h(px(32.0))
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .px(px(10.0))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(border())
        .cursor_pointer()
        .window_drag(false)
        .bg(move || {
            if gate.is_open() {
                hover()
            } else {
                nsh_base::theme().card
            }
        })
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.8))
        .role(AccessibilityRole::Button)
        .label("列显示设置")
        .child(
            text(move || {
                let count = visible_cols.with(|s| s.len());
                let total = table::COLUMNS.len();
                format!("显示列 ({count}/{total})")
            })
            .text_size(px(13.0))
            .line_height(px(13.0))
            .text_color(fg()),
        )
        .child(icon(Icon::ChevronDown, 12.0, muted_fg));

    let mut items = Vec::new();
    for (group_name, keys) in COLUMN_GROUPS {
        items.push(
            view()
                .px(px(8.0))
                .pt(px(8.0))
                .pb(px(4.0))
                .child(
                    text(*group_name)
                        .text_size(px(11.0))
                        .line_height(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(muted_fg()),
                )
                .into_element(),
        );
        for key in *keys {
            let key_s = (*key).to_string();
            let title_str = table::COLUMNS
                .iter()
                .find(|(k, ..)| *k == *key)
                .map(|(.., t, _, _, _)| *t)
                .unwrap_or(*key);
            let is_checked = {
                let key_s = key_s.clone();
                move || visible_cols.with(|s| s.contains(&key_s))
            };
            let toggle = {
                let key_s = key_s.clone();
                move || {
                    visible_cols.update(|set| {
                        if set.contains(&key_s) {
                            if set.len() > 1 {
                                set.remove(&key_s);
                            }
                        } else {
                            set.insert(key_s.clone());
                        }
                    });
                }
            };
            let on_row_click = toggle.clone();
            items.push(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(8.0))
                    .py(px(5.0))
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .hover(|st| st.bg(hover()))
                    .active(|st| st.opacity(0.85))
                    .on_press(move || on_row_click())
                    .child(checkbox(is_checked, || false, title_str, {
                        let toggle = toggle.clone();
                        move |_| toggle()
                    }))
                    .child(
                        text(title_str)
                            .flex_1()
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(fg()),
                    )
                    .into_element(),
            );
        }
    }

    let popup = view()
        .w(px(230.0))
        .h(px(390.0))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(border())
        .bg(nsh_base::theme().popover)
        .shadow_md()
        .overflow_hidden()
        .flex_col()
        .window_drag(false)
        .child(
            view()
                .h(px(36.0))
                .px(px(12.0))
                .border_b_1()
                .border_b_color(border())
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text("选择显示列")
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(muted_fg()),
                )
                .child(
                    view()
                        .flex_row()
                        .gap(px(4.0))
                        .child(text_button(
                            Variant::Ghost,
                            Size::Xs,
                            None,
                            "全选",
                            move || visible_cols.set(all_columns()),
                        ))
                        .child(text_button(
                            Variant::Ghost,
                            Size::Xs,
                            None,
                            "默认",
                            move || visible_cols.set(default_columns()),
                        )),
                ),
        )
        .child(
            scroll()
                .flex_1()
                .min_h(px(0.0))
                .child(view().p(px(6.0)).gap(px(2.0)).children(items)),
        );

    let close = {
        let open = open.clone();
        move || {
            open.set(false);
            gate.close();
        }
    };

    let selector = popover()
        .open(gate.open_with(open))
        .on_open_change(gate.popover_change(open))
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(6.0))
        .margin(px(8.0))
        .trigger(trigger)
        .child(popup);

    view()
        .child(when(
            move || gate.is_open(),
            move || {
                overlay()
                    .test_id("column_selector_overlay")
                    .absolute()
                    .inset_0()
                    .on_press({
                        let close = close.clone();
                        move || close()
                    })
                    .on_context_menu({
                        let close = close.clone();
                        move |_: &nsh_core::ContextMenuEvent| close()
                    })
            },
        ))
        .child(selector)
        .into_element()
}
