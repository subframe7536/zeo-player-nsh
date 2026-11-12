use super::*;

/// 新建 / 编辑歌单的弹窗（`store().editor` 有草稿时显示）。
pub fn editor_dialog() -> AnyElement {
    let s = store();
    let draft = signal(s.editor.get_untracked().unwrap_or(Draft {
        id: None,
        name: String::new(),
        rule: None,
        add: Vec::new(),
    }));
    let close = move || s.editor.set(None);
    let save = move || {
        store::save_draft(draft.get_untracked());
    };
    let smart = untracked(|| draft.with(|d| d.rule.is_some()));
    let heading = untracked(|| match (draft.with(|d| d.id.is_some()), smart) {
        (false, false) => "新建歌单",
        (false, true) => "新建智能歌单",
        (true, false) => "重命名歌单",
        (true, true) => "编辑智能歌单",
    });
    let name_input = input_field(false)
        .w_full()
        .label("歌单名称")
        .auto_focus(true)
        .default_value(untracked(|| draft.with(|d| d.name.clone())))
        .placeholder("歌单名称")
        .on_change(move |t: &str| draft.update(|d| d.name = t.to_owned()))
        // 只在回车 / 「保存」时提交：失焦不能保存——名称框自动聚焦，点
        // 智能规则里任何一个输入框都会让它失焦，保存会把弹窗直接关掉。
        .on_submit(move |_: &str| save());
    let description = if smart {
        Some("按条件从曲库自动聚合，曲库变化时跟着更新。")
    } else {
        None
    };
    let content = dialog_content(if smart { 640.0 } else { 425.0 })
        .child(dialog_header(heading, description))
        .child(name_input)
        .child(smart.then(|| rule_editor(draft)))
        .child(
            dialog_footer()
                .child(text_button(
                    Variant::Outline,
                    Size::Default,
                    None,
                    "取消",
                    close,
                ))
                .child(text_button(
                    Variant::Default,
                    Size::Default,
                    None,
                    if heading.starts_with("新建") {
                        "创建"
                    } else {
                        "保存"
                    },
                    save,
                )),
        );
    modal(content, close)
}

/// 智能规则编辑：全部 / 任一、条件行、添加条件、排序、数量上限、实时匹配数。
pub(crate) fn rule_editor(draft: Signal<Draft>) -> View {
    let s = store();
    let rule = move |f: &dyn Fn(&Rule) -> String| {
        draft.with(|d| d.rule.as_ref().map(f).unwrap_or_default())
    };
    let change = move |f: Box<dyn FnOnce(&mut Rule)>| {
        draft.update(|d| {
            if let Some(r) = d.rule.as_mut() {
                f(r);
            }
        })
    };
    let count =
        computed(move || draft.with(|d| d.rule.as_ref().map(|r| r.conditions.len()).unwrap_or(0)));
    let matched = computed(move || {
        let tracks = s.tracks.get();
        draft.with(|d| {
            d.rule
                .as_ref()
                .map(|r| r.select(&tracks, unix_now()).len())
                .unwrap_or(0)
        })
    });
    let match_all = move || draft.with(|d| d.rule.as_ref().is_none_or(|r| r.match_all));
    let chip = move |label: &'static str, all: bool| {
        tab_trigger(
            move || match_all() == all,
            None,
            label,
            move || change(Box::new(move |r| r.match_all = all)),
        )
    };
    let add = text_button(
        Variant::Ghost,
        Size::Sm,
        Some(Icon::Plus),
        "添加条件",
        move || change(Box::new(|r| r.conditions.push(Condition::default()))),
    );
    let mode = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text("符合以下")
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(muted_fg()),
        )
        .child(
            tab_list()
                .child(chip("全部", true))
                .child(chip("任一", false)),
        )
        .child(
            text("条件")
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(muted_fg()),
        )
        .child(add)
        .child(view().flex_1())
        .child(
            text(move || format!("匹配 {} 首", matched.get()))
                .text_size(px(14.0))
                .line_height(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .tabular_nums()
                .text_color(move || primary())
                .transition_colors()
                .duration(300.0),
        );
    // 行数变化（添加 / 删除）时整组重建；改字段、条件、值只更新所在行。
    let rows = dynamic(move || {
        (0..count.get())
            .map(|i| condition_row(draft, i).into_element())
            .collect::<Vec<_>>()
    });
    let sort = select(
        move || rule(&|r| r.sort.label().to_owned()),
        "排序",
        Sort::ALL
            .iter()
            .map(|&o| SelectOption::new(o.label(), move || change(Box::new(move |r| r.sort = o))))
            .collect(),
        170.0,
    );
    let limit = input_field(true)
        .w(px(80.0))
        .label("数量上限")
        .keyboard_type(KeyboardType::Number)
        .default_value(untracked(|| {
            rule(&|r| r.limit.map(|n| n.to_string()).unwrap_or_default())
        }))
        .placeholder("不限")
        .on_change(move |t: &str| {
            let limit = t.trim().parse::<usize>().ok().filter(|n| *n > 0);
            change(Box::new(move |r| r.limit = limit));
        });
    let footer = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text("排序")
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(muted_fg()),
        )
        .child(sort)
        .child(view().w(px(12.0)))
        .child(
            text("最多")
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(muted_fg()),
        )
        .child(limit)
        .child(
            text("首")
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(muted_fg()),
        );
    view()
        .gap(px(12.0))
        .flex_shrink(1.0)
        .min_h(px(0.0))
        .child(mode)
        .child(
            scroll()
                .flex_shrink(1.0)
                .max_h(px(300.0))
                .child(view().gap(px(8.0)).child(rows)),
        )
        .child(view().h(px(1.0)).bg(border()))
        .child(footer)
}

