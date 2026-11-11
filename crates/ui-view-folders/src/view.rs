use super::*;

/// 侧栏文件夹树列表。
/// 侧栏文件夹树列表。
pub fn tree_sidebar_list() -> View {
    let ui = folder_ui();
    let s = store();
    let all_rows = ui.all_rows;
    let here = ui.here;
    let ctx = ui.ctx.clone();
    let ops = ui.ops.clone();
    let select = ui.select.clone();
    let toggle = ui.toggle.clone();
    let scroll_sig = ui.scroll;
    let cell_select = select.clone();
    let cell_toggle = toggle.clone();
    let tree_list = virtual_list(computed(move || all_rows.with(|r| r.len())), move |at| {
        let row = computed(move || all_rows.with(|r| r.get(at.index()).cloned()));
        tree_cell(
            row,
            here,
            ctx.clone(),
            ops.clone(),
            cell_select.clone(),
            cell_toggle.clone(),
        )
    })
    .keys(move || {
        all_rows
            .get()
            .iter()
            .map(|r| r.path.clone())
            .collect::<Vec<_>>()
    })
    .estimated_item_size(px(TREE_ROW))
    .content_padding_x(px(4.0))
    .content_padding_bottom(px(BAR_SPACE))
    .element_ref(&ui.list_ref)
    .on_scroll(move |e: &ScrollEvent| scroll_sig.set(e.offset.y.0))
    .empty(empty_state(Icon::Folder, "没有文件夹", ""))
    .flex_1();

    let on_drop = {
        let select = select.clone();
        let message = s.folder_message;
        move |event: &FileDropEvent| {
            let mut dir = None;
            let mut added = false;
            let notes: Vec<String> = event
                .uris
                .iter()
                .map(|uri| {
                    let dropped = add_dropped(&url_to_path(uri));
                    added |= dropped.added;
                    if dir.is_none() {
                        dir = dropped.dir;
                    }
                    dropped.note
                })
                .collect();
            if added {
                scan();
            }
            if let Some(dir) = dir {
                select(&dir);
            }
            show_message(message, notes.join("；"));
        }
    };

    view()
        .flex_1()
        .min_h(px(0.0))
        .on_file_drop(on_drop)
        .child(tree_list)
}

/// 文件夹增删改弹窗（新建 / 重命名输入名，删除先确认）。
pub fn op_dialog() -> impl IntoElement {
    let s = store();
    let ui = folder_ui();
    let ops = ui.ops;
    when(
        move || s.folder_op.with(|o| o.is_some()),
        move || {
            let Some(op) = s.folder_op.get() else {
                return view().into_element();
            };
            let ops_cloned = ops.clone();
            let close = move || s.folder_op.set(None);
            match op {
                FileOp::DeleteFolder { path } => delete_folder_dialog(ops_cloned, path, close),
                FileOp::NewFolder { .. } | FileOp::Rename { .. } => {
                    name_dialog(ops_cloned, op, close)
                }
            }
        },
    )
}

/// 全局拖拽浮层：跟着指针的小条。
pub fn drag_ghost() -> impl IntoElement {
    let ui = folder_ui();
    let active = ui.ctx.active;
    let anchor = ui.ctx.anchor;
    let gx = ui.ctx.gx.clone();
    let gy = ui.ctx.gy.clone();
    when(
        move || active.with(|a| a.is_some()),
        move || {
            let name =
                move || active.with(|a| a.as_ref().map(|d| d.name.clone()).unwrap_or_default());
            let dir = move || active.with(|a| a.as_ref().is_some_and(|d| d.dir));
            view()
                .absolute()
                .top(move || px(anchor.get().1))
                .left(move || px(anchor.get().0))
                .bind_translate_x(&gx)
                .bind_translate_y(&gy)
                .pointer_events(PointerEvents::None)
                .h(px(28.0))
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .pl(px(8.0))
                .pr(px(12.0))
                .rounded(px(8.0))
                .bg(bar())
                .shadow_lg()
                .child(icon_dyn(
                    move || {
                        if dir() { Icon::Folder } else { Icon::Note }
                    },
                    13.0,
                    primary,
                ))
                .child(
                    text(name)
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(fg()),
                )
        },
    )
}

pub(crate) fn rescan_button() -> nsh_base::ButtonElement {
    let s = store();
    icon_button(|| Icon::Refresh, 16.0, "重新扫描", muted_fg, scan)
        .disabled(move || s.library.with(|l| l.progress.scanning))
}

