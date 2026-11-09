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
    input_field(true)
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
pub(crate) fn multi_row(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
    min_height: f32,
    max_height: f32,
) -> View {
    view()
        .flex_row()
        .items_start()
        .gap(px(8.0))
        .child(view().w(px(72.0)).pt(px(8.0)).child(label_cell(label)))
        .child(dynamic(move || {
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
            let content_h = signal(0.0f32);
            input_field(false)
                .flex_1()
                .min_w(px(0.0))
                // 文本超过上限时锁成固定高：h(auto) 的输入框 intrinsic 高度
                // 随内容长，max_h 只裁元素框，宿主编辑器仍按内容高开视口——
                // 内部永远没有可滚范围（overflow_hidden 再裁一下只是看不见
                // 溢出）。固定到 max_height 后编辑器视口被框住，原生滚动
                // 恢复；未超限时仍 auto 随文生长。22 = py(10)×2 + border_1×2。
                .h(move || {
                    if content_h.get() + 22.0 > max_height {
                        Length::from(px(max_height))
                    } else {
                        auto()
                    }
                })
                .min_h(px(min_height))
                .max_h(px(max_height))
                .py(px(10.0))
                .multiline(true)
                .label(label)
                .default_value(initial)
                .readonly(move || draft.with(|d| d.saving))
                .on_content_size_change(move |s: &nsh_core::Size<nsh_core::Points>| {
                    content_h.set(s.height.0)
                })
                .on_change(move |t: &str| {
                    draft.update(|d| {
                        d.values.insert(field, t.to_owned());
                        d.saved = false;
                    })
                })
                .into_element()
        }))
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
                    text_button(Variant::Outline, Size::Xs, None, "更换图片…", pick)
                        .disabled(busy_or_loading),
                )
                .child(
                    text_button(
                        Variant::Outline,
                        Size::Xs,
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
