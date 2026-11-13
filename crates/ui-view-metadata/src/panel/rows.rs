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
            let t = nsh_base::theme();
            let (input_c, ring) = (t.input, t.ring);
            // Windows 上宿主编辑器（无窗口 RichEdit）没有滚轮通路：上游
            // `nsh-platform-windows` 的 `text_input::wheel` 存在但没有调用
            // 方，限高视口换来的内部滚动只能靠键盘走（见 upstream.md）。
            // 改成输入框 h(auto) 随内容长高、外面套 max_height 的 scroll
            // 滚整个元素——滚轮 / 滚动条 / 拖滚动都按普通滚动区来，滚到界
            // 自然落回面板。字段外观（边框、聚焦 ring）搬到 scroll 上，
            // 输入框自身不画 chrome。编辑器视口随全文高、不做内部滚动，
            // 光标滚出视口后续输不跟随（原限高方案滚轮本来就不通）。
            scroll()
                .flex_1()
                .min_w(px(0.0))
                .min_h(px(min_height))
                .max_h(px(max_height))
                .rounded(px(RADIUS_MD))
                .border_1()
                .border_color(input_c)
                .shadow_xs()
                .bg(t.card)
                .focus_within(move |st| {
                    st.border_color(ring)
                        .ring(px(3.0))
                        .ring_color(ring.opacity(0.5))
                })
                .child(
                    nsh_base::input()
                        .w(relative(1.0))
                        .min_h(px(min_height - 2.0))
                        .px(px(8.0))
                        .py(px(10.0))
                        .multiline(true)
                        .scrollbar(false)
                        .text_size(px(14.0))
                        .text_color(fg())
                        .label(label)
                        .default_value(initial)
                        .readonly(move || draft.with(|d| d.saving))
                        .on_change(move |t: &str| {
                            draft.update(|d| {
                                d.values.insert(field, t.to_owned());
                                d.saved = false;
                            })
                        }),
                )
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