/// 树里的一行：缩进 + 折叠箭头（叶子为占位）+ 文件夹图标 + 名字 + 曲目
/// 数。行是整宽的可点区（选中），箭头单独可点（折叠 / 展开）；行也能拖
/// 起这个文件夹，或作为别的拖动落下的目标（`over` 命中时高亮）；右键
/// 是增删改菜单。
pub(crate) fn tree_cell(
    row: Memo<Option<TreeRow>>,
    here: Memo<Place>,
    ctx: Rc<DragCtx>,
    ops: Rc<Ops>,
    select: Rc<dyn Fn(&str)>,
    toggle: Rc<dyn Fn(&str, bool)>,
) -> AnyElement {
    let name = move || row.with(|r| r.as_ref().map(|r| r.name.clone()).unwrap_or_default());
    let depth = move || row.with(|r| r.as_ref().map(|r| r.depth).unwrap_or(0));
    let has_children = move || row.with(|r| r.as_ref().is_some_and(|r| r.has_children));
    let open = move || row.with(|r| r.as_ref().is_some_and(|r| r.open));
    let count = move || row.with(|r| r.as_ref().map(|r| r.count).unwrap_or(0));
    let nav = global::<zeo_ui_store::Nav>();
    let selected = move || {
        nav.get() == zeo_ui_store::Nav::Folders
            && row.with(|r| {
                let Some(r) = r.as_ref() else { return false };
                here.with(|p| p.path.as_deref() == Some(r.path.as_str()))
            })
    };
    let over = ctx.over;
    let drop_to = move || {
        row.with(|r| {
            r.as_ref()
                .is_some_and(|r| over.with(|o| o.as_deref() == Some(r.path.as_str())))
        })
    };
    let cell = view()
        .w_full()
        .h(px(TREE_ROW))
        .pl(move || px(6.0 + depth() as f32 * INDENT))
        .pr(px(8.0))
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .rounded(px(8.0))
        .overflow_hidden()
        .role(AccessibilityRole::Button)
        .label(move || SharedString::from(name()))
        .accessibility_state(move || AccessibilityState {
            selected: selected(),
            ..AccessibilityState::default()
        })
        .bg(move || {
            if drop_to() {
                primary().opacity(0.24)
            } else if selected() {
                primary().opacity(0.12)
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .hover(|st| st.bg(move || primary().opacity(0.08)))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| {
            if let Some(r) = row.get_untracked() {
                select(&r.path);
            }
        })
        .child(
            when(has_children, move || {
                let toggle = toggle.clone();
                view()
                    .size(px(16.0))
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .role(AccessibilityRole::Button)
                    .label(move || format!("{}{}", if open() { "收起 " } else { "展开 " }, name()))
                    .hover(|st| st.bg(hover()))
                    .on_press(move |_: &()| {
                        if let Some(r) = row.get_untracked() {
                            toggle(&r.path, r.open);
                        }
                    })
                    .child(icon_dyn(
                        move || {
                            if open() {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            }
                        },
                        12.0,
                        faint,
                    ))
            })
            .otherwise(|| view().w(px(16.0)).flex_shrink_0()),
        )
        .child(
            view()
                .size(px(20.0))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .child(icon(Icon::Folder, 16.0, move || {
                    if selected() { primary() } else { sidebar_fg() }
                })),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(name)
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || if selected() { fg() } else { muted_fg() }),
                )
                .child(when(
                    move || count() > 0,
                    move || {
                        text(move || count().to_string())
                            .text_size(px(11.0))
                            .line_height(px(11.0))
                            .tabular_nums()
                            .text_color(faint())
                    },
                )),
        );
    tree_menu(
        cell.pan_values(Some(&ctx.gx), Some(&ctx.gy))
            .claim_touches(ctx.claim())
            .keep_touches(true)
            .on_responder_grant({
                let ctx = ctx.clone();
                move |_: &ResponderEvent| {
                    if let Some(r) = row.get_untracked() {
                        ctx.begin(vec![r.path.clone()], true, r.name.clone());
                    }
                }
            })
            .on_responder_move({
                let ctx = ctx.clone();
                move |e: &ResponderEvent| ctx.move_to(e.page)
            })
            .on_responder_release({
                let ctx = ctx.clone();
                move |e: &ResponderEvent| ctx.finish(e.page)
            })
            .on_responder_terminate({
                let ctx = ctx.clone();
                move |_: &ResponderEvent| ctx.cancel()
            }),
        row,
        ops,
    )
}
