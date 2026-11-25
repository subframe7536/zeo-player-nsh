use super::*;

/// 一行 `标签 + 定宽输入框`（年份 / ReplayGain 类，控件列从同一 x 起）。
pub(crate) fn narrow_row(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
    width: f32,
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
                return loading_input(true, Some(width)).into_element();
            }
            number_input(field.label(), field, draft, full, width).into_element()
        }))
}

/// 一行 `标签 + 号 / 总数`：两个等宽输入，`/` 居中，从控件列起点排。
pub(crate) fn pair_row(
    label: &'static str,
    a_label: &'static str,
    a: TagField,
    b_label: &'static str,
    b: TagField,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell(label))
        .child(dynamic(move || {
            let is_ready = untracked(|| {
                draft.with(|d| d.values.contains_key(&a) || d.values.contains_key(&b))
            }) || full.get().is_some();
            if !is_ready {
                return view()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .child(loading_input(true, Some(56.0)))
                    .child(
                        text("/")
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(faint()),
                    )
                    .child(loading_input(true, Some(56.0)))
                    .into_element();
            }
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(number_input(a_label, a, draft, full, 56.0))
                .child(
                    text("/")
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(faint()),
                )
                .child(number_input(b_label, b, draft, full, 56.0))
                .into_element()
        }))
}

/// 窄数字输入（行内并排用）。
pub(crate) fn number_input(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
    width: f32,
) -> impl IntoElement {
    let initial = untracked(|| {
        draft
            .with(|d| d.values.get(&field).cloned())
            .or_else(|| full.with(|f| f.as_ref().and_then(|(m, _, _)| m.get(&field).cloned())))
            .unwrap_or_default()
    });
    text_input(true)
        .w(px(width))
        .label(label)
        .default_value(initial)
        .readonly(move || draft.with(|d| d.saving))
        .on_change(move |t: &str| {
            draft.update(|d| {
                d.values.insert(field, t.to_owned());
                d.saved = false;
            })
        })
}

/// 评分：5 颗星；点当前星清空。加载中显示灰星且不可交互。
pub(crate) fn rating_row(draft: Signal<SingleDraft>, full: Signal<Option<FullRow>>) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell("评分"))
        .child(dynamic(move || {
            let is_ready = untracked(|| draft.with(|d| d.values.contains_key(&TagField::Rating)))
                || full.get().is_some();
            if !is_ready {
                return view()
                    .flex_row()
                    .gap(px(2.0))
                    .children((1..=5).map(|_| {
                        view()
                            .size(px(22.0))
                            .items_center()
                            .justify_center()
                            .child(
                                text("☆")
                                    .text_size(px(16.0))
                                    .line_height(px(16.0))
                                    .text_color(faint()),
                            )
                            .into_element()
                    }))
                    .into_element();
            }
            let star = |n: u32| {
                let filled = move || {
                    draft.with(|d| {
                        d.values
                            .get(&TagField::Rating)
                            .and_then(|v| v.parse::<u32>().ok())
                            .unwrap_or(0)
                            >= n
                    })
                };
                view()
                    .size(px(22.0))
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .role(AccessibilityRole::Button)
                    .label(format!("评分 {n} 星"))
                    .cursor(Cursor::Pointer)
                    .hover(|st| st.bg(hover()))
                    .on_press(move || {
                        draft.update(|d| {
                            let current =
                                d.values.get(&TagField::Rating).cloned().unwrap_or_default();
                            d.values.insert(
                                TagField::Rating,
                                if current == n.to_string() {
                                    String::new()
                                } else {
                                    n.to_string()
                                },
                            );
                            d.saved = false;
                        });
                    })
                    .child(
                        text(move || if filled() { "★" } else { "☆" })
                            .text_size(px(16.0))
                            .line_height(px(16.0))
                            .text_color(move || if filled() { primary() } else { faint() }),
                    )
            };
            view()
                .flex_row()
                .gap(px(2.0))
                .children((1..=5).map(|n| star(n).into_element()))
                .into_element()
        }))
}

