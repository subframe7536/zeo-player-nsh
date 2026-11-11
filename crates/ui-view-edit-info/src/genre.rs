use super::*;

/// 流派：标签输入器 + 下拉。chips 在 combobox 的 `prefix`，宽度随内容、
/// 顶满整行后内部折行；combobox 根行 flex_wrap，输入框紧跟最后一个
/// chip，剩余宽度不够时折到下一行占满。输入即过滤曲库已有流派
/// （`s.tracks` 去重），点选 / 回车选中，回车对第一项生效——查询不是
/// 已有流派时第一项是「添加“…”」自由新增。选过的项再选会取消勾选
/// （等同删 chip）。去重忽略大小写，草稿存 `;` 合并串。
pub(crate) fn genre_row(draft: Signal<TagDraft>) -> View {
    let s = store();
    let t = nsh_base::theme();
    let (input_c, ring, popover_c, card_c) = (t.input, t.ring, t.popover, t.card);
    let gate = store::menu_control();
    let query = signal(String::new());
    let genres = move || {
        draft.with(|d| {
            split_genres(
                d.values
                    .get(&TagField::Genre)
                    .map(String::as_str)
                    .unwrap_or_default(),
            )
        })
    };
    let set_genres = move |list: Vec<String>| {
        draft.update(|d| {
            d.values.insert(TagField::Genre, list.join("; "));
        })
    };
    // 下拉项：曲库已有流派（排序去重）；查询串不是已有流派时最上面
    // 塞一项「添加“{查询}”」，回车 / 点选即自由新增。
    let items = move || {
        let q = query.get();
        let q = q.trim();
        let mut known: Vec<String> = Vec::new();
        s.tracks.with(|tracks| {
            for g in tracks.iter().flat_map(|t| &t.genres) {
                if !known.iter().any(|k| k == g) {
                    known.push(g.clone());
                }
            }
        });
        known.sort();
        let mut items = Vec::with_capacity(known.len() + 1);
        if !q.is_empty() && !known.iter().any(|g| g.eq_ignore_ascii_case(q)) {
            items.push(nsh_base::SelectItem::new(
                q.to_owned(),
                format!("添加“{q}”"),
            ));
        }
        items.extend(
            known
                .into_iter()
                .map(|g| nsh_base::SelectItem::new(g.clone(), g)),
        );
        items
    };
    let chips = move || {
        view()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(4.0))
            .child(dynamic(move || {
                genres()
                    .into_iter()
                    .map(|g| genre_chip(draft, g).into_element())
                    .collect::<Vec<_>>()
            }))
    };
    view()
        .flex_row()
        .items_start()
        .gap(px(12.0))
        .child(
            view().w(px(56.0)).pt(px(10.0)).child(
                text("标签")
                    .w(px(56.0))
                    .text_size(px(13.0))
                    .line_height(px(13.0))
                    .text_color(muted_fg()),
            ),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(4.0))
                .child(
                    nsh_base::combobox()
                        .w(percentage(1.0))
                        .min_h(px(36.0))
                        // 根行折行：chips 块顶满整行时输入框折到下一行；行内居中：
                        // chips 撑高后输入框不贴顶。右内边距给绝对定位的 chevron
                        // 留位（chevron 不参与折行，不会被挤掉）。
                        .flex_wrap()
                        .items_center()
                        .pl(px(8.0))
                        .pr(px(30.0))
                        .py(px(5.0))
                        .gap(px(6.0))
                        .rounded(px(RADIUS_MD))
                        .border_1()
                        .border_color(input_c)
                        .shadow_xs()
                        .bg(card_c)
                        .focus_within(move |st| {
                            st.border_color(ring)
                                .ring(px(3.0))
                                .ring_color(ring.opacity(0.5))
                        })
                        .items(items)
                        .multiple(true)
                        .value(move || {
                            Some(
                                genres()
                                    .into_iter()
                                    .map(SharedString::from)
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .query(move || Some(SharedString::from(query.get())))
                        .on_query_change(move |q: &SharedString| query.set(q.to_string()))
                        .open(gate.open())
                        .on_open_change(gate.combobox_change())
                        .on_change(move |v: &Vec<SharedString>| {
                            let mut list: Vec<String> = Vec::new();
                            for x in v {
                                let g = x.trim().to_owned();
                                if !g.is_empty() && !list.iter().any(|k| k.eq_ignore_ascii_case(&g))
                                {
                                    list.push(g);
                                }
                            }
                            set_genres(list);
                            query.set(String::new());
                        })
                        .placeholder(Some(SharedString::from("回车添加")))
                        .accessibility_label(Some(SharedString::from("标签")))
                        .disabled(move || draft.with(|d| d.saving))
                        .placement(Placement::Bottom)
                        .align(Align::Start)
                        .offset(px(4.0))
                        .margin(px(8.0))
                        .popup_style(move |st| {
                            st.p(px(4.0))
                                .rounded(px(10.0))
                                .overflow_hidden()
                                .border_1()
                                .border_color(border())
                                .bg(popover_c)
                                .shadow_lg()
                        })
                        // 行高 = 复选框 16 + py(6)×2 = 28；224 恰好 8 行，
                        // 不会在弹层底边把最后一行切成半行。
                        .list_style(|st| st.max_h(px(224.0)))
                        .item_class("focus:bg-hover")
                        .item_style(|st| st.px(px(8.0)).py(px(6.0)).rounded(px(6.0)))
                        .render_item(move |entry: &nsh_base::SelectEntry| genre_item(draft, entry))
                        .input_style(move |st| {
                            // 紧跟最后一个 chip 并吃掉该行剩余宽度；放不下 72
                            // （够显示“回车添加”占位）时随根行折到下一行占满。
                            st.h(px(24.0))
                                .flex_1()
                                .min_w(px(72.0))
                                .text_size(px(13.0))
                                .text_color(fg())
                        })
                        .prefix(when(move || !genres().is_empty(), move || chips()))
                        .trigger_button(
                            view()
                                .absolute()
                                .top(px(5.0))
                                .right(px(6.0))
                                .h(px(24.0))
                                .items_center()
                                .justify_center()
                                .child(trigger_chevron()),
                        )
                        .empty(
                            text("没有可添加的标签")
                                .text_size(px(12.0))
                                .line_height(px(12.0))
                                .text_color(faint()),
                        ),
                )
                .child(dynamic(move || {
                    let formatted = genres().join("; ");
                    text(format!("流派：{formatted}"))
                        .text_size(px(11.0))
                        .line_height(px(15.0))
                        .text_color(muted_fg())
                        .into_element()
                })),
        )
}

/// 下拉行：左侧复选框（选中打勾）+ 流派名。「添加“…”」项不在已选集
/// 里，自然显示空框。
pub(crate) fn genre_item(draft: Signal<TagDraft>, entry: &nsh_base::SelectEntry) -> View {
    let value = entry.value.to_string();
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            view()
                .w(px(16.0))
                .h(px(16.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(border())
                .items_center()
                .justify_center()
                .child(when(
                    move || {
                        draft.with(|d| {
                            split_genres(
                                d.values
                                    .get(&TagField::Genre)
                                    .map(String::as_str)
                                    .unwrap_or_default(),
                            )
                            .iter()
                            .any(|g| *g == value)
                        })
                    },
                    || icon(Icon::Check, 10.0, fg),
                )),
        )
        .child(
            text(entry.title.to_string())
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(fg()),
        )
}

/// 一个流派 chip：文字 + × 按钮。
pub(crate) fn genre_chip(draft: Signal<TagDraft>, name: String) -> View {
    let remove = {
        let name = name.clone();
        move || {
            if draft.with(|d| d.saving) {
                return;
            }
            draft.update(|d| {
                let mut list = split_genres(
                    d.values
                        .get(&TagField::Genre)
                        .map(String::as_str)
                        .unwrap_or_default(),
                );
                list.retain(|x| *x != name);
                d.values.insert(TagField::Genre, list.join("; "));
            });
        }
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .h(px(24.0))
        .pl(px(8.0))
        .pr(px(4.0))
        .rounded(px(6.0))
        .bg(muted())
        .child(
            text(name)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(fg()),
        )
        .child(
            view()
                .h(px(16.0))
                .w(px(16.0))
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .hover(|st| st.bg(hover()))
                .role(AccessibilityRole::Button)
                .label("移除流派")
                .on_press(move || remove())
                .child(icon(Icon::Close, 10.0, muted_fg)),
        )
}

// --- 封面 ----------------------------------------------------------------------
