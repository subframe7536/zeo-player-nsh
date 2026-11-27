use super::*;

/// 字段已修改的小徽章（主题色淡底 + 主色字，与在线匹配卡片徽章同款）。
pub(crate) fn modified_badge(label: &'static str) -> View {
    view()
        .px(px(5.0))
        .py(px(1.0))
        .rounded(px(4.0))
        .bg(move || primary().opacity(0.14))
        .child(
            text(label)
                .text_size(px(10.0))
                .line_height(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || primary()),
        )
}

/// 「标签在上、控件在下」的一格：`flex_1` 在行里等分宽度；`dirty` 为真时在标签旁显示「已修改」标识。
pub(crate) fn labeled(
    label: &'static str,
    dirty: impl Fn() -> bool + 'static,
    child: impl IntoElement,
) -> View {
    view()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(6.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .h(px(18.0))
                .gap(px(6.0))
                .child(
                    text(label)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(muted_fg()),
                )
                .child(when(dirty, || modified_badge("已修改"))),
        )
        .child(child)
}

/// 标签字段区：文本字段 + 流派标签输入 + 数字行（标签都在输入框上方）。
pub(crate) fn fields(draft: Signal<TagDraft>) -> View {
    let row = |child: View| view().flex_row().gap(px(12.0)).child(child);
    let dirty = move |field: TagField| move || draft.with(|d| field_dirty(d, field));
    view()
        .gap(px(14.0))
        .child(row(labeled(
            "标题",
            dirty(TagField::Title),
            field_input("标题", TagField::Title, draft)
                .auto_focus(true)
                .w_full(),
        )))
        .child(
            view()
                .flex_row()
                .gap(px(12.0))
                .child(labeled(
                    "艺术家",
                    dirty(TagField::Artist),
                    field_input("艺术家", TagField::Artist, draft).w_full(),
                ))
                .child(labeled(
                    "专辑艺术家",
                    dirty(TagField::AlbumArtist),
                    field_input("专辑艺术家", TagField::AlbumArtist, draft).w_full(),
                )),
        )
        .child(
            view()
                .flex_row()
                .gap(px(12.0))
                .child(labeled(
                    "专辑",
                    dirty(TagField::Album),
                    field_input("专辑", TagField::Album, draft).w_full(),
                ))
                .child(labeled(
                    "作曲",
                    dirty(TagField::Composer),
                    field_input("作曲", TagField::Composer, draft).w_full(),
                )),
        )
        .child(row(labeled(
            "标签",
            dirty(TagField::Genre),
            genre_row(draft),
        )))
        .child(
            view()
                .flex_row()
                .gap(px(12.0))
                .child(labeled(
                    "年份",
                    dirty(TagField::Year),
                    field_input("年份", TagField::Year, draft).w_full(),
                ))
                .child(labeled(
                    "音轨",
                    move || {
                        draft.with(|d| {
                            field_dirty(d, TagField::Track) || field_dirty(d, TagField::TrackTotal)
                        })
                    },
                    pair(
                        draft,
                        "音轨号",
                        TagField::Track,
                        "音轨总数",
                        TagField::TrackTotal,
                    ),
                ))
                .child(labeled(
                    "碟号",
                    move || {
                        draft.with(|d| {
                            field_dirty(d, TagField::Disc) || field_dirty(d, TagField::DiscTotal)
                        })
                    },
                    pair(draft, "碟号", TagField::Disc, "碟总数", TagField::DiscTotal),
                )),
        )
        .child(row(labeled(
            "注释",
            dirty(TagField::Comment),
            field_input("注释", TagField::Comment, draft).w_full(),
        )))
}

/// 单个标签字段的输入框（`text_input` 36 高纯文本样式；宽 / 聚焦由
/// 调用处加）。`label` 同时是无障碍名。
fn field_input(label: &'static str, field: TagField, draft: Signal<TagDraft>) -> Input {
    let initial = untracked(|| {
        draft
            .with(|d| d.values.get(&field).cloned())
            .unwrap_or_default()
    });
    text_input(false)
        .label(label)
        .default_value(initial)
        .readonly(move || draft.with(|d| d.saving))
        .on_change(move |t: &str| {
            draft.update(|d| {
                d.values.insert(field, t.to_owned());
            })
        })
}

/// 「号 / 总数」并排输入（`n/m` 拆成两格）：各 flex_1，中间一道淡淡
/// 的斜杠；总数格的占位是「总数」。
fn pair(
    draft: Signal<TagDraft>,
    number_label: &'static str,
    number: TagField,
    total_label: &'static str,
    total: TagField,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(field_input(number_label, number, draft).flex_1())
        .child(
            text("/")
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(faint()),
        )
        .child(
            field_input(total_label, total, draft)
                .flex_1()
                .placeholder("总数"),
        )
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