/// 单个条件的编辑器行（字段、比较方式、值、单位、删除/清除）：智能歌单与文件夹筛选共用。
pub fn condition_editor(
    get: impl Fn() -> Condition + Copy + 'static,
    change: impl Fn(Box<dyn FnOnce(&mut Condition)>) + Copy + 'static,
    remove: Option<Rc<dyn Fn()>>,
) -> View {
    let field = computed(move || get().field);
    let field_options = Field::ALL
        .iter()
        .enumerate()
        .map(|(i, &f)| {
            let heading = (i == 0 || Field::ALL[i - 1].group() != f.group()).then(|| f.group());
            let mut opt = SelectOption::new(f.label(), move || {
                change(Box::new(move |c| {
                    // 标签只能从列表里选，换过去时把自由文本的值也清掉。
                    if c.field.kind() != f.kind() || f == Field::Tag {
                        c.value.clear();
                    }
                    c.field = f;
                    if !f.ops().contains(&c.op) {
                        c.op = f.ops()[0];
                    }
                }))
            });
            if let Some(h) = heading {
                opt = opt.with_heading(h);
            }
            opt
        })
        .collect();
    let field_select = select(
        move || get().field.label().to_owned(),
        "字段",
        field_options,
        120.0,
    );
    // 比较方式随字段变：字段换了整个下拉重建。
    let op_select = dynamic(move || {
        let options = field
            .get()
            .ops()
            .iter()
            .map(|&o: &Op| {
                SelectOption::new(o.label(), move || change(Box::new(move |c| c.op = o)))
            })
            .collect();
        select(move || get().op.label().to_owned(), "条件", options, 110.0)
    });
    // 值：标签是从曲库现有标签里选，其余是输入框；换了字段（可能清空了值）
    // 时重建，打字时不重建。
    let value_input = dynamic(move || {
        let f = field.get();
        if f == Field::Tag {
            return tag_select(
                move || get().value,
                move |v| change(Box::new(move |c| c.value = v)),
            );
        }
        let placeholder = match f.kind() {
            FieldType::Text if f == Field::Format => "mp3 / flac / wav",
            FieldType::Text => "",
            FieldType::Number if f == Field::Duration => "3:30",
            FieldType::Number => "数值",
            FieldType::Date => "天数",
        };
        input_field(true)
            .flex_1()
            .min_w(px(80.0))
            .label("值")
            .value(move || get().value)
            .placeholder(placeholder)
            .on_change(move |t: &str| {
                let t = t.to_owned();
                change(Box::new(move |c| c.value = t));
            })
            .into_element()
    });
    let unit = move || {
        let c = get();
        match (c.field.kind(), c.field.unit()) {
            (FieldType::Date, _) => "天内".to_owned(),
            (_, unit) => unit.to_owned(),
        }
    };
    let invalid = move || {
        let c = get();
        !c.value.trim().is_empty() && !c.is_complete()
    };
    let mut row = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        // 行比输入框高 8 pt：聚焦环（3 pt）才不会被滚动区裁掉。
        .p(px(4.0));
    if let Some(remove) = remove {
        row = row.child(icon_button(
            || Icon::Close,
            16.0,
            "删除条件",
            muted_fg,
            move || remove(),
        ));
    }
    row.child(field_select)
        .child(op_select)
        .child(
            view()
                .flex_1()
                .max_w(px(320.0))
                .flex_row()
                .items_center()
                .child(value_input),
        )
        .child(
            text(move || {
                if invalid() {
                    "无效".to_owned()
                } else {
                    unit()
                }
            })
            .w(px(70.0))
            .number_of_lines(1)
            .text_size(px(12.0))
            .line_height(px(12.0))
            .text_color(move || {
                if invalid() {
                    nsh_base::theme().destructive
                } else {
                    faint()
                }
            }),
        )
}

