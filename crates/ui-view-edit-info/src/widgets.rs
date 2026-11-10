use super::*;

/// 「基础元数据」栏：文本字段 + 流派标签输入 + 数字行。
pub(crate) fn basic_pane(draft: Signal<TagDraft>) -> View {
    view()
        .gap(px(10.0))
        .child(field_row("标题", TagField::Title, draft, true))
        .child(field_row("歌手", TagField::Artist, draft, false))
        .child(field_row("专辑", TagField::Album, draft, false))
        .child(field_row("专辑歌手", TagField::AlbumArtist, draft, false))
        .child(genre_row(draft))
        .child(number_row(draft))
        .child(disc_row(draft))
        .child(field_row("作曲", TagField::Composer, draft, false))
        .child(field_row("注释", TagField::Comment, draft, false))
}

/// 一行 `标签 + 输入框`。
pub(crate) fn field_row(
    label: &'static str,
    field: TagField,
    draft: Signal<TagDraft>,
    auto_focus: bool,
) -> View {
    let initial = untracked(|| {
        draft
            .with(|d| d.values.get(&field).cloned())
            .unwrap_or_default()
    });
    let input = input_field(false, |i| {
        i.label(label)
            .default_value(initial)
            .auto_focus(auto_focus)
            .readonly(move || draft.with(|d| d.saving))
            .on_change(move |t: &str| {
                draft.update(|d| {
                    d.values.insert(field, t.to_owned());
                })
            })
    })
    .flex_1();
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            text(label)
                .w(px(56.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(input)
}

/// 窄数字输入框（行内并排用）。
pub(crate) fn number_input(
    label: &'static str,
    field: TagField,
    draft: Signal<TagDraft>,
    width: f32,
) -> View {
    let initial = untracked(|| {
        draft
            .with(|d| d.values.get(&field).cloned())
            .unwrap_or_default()
    });
    input_field(false, |i| {
        i.label(label)
            .default_value(initial)
            .readonly(move || draft.with(|d| d.saving))
            .on_change(move |t: &str| {
                draft.update(|d| {
                    d.values.insert(field, t.to_owned());
                })
            })
    })
    .w(px(width))
}

/// 年份、音轨号 / 总数（`n/m` 拆成两格）。
pub(crate) fn number_row(draft: Signal<TagDraft>) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            text("年份")
                .w(px(56.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(number_input("年份", TagField::Year, draft, 72.0))
        .child(
            text("音轨")
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(number_input("音轨号", TagField::Track, draft, 56.0))
        .child(
            text("/")
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(faint()),
        )
        .child(number_input("音轨总数", TagField::TrackTotal, draft, 56.0))
}

/// 碟号 / 碟总数。
pub(crate) fn disc_row(draft: Signal<TagDraft>) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            text("碟号")
                .w(px(56.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(number_input("碟号", TagField::Disc, draft, 56.0))
        .child(
            text("/")
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(faint()),
        )
        .child(number_input("碟总数", TagField::DiscTotal, draft, 56.0))
}

// --- 流派标签输入器 --------------------------------------------------------------

/// `;` 分隔串 → 标签列表（草稿里流派仍是 "a; b" 形式，写回格式不变）。
pub(crate) fn split_genres(raw: &str) -> Vec<String> {
    raw.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
