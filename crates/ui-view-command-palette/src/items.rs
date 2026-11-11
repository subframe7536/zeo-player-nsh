use super::*;

pub(crate) fn matches_keyword(query: &str, label: &str, keywords: &[&str]) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    label.to_lowercase().contains(&q) || keywords.iter().any(|k| k.to_lowercase().contains(&q))
}

pub(crate) fn shortcut_pill(key: &'static str, label: &'static str) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .child(
            text(key)
                .text_size(px(10.0))
                .line_height(px(10.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(faint())
                .px(px(4.0))
                .py(px(2.0))
                .rounded(px(3.0))
                .border_1()
                .border_color(border().opacity(0.5)),
        )
        .child(
            text(label)
                .text_size(px(11.0))
                .line_height(px(11.0))
                .text_color(faint()),
        )
}

pub(crate) fn command_item_view(entry: &CommandEntry) -> View {
    let id = entry.id.to_string();
    let label = entry.label.to_string();
    let group = entry.group.as_deref().unwrap_or_default();

    if group == "曲目" || id.starts_with("track:") {
        let artist = entry
            .keywords
            .first()
            .map(|s| s.as_ref())
            .unwrap_or_default();
        let album = entry
            .keywords
            .get(1)
            .map(|s| s.as_ref())
            .unwrap_or_default();
        let dur = entry
            .keywords
            .get(3)
            .map(|s| s.as_ref())
            .unwrap_or_default();
        let sub = match (!artist.is_empty(), !album.is_empty()) {
            (true, true) => format!("{artist} · {album}"),
            (true, false) => artist.to_owned(),
            (false, true) => album.to_owned(),
            (false, false) => String::new(),
        };

        let mut info = view().flex_1().min_w(px(0.0)).gap(px(2.0)).child(
            text(label)
                .number_of_lines(1)
                .text_size(px(13.0))
                .line_height(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg()),
        );
        if !sub.is_empty() {
            info = info.child(
                text(sub)
                    .number_of_lines(1)
                    .text_size(px(11.0))
                    .line_height(px(11.0))
                    .text_color(muted_fg()),
            );
        }

        let mut item_view = view()
            .flex_1()
            .min_w(px(0.0))
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .child(
                view()
                    .size(px(28.0))
                    .rounded(px(6.0))
                    .bg(primary().opacity(0.08))
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .child(icon(Icon::Note, 14.0, faint)),
            )
            .child(info);

        if !dur.is_empty() {
            item_view = item_view.child(
                text(dur.to_owned())
                    .text_size(px(11.0))
                    .line_height(px(11.0))
                    .text_color(faint())
                    .flex_shrink_0(),
            );
        }

        item_view
    } else {
        let glyph = match id.as_str() {
            "nav:home" => Icon::Home,
            "nav:library" => Icon::Note,
            "nav:artists" => Icon::Mic,
            "nav:albums" => Icon::Disc,
            "nav:metadata" => Icon::Edit,
            "nav:folders" => Icon::Folder,
            "nav:genres" => Icon::Tag,
            "action:settings" => Icon::Settings,
            "action:play_pause" => Icon::Play,
            "action:next" => Icon::NextLine,
            "action:prev" => Icon::PrevLine,
            "action:like" => Icon::Heart,
            "action:loop" => Icon::Repeat,
            "action:mute" => Icon::Volume,
            "action:fullscreen" => Icon::Expand,
            "action:scan" => Icon::Refresh,
            _ => Icon::Search,
        };

        let badge_text = if group == "页面" {
            "页面"
        } else {
            "操作"
        };

        view()
            .flex_1()
            .min_w(px(0.0))
            .h(px(32.0))
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .child(
                view()
                    .size(px(28.0))
                    .rounded(px(6.0))
                    .bg(primary().opacity(0.08))
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .child(icon(glyph, 14.0, faint)),
            )
            .child(
                text(label)
                    .flex_1()
                    .min_w(px(0.0))
                    .number_of_lines(1)
                    .text_size(px(13.0))
                    .line_height(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg()),
            )
            .child(
                view()
                    .flex_shrink_0()
                    .child(if let Some(sc) = &entry.shortcut_label {
                        text(sc.clone())
                            .text_size(px(10.0))
                            .line_height(px(10.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(faint())
                            .px(px(5.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(border().opacity(0.5))
                            .into_element()
                    } else {
                        text(badge_text)
                            .text_size(px(10.0))
                            .line_height(px(10.0))
                            .text_color(faint())
                            .px(px(5.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(primary().opacity(0.06))
                            .into_element()
                    }),
            )
    }
}
