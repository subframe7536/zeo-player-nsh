use super::*;

/// 新建 / 重命名：一个输入框 + 取消 / 确定；回车即确定（`on_submit`），
/// 名字不合法 / 已存在时错误文案留在弹窗里。
pub(crate) fn name_dialog(
    ops: Rc<Ops>,
    op: FileOp,
    close: impl Fn() + Copy + 'static,
) -> AnyElement {
    let (heading, default, confirm) = match &op {
        FileOp::NewFolder { .. } => ("新建文件夹", String::new(), "创建"),
        FileOp::Rename { path, dir } => (
            if *dir {
                "重命名文件夹"
            } else {
                "重命名文件"
            },
            rename_default(path, *dir),
            "重命名",
        ),
        FileOp::DeleteFolder { .. } => return view().into_element(),
    };
    let draft = signal(default);
    let err = signal(String::new());
    let submit = Rc::new({
        let ops = ops.clone();
        move || {
            let name = draft.get_untracked();
            if let Err(e) = ops.submit_name(&name) {
                err.set(e);
            }
        }
    });
    let content = dialog_content(425.0)
        .child(dialog_header(heading, None))
        .child(
            input_field(false)
                .w_full()
                .label("名称")
                .auto_focus(true)
                .default_value(untracked(|| draft.get()))
                .placeholder("名称")
                .on_change(move |t: &str| {
                    draft.set(t.to_owned());
                    err.set(String::new());
                })
                .on_submit({
                    let submit = submit.clone();
                    move |_: &str| submit()
                })
                .on_blur({
                    let submit = submit.clone();
                    move || submit()
                }),
        )
        .child(
            text(move || err.get())
                .number_of_lines(1)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(move || nsh_base::theme().destructive),
        )
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
                    confirm,
                    move || submit(),
                )),
        );
    modal(content, close)
}

/// 「删除文件夹」确认弹窗（回收站 / 直接删按「删除文件」设置）：整棵
/// 子树都没了，曲库里的曲目记录也一并移除。
pub(crate) fn delete_folder_dialog(
    ops: Rc<Ops>,
    path: String,
    close: impl Fn() + Copy + 'static,
) -> AnyElement {
    let permanent = library_store().delete_permanently();
    let (desc, confirm) = if permanent {
        (
            "将永久删除整个文件夹（含其中的文件，不经过回收站，无法恢复），曲库记录一并移除。",
            "永久删除",
        )
    } else {
        ("将把整个文件夹移到回收站，曲库记录一并移除。", "移到回收站")
    };
    let content = dialog_content(440.0)
        .child(dialog_header("删除文件夹", Some(desc)))
        .child(
            text(last_segment(&path).to_owned())
                .number_of_lines(1)
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
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
                    Variant::Destructive,
                    Size::Default,
                    None,
                    confirm,
                    move || ops.run_delete(path.clone()),
                )),
        );
    modal(content, close)
}

/// 树行右键菜单：新建子文件夹 / 重命名 / 在文件管理器中显示 / 删除。
/// 根目录行特殊：它是曲库根（设置页管），不给重命名 / 删除，改成
/// 「从曲库移除」（只摘配置，不动文件）。
pub(crate) fn tree_menu(trigger: View, row: Memo<Option<TreeRow>>, ops: Rc<Ops>) -> AnyElement {
    let gate = store::menu_control();
    let menu = context_menu()
        .trigger(trigger)
        .open(gate.open())
        .on_open_change(gate.menu_change());
    let path = move || row.with_untracked(|r| r.as_ref().map(|r| r.path.clone()));
    let is_root = row.with_untracked(|r| r.as_ref().is_some_and(|r| r.depth == 0));
    let message = ops.message;
    let open = move |make: fn(String) -> FileOp| {
        let ops = ops.clone();
        move || {
            if let Some(p) = path() {
                ops.op.set(Some(make(p)));
            }
        }
    };
    popup!(menu, 200.0)
        .child(item(
            "新建子文件夹…",
            Some(Icon::Plus),
            open(|p| FileOp::NewFolder { parent: p }),
        ))
        .child((!is_root).then(|| {
            item(
                "重命名…",
                Some(Icon::Edit),
                open(|p| FileOp::Rename { path: p, dir: true }),
            )
        }))
        .child(item(REVEAL_LABEL, Some(Icon::Reveal), {
            let path = path;
            move || {
                if let Some(p) = path() {
                    reveal(&p);
                }
            }
        }))
        .child(separator())
        .child(if is_root {
            Some(item("从曲库移除", Some(Icon::Close), move || {
                if let Some(p) = path() {
                    // 等菜单关掉再摘：行会随树重建一起拆掉。
                    store::later(move || {
                        library_store().remove_root(&p);
                        // 根摘掉了：其下的索引与元数据记录一起清（文件还
                        // 在，歌单条目保留——重新加根后又出现）。
                        library_store().remove_under(&p);
                        show_message(message, format!("已移除 {}", last_segment(&p)));
                    });
                }
            }))
        } else {
            Some(item(
                "删除文件夹…",
                Some(Icon::Trash),
                open(|p| FileOp::DeleteFolder { path: p }),
            ))
        })
        .into_element()
}