/// 一行条件：字段、比较方式、值、单位、删除。
pub(crate) fn condition_row(draft: Signal<Draft>, index: usize) -> View {
    let get = move || {
        draft.with(|d| {
            d.rule
                .as_ref()
                .and_then(|r| r.conditions.get(index).cloned())
                .unwrap_or_default()
        })
    };
    let change = move |f: Box<dyn FnOnce(&mut Condition)>| {
        draft.update(|d| {
            if let Some(c) = d.rule.as_mut().and_then(|r| r.conditions.get_mut(index)) {
                f(c);
            }
        })
    };
    let remove = Rc::new(move || {
        draft.update(|d| {
            if let Some(r) = d.rule.as_mut() {
                if r.conditions.len() > 1 {
                    if index < r.conditions.len() {
                        r.conditions.remove(index);
                    }
                } else if let Some(c) = r.conditions.first_mut() {
                    *c = Condition::default();
                }
            }
        });
    });
    condition_editor(get, change, Some(remove))
}

/// 标签条件的值：从曲库现有标签里选（按小写名排序，忽略大小写去重）。
/// nsh-base `select`：可搜索（标签多了以后），列表虚拟化。
pub(crate) fn tag_select(
    current: impl Fn() -> String + Copy + 'static,
    pick: impl Fn(String) + Copy + 'static,
) -> AnyElement {
    let s = store();
    let pulse = signal(0.0f32);
    // 弹性宽加在 select 元素上（根是列容器；触发器里的 flex_1 会在纵轴
    // 把它压没，只保 min_w 由列拉伸铺满）。宽度脉冲走根的 min_w，基准
    // 宽由触发器记（cover_realign_flex）。
    let (min_w, layout) = cover_realign_flex(pulse);
    let trigger = select_trigger(current, || None, "值", 0.0, "选择标签").on_layout(layout);
    let no_tags = move || !s.tracks.get().iter().any(|t| !t.genres.is_empty());
    nsh_base::select()
        .flex_1()
        .min_w(min_w)
        .items(move || {
            let tracks = s.tracks.get();
            // 键用小写：匹配不分大小写，只差大小写的标签算同一个。
            let mut tags: BTreeMap<String, String> = BTreeMap::new();
            for g in tracks.iter().flat_map(|t| &t.genres) {
                tags.entry(g.to_lowercase()).or_insert_with(|| g.clone());
            }
            tags.into_values()
                .map(|tag| nsh_base::SelectItem::new(tag.clone(), tag))
                .collect::<Vec<_>>()
        })
        .value(move || {
            let cur = current();
            nsh_base::SelectValue::Controlled((!cur.is_empty()).then(|| cur.into()))
        })
        .searchable(true)
        .search_placeholder("搜索标签")
        .search_accessibility_label("搜索标签")
        .accessibility_label("值")
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(4.0))
        .margin(px(8.0))
        .on_open(move || cover_realign(pulse))
        .on_change(move |v: &Option<SharedString>| {
            if let Some(v) = v {
                pick(v.to_string())
            }
        })
        .popup_style(|st| {
            st.min_w(px(200.0))
                .p(px(4.0))
                .rounded(px(10.0))
                .overflow_hidden()
                .border_1()
                .border_color(border())
                .bg(nsh_base::theme().popover)
                .shadow_lg()
        })
        .item_class("focus:bg-hover")
        .item_style(|st| {
            st.px(px(10.0))
                .py(px(6.0))
                .rounded(px(6.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(fg())
        })
        .search_style(|st| {
            st.h(px(32.0))
                .px(px(10.0))
                .text_size(px(13.0))
                .text_color(fg())
        })
        .empty(
            view().h(px(48.0)).items_center().justify_center().child(
                text(move || {
                    if no_tags() {
                        "曲库里还没有标签"
                    } else {
                        "没有匹配的标签"
                    }
                })
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
            ),
        )
        .trigger(trigger)
        .into_element()
}
