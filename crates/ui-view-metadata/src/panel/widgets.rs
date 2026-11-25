use super::*;

/// 节标题：12pt semibold muted。
pub(crate) fn section_title(title: &'static str) -> Text {
    text(title)
        .text_size(px(12.0))
        .line_height(px(12.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(muted_fg())
}

/// 一节：标题 + 间距 10 的若干行（无内部卡片圆角边框）。
pub(crate) fn section(title: &'static str, rows: Vec<View>) -> View {
    view()
        .gap(px(10.0))
        .child(section_title(title))
        .children(rows)
}

/// 行首标签：72pt 列，13pt 淡色，与控件垂直居中。
pub(crate) fn label_cell(label: &'static str) -> Text {
    text(label)
        .w(px(72.0))
        .text_size(px(13.0))
        .line_height(px(13.0))
        .text_color(muted_fg())
}

/// 批量面板的一行 `标签 + 控件组`：控件组占满标签右侧余宽并可折行——
/// 面板窄时放不下的控件落到下一行、仍与控件列左对齐，而不是被裁掉。
/// 想占满余宽的控件自己设 `flex_1().min_w(N)`：`N` 就是它折行的门槛。
/// 标签与首行垂直居中对齐：控件组首位放一根 32pt 高、零宽的撑杆（左移一个
/// gap 抵掉它占的间隔），把每行撑到输入框高——只有按钮这类矮控件的行
/// 也与标签同一条中线，而不是贴着行顶。
pub(crate) fn wrap_row(label: &'static str, controls: Vec<AnyElement>) -> View {
    view()
        .flex_row()
        .items_start()
        .gap(px(8.0))
        .child(
            view()
                .h(px(32.0))
                .flex_shrink_0()
                .justify_center()
                .child(label_cell(label)),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(8.0))
                .child(view().w(px(0.0)).h(px(32.0)).ml(px(-8.0)))
                .children(controls),
        )
}

/// 加载中的输入框占位：与 `text_input` 同款外观，无法获得焦点，文字显示「加载中...」。
pub(crate) fn loading_input(small: bool, width: Option<f32>) -> View {
    let t = nsh_base::theme();
    let pad = match width {
        Some(w) if w < 70.0 => 4.0,
        _ => 12.0,
    };
    let mut v = view()
        .h(px(if small { 32.0 } else { 36.0 }))
        .px(px(pad))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(t.input)
        .bg(t.card)
        .items_start()
        .justify_center()
        .overflow_hidden();
    if let Some(w) = width {
        v = v.w(px(w));
    } else {
        v = v.flex_1().min_w(px(0.0));
    }
    v.child(
        text("加载中...")
            .number_of_lines(1)
            .text_size(px(12.0))
            .line_height(px(12.0))
            .text_color(faint()),
    )
}

/// 一行 `标签 + 输入框`（标签列 72pt，控件占满余宽）。
pub(crate) fn field_row(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell(label))
        .child(dynamic(move || {
            let is_ready =
                untracked(|| draft.with(|d| d.values.contains_key(&field))) || full.get().is_some();
            if !is_ready {
                return loading_input(true, None).into_element();
            }
            let initial = untracked(|| {
                draft
                    .with(|d| d.values.get(&field).cloned())
                    .or_else(|| {
                        full.with(|f| f.as_ref().and_then(|(m, _, _)| m.get(&field).cloned()))
                    })
                    .unwrap_or_default()
            });
            text_input(true)
                .flex_1()
                .min_w(px(0.0))
                .label(label)
                .default_value(initial)
                .readonly(move || draft.with(|d| d.saving))
                .on_change(move |t: &str| {
                    draft.update(|d| {
                        d.values.insert(field, t.to_owned());
                        d.saved = false;
                    })
                })
                .into_element()
        }))
}

/// `;` 分隔串 → 标签列表（草稿里流派仍是 "a; b" 形式，写回格式不变）。
pub(crate) fn split_genres(raw: &str) -> Vec<String> {
    raw.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
