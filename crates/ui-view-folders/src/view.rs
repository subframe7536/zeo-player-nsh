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
    .estimated_item_size(px(TREE_ROW + ROW_GAP))
    .row_gap(px(ROW_GAP))
    // 左右各 10pt：行高亮胶囊与 nav_row / 歌单行完全同宽对齐。
    .content_padding_x(px(10.0))
    .content_padding_bottom(px(BAR_SPACE))
    .element_ref(&ui.list_ref)
    .on_scroll(move |e: &ScrollEvent| scroll_sig.set(e.offset.y.0))
    .empty(empty_state(Icon::Folder, "没有文件夹", ""))
    .flex_1();

    let dragging = ui.dragging;
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
        .on_file_drag_enter(move || dragging.set(true))
        .on_file_drag_leave(move || dragging.set(false))
        .on_file_drop(move |event: &FileDropEvent| {
            dragging.set(false);
            on_drop(event);
        })
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
    let gx = ui.ctx.gx;
    let gy = ui.ctx.gy;
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

/// 「+」：系统选择器选目录加进曲库（同设置 › 曲库 的「添加文件夹…」，
/// 与把文件夹拖进来同效）；结果写副标题提示，选择器一次只开一个。
pub(crate) fn pick_folder_into_library() {
    let ui = folder_ui();
    if ui.picking.get_untracked() {
        return;
    }
    ui.picking.set(true);
    let message = store().folder_message;
    spawn(async move {
        if let Some(result) = store::pick_folders().await {
            show_message(message, result);
        }
        ui.picking.set(false);
    });
}

/// 侧栏「文件夹」小标题行的「+」钮（样式同 `sidebar_rescan_button`）。
pub fn add_folder_button() -> AnyElement {
    let ui = folder_ui();
    let picking = ui.picking;
    view()
        .size(px(22.0))
        .rounded(px(6.0))
        .items_center()
        .justify_center()
        .role(AccessibilityRole::Button)
        .label("添加文件夹")
        .window_drag(false)
        .opacity(move || if picking.get() { 0.4_f32 } else { 1.0_f32 })
        .hover(move |st| {
            if !picking.get() {
                st.bg(primary().opacity(0.08))
            } else {
                st
            }
        })
        .active(move |st| if !picking.get() { st.opacity(0.6) } else { st })
        .on_press(move |_: &()| pick_folder_into_library())
        .child(icon(Icon::Plus, 14.0, faint))
        .into_element()
}

/// 拖入装饰：外部文件拖到任一文件夹拖放目标（本页 / 侧栏树）期间盖
/// 住整窗的虚线遮罩。自身不接收事件——`pointer_events` 全关，命中判
/// 定与落下仍归原来的目标。
pub fn drop_overlay() -> impl IntoElement {
    let ui = folder_ui();
    let dragging = ui.dragging;
    when(
        move || dragging.get(),
        move || {
            view()
                .absolute()
                .inset(px(0.0))
                .pointer_events(PointerEvents::None)
                .items_center()
                .justify_center()
                .bg(move || bg().opacity(0.72))
                .child(
                    view()
                        .absolute()
                        .inset(px(12.0))
                        .rounded(px(16.0))
                        .border_2()
                        .border_dashed()
                        .border_color(move || primary()),
                )
                .child(
                    view()
                        .items_center()
                        .gap(px(10.0))
                        .child(icon(Icon::FolderOpen, 40.0, primary))
                        .child(
                            text("松开以添加文件夹到曲库")
                                .text_size(px(17.0))
                                .line_height(px(17.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(fg()),
                        )
                        .child(
                            text("已在曲库中的文件夹会直接定位过去")
                                .text_size(px(13.0))
                                .line_height(px(13.0))
                                .text_color(muted_fg()),
                        ),
                )
        },
    )
}

/// 树里的一行：缩进 + 文件夹图标 + 名字 + 曲目数。行是整宽的可点区
/// （选中）；图标本体就是折叠标识——有子级的行它是「展开 / 收起」钮
/// （`Folder` ⇄ `FolderOpen`），叶子行不可点；行也能拖起这个文件夹，
/// 或作为别的拖动落下的目标（`over` 命中时高亮）；右键是增删改菜单。
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
        // 与 nav_row 同位：图标格中心在 30pt（10 列表内边距 + 10 行内
        // 边距 + 半个 20pt 图标格），缩进在 10 之上逐层加。
        .pl(move || px(10.0 + depth() as f32 * INDENT))
        .pr(px(10.0))
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .rounded(px(8.0))
        .overflow_hidden()
        .cursor(Cursor::Pointer)
        .role(AccessibilityRole::Link)
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
        .on_press(move |_: &()| {
            if let Some(r) = row.get_untracked() {
                select(&r.path);
            }
        })
        .child(
            when(has_children, move || {
                let toggle = toggle.clone();
                view()
                    .size(px(20.0))
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
                                Icon::FolderOpen
                            } else {
                                Icon::Folder
                            }
                        },
                        17.0,
                        move || {
                            if selected() { primary() } else { sidebar_fg() }
                        },
                    ))
            })
            .otherwise(move || {
                view()
                    .size(px(20.0))
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .child(icon(Icon::Folder, 17.0, move || {
                        if selected() { primary() } else { sidebar_fg() }
                    }))
            }),
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
                        ctx.begin(vec![r.path.clone()], true, r.name);
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