/// 多行输入（注释 / 歌词）：限制最大高度（歌词 260pt，注释 120pt）；未就绪时画同款外观占位。
/// 输入框非受控（`default_value` 只在挂载时生效）：外部要整段换文本（导入歌词）时
/// 给 `remount`，它一变输入框按草稿里的新值重新挂载。
pub(crate) fn multi_row(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
    min_height: f32,
    max_height: f32,
    remount: Option<Signal<u32>>,
) -> View {
    view()
        .flex_row()
        .items_start()
        .gap(px(8.0))
        .child(view().w(px(72.0)).pt(px(8.0)).child(label_cell(label)))
        .child(dynamic(move || {
            // 递增的 key 逼它换节点：同 key 的非受控输入框会被复用、忽略新的
            // `default_value`。
            let generation = remount.map_or(0, |r| r.get());
            let is_ready =
                untracked(|| draft.with(|d| d.values.contains_key(&field))) || full.get().is_some();
            if !is_ready {
                return view()
                    .flex_1()
                    .min_w(px(0.0))
                    .min_h(px(min_height))
                    .max_h(px(max_height))
                    .px(px(12.0))
                    .py(px(10.0))
                    .rounded(px(RADIUS_MD))
                    .border_1()
                    .border_color(nsh_base::theme().input)
                    .bg(nsh_base::theme().card)
                    .overflow_hidden()
                    .child(
                        text("加载中...")
                            .number_of_lines(1)
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(faint()),
                    )
                    .into_element();
            }
            let initial = untracked(|| {
                draft
                    .with(|d| d.values.get(&field).cloned())
                    .or_else(|| {
                        full.with(|f| f.as_ref().and_then(|(m, _, _)| m.get(&field).cloned()))
                    })
                    .unwrap_or_default()
            });
            multi_input(
                label, field, draft, initial, min_height, max_height, generation,
            )
        }))
}

/// 多行输入本体：`text_area` 的 `Grow` 随文生长、超过 `max_height`
/// 锁成固定高恢复内部原生滚动（机制见 `components::input`）。
fn multi_input(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    initial: String,
    min_height: f32,
    max_height: f32,
    generation: u32,
) -> AnyElement {
    text_area(TextAreaHeight::Grow {
        min: min_height,
        max: max_height,
    })
    .key(format!("{field:?}-{generation}"))
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
}

/// 封面行：缩略图 + 更换 / 移除（写内嵌图片）。
pub(crate) fn cover_row(draft: Signal<SingleDraft>, full: Signal<Option<FullRow>>) -> View {
    let src = move || {
        let d = draft.get();
        match &d.cover {
            Some(CoverEdit::Replace { .. }) => d.cover_src.clone().map(SharedString::from),
            Some(CoverEdit::Remove) => None,
            None => store::cover_for_always(&d.path, 80.0),
        }
    };
    let pick = move || {
        let is_ready = untracked(|| draft.with(|d| !d.values.is_empty())) || full.get().is_some();
        if !is_ready {
            return;
        }
        spawn(async move {
            let picked = blocking(|| {
                rfd::FileDialog::new()
                    .set_title("选择封面图片")
                    .add_filter("图片", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
                    .pick_file()
            })
            .await
            .ok()
            .flatten();
            let Some(file) = picked else { return };
            let outcome = write::cover_mime(&file)
                .ok_or_else(|| "不支持的图片格式（PNG / JPEG / WebP / BMP / GIF）".to_owned())
                .and_then(|mime| {
                    std::fs::read(&file)
                        .map(|data| (data, mime))
                        .map_err(|e| format!("读取图片失败: {e}"))
                });
            match outcome {
                Ok((data, mime)) => draft.update(|d| {
                    d.cover = Some(CoverEdit::Replace { data, mime });
                    d.cover_src = Some(file.to_string_lossy().into_owned());
                    d.saved = false;
                    d.error = None;
                }),
                Err(error) => draft.update(|d| d.error = Some(error)),
            }
        });
    };
    let busy_or_loading = move || {
        (draft.with(|d| d.values.is_empty()) && full.get().is_none()) || draft.with(|d| d.saving)
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .child(label_cell("封面"))
        .child(cover(src, Some(80.0), 8.0))
        .child(
            view()
                .gap(px(6.0))
                .child(
                    text_button(Variant::Outline, Size::Sm, None, "更换图片…", pick)
                        .disabled(busy_or_loading),
                )
                .child(
                    text_button(
                        Variant::Outline,
                        Size::Sm,
                        None,
                        "移除封面",
                        move || {
                            draft.update(|d| {
                                d.cover = Some(CoverEdit::Remove);
                                d.cover_src = None;
                                d.saved = false;
                            })
                        },
                    )
                    .disabled(busy_or_loading),
                ),
        )
}
