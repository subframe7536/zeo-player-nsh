use super::*;

#[component]
pub fn Folders() -> impl IntoElement {
    let s = store();
    let ui = folder_ui();
    let here = ui.here;
    let files = ui.files;
    let ctx = ui.ctx.clone();
    let ops = ui.ops.clone();
    let select_folder = ui.select.clone();
    let message = s.folder_message;

    let on_drop = {
        let select_folder = select_folder.clone();
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
                select_folder(&dir);
            }
            show_message(message, notes.join("；"));
        }
    };

    let filter_conditions = signal(vec![Condition::default()]);
    let match_all = signal(true);
    let filter_open = signal(false);
    let selected_paths = signal(BTreeSet::<String>::new());
    let anchor_path = signal(None::<String>);

    // 切换文件夹时清空过滤条件并收起筛选，清空选择状态。只在所在文件夹
    // 换身份时复位：同一文件夹的内容刷新（重扫、树里别的节点增删、并行
    // 用例动共享语料）不收筛选、不清选择。
    let here_path = computed(move || here.with(|p| p.path.clone()));
    effect(move || {
        let _ = here_path.get();
        filter_conditions.set(vec![Condition::default()]);
        match_all.set(true);
        filter_open.set(false);
        selected_paths.set(BTreeSet::new());
        anchor_path.set(None);
    });

    let filtered_files = computed(move || {
        let current_files = files.get();
        if !filter_open.get() {
            return current_files;
        }
        let active: Vec<Condition> =
            filter_conditions.with(|cs| cs.iter().filter(|c| c.is_complete()).cloned().collect());
        if active.is_empty() {
            return current_files;
        }
        let all_tracks = s.tracks.get();
        let now = unix_now();
        let all = match_all.get();
        let kept: Vec<usize> = current_files
            .iter()
            .copied()
            .filter(|&idx| {
                all_tracks.get(idx).is_some_and(|track| {
                    if all {
                        active.iter().all(|c| c.matches(track, now))
                    } else {
                        active.iter().any(|c| c.matches(track, now))
                    }
                })
            })
            .collect();
        Rc::new(kept)
    });

    // 筛选发生变化时，剔除不再可见的选中曲目与锚点
    effect(move || {
        let cur_files = filtered_files.get();
        let all = s.tracks.get();
        let visible: BTreeSet<String> = cur_files
            .iter()
            .filter_map(|&idx| all.get(idx).map(|t| t.path.clone()))
            .collect();
        selected_paths.update(|paths| {
            paths.retain(|p| visible.contains(p));
        });
        if let Some(ref a) = anchor_path.get_untracked() {
            if !visible.contains(a) {
                anchor_path.set(None);
            }
        }
    });

    view().flex_1().on_file_drop(on_drop).child(
        when(
            move || s.library.with(|l| l.config.roots.is_empty()),
            no_library,
        )
        .otherwise(move || {
            folder_page(
                &ui,
                filtered_files,
                filter_conditions,
                match_all,
                filter_open,
                selected_paths,
                anchor_path,
            )
        }),
    )
}

pub(crate) fn folder_page(
    ui: &FolderUi,
    filtered_files: Memo<Rc<Vec<usize>>>,
    filter_conditions: Signal<Vec<Condition>>,
    match_all: Signal<bool>,
    filter_open: Signal<bool>,
    selected_paths: Signal<BTreeSet<String>>,
    anchor_path: Signal<Option<String>>,
) -> AnyElement {
    let s = store();
    let ctx = ui.ctx.clone();
    let ops = ui.ops.clone();
    let here = ui.here;
    let files = ui.files;
    let message = s.folder_message;
    let list_ctx = ctx.clone();
    let list_ops = ops.clone();
    let subtitle = move || {
        let note = message.get();
        if !note.is_empty() {
            return note;
        }
        let sel_count = selected_paths.with(|s| s.len());
        let has_active =
            filter_open.get() && filter_conditions.with(|cs| cs.iter().any(|c| c.is_complete()));
        let total = files.with(|f| f.len());
        let shown = filtered_files.with(|f| f.len());
        if sel_count > 0 {
            if has_active {
                return format!(
                    "已选 {} 首 · 筛选出 {} 首（共 {} 首）",
                    sel_count, shown, total
                );
            } else {
                return format!("已选 {} 首（共 {} 首）", sel_count, total);
            }
        }
        if has_active {
            return format!("筛选出 {} 首（共 {} 首）", shown, total);
        }
        here.with(|p| {
            let mut parts = Vec::new();
            if !p.trail.is_empty() && p.trail != p.title {
                parts.push(p.trail.clone());
            }
            if p.folders > 0 {
                parts.push(format!("{} 个文件夹", p.folders));
            }
            parts.push(format!("{} 首", p.tracks));
            parts.push(format_bytes(p.bytes));
            parts.join(" · ")
        })
    };
    let title = move || here.with(|p| p.title.clone());
    let actions = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(rescan_button())
        .child(when(
            move || here.with(|p| !p.all.is_empty()),
            move || {
                pill_button(Icon::Play, "播放全部", move || {
                    let cur = filtered_files.get_untracked();
                    if !cur.is_empty() {
                        play_indices(&cur, 0);
                    } else {
                        here.with_untracked(|p| play_indices(&p.all, 0));
                    }
                })
            },
        ))
        .child(when(
            move || here.with(|p| !p.all.is_empty()),
            move || {
                dynamic(move || {
                    if filter_open.get() {
                        outline_button(Icon::Close, "清除筛选", move || {
                            filter_conditions.set(vec![Condition::default()]);
                            match_all.set(true);
                            filter_open.set(false);
                        })
                        .into_element()
                    } else {
                        outline_button(Icon::Search, "筛选", move || {
                            filter_open.set(true);
                        })
                        .into_element()
                    }
                })
            },
        ));
    let filter_bar = when(
        move || filter_open.get(),
        move || {
            let matched = computed(move || filtered_files.with(|f| f.len()));
            let chip = move |label: &'static str, all: bool| {
                tab_trigger(
                    move || match_all.get() == all,
                    None,
                    label,
                    move || match_all.set(all),
                )
            };
            let add = text_button(
                Variant::Ghost,
                Size::Sm,
                Some(Icon::Plus),
                "添加条件",
                move || filter_conditions.update(|cs| cs.push(Condition::default())),
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

            let filter_count = computed(move || filter_conditions.with(|cs| cs.len()));
            let rows = dynamic(move || {
                let n = filter_count.get();
                (0..n)
                    .map(|i| {
                        let get = move || {
                            filter_conditions.with(|cs| cs.get(i).cloned().unwrap_or_default())
                        };
                        let change = move |f: Box<dyn FnOnce(&mut Condition)>| {
                            filter_conditions.update(|cs| {
                                if let Some(c) = cs.get_mut(i) {
                                    f(c);
                                }
                            });
                        };
                        let remove = Rc::new(move || {
                            filter_conditions.update(|cs| {
                                if cs.len() > 1 {
                                    if i < cs.len() {
                                        cs.remove(i);
                                    }
                                } else if let Some(c) = cs.first_mut() {
                                    *c = Condition::default();
                                }
                            });
                        });
                        zeo_ui_view_playlists::condition_editor(get, change, Some(remove))
                            .into_element()
                    })
                    .collect::<Vec<_>>()
            });

            view()
                .px(px(32.0))
                .py(px(12.0))
                .gap(px(12.0))
                .border_b_1()
                .border_b_color(border())
                .child(mode)
                .child(view().gap(px(8.0)).child(rows))
        },
    );
    let header = view()
        .child(page_header(
            title,
            subtitle,
            None,
            Some(actions.into_element()),
        ))
        .child(filter_bar)
        .child(track_columns(FOLDER_COLS));
    let row_ctx = FileRowCtx {
        s,
        filtered_files,
        files,
        selected_paths,
        anchor_path,
        list_ctx,
        list_ops,
    };
    let file_list = virtual_list(
        computed(move || filtered_files.with(|f| f.len())),
        move |at| file_row(at, &row_ctx),
    )
    .keys(move || {
        let all = s.tracks.get();
        filtered_files
            .get()
            .iter()
            .map(|&i| all.get(i).map(|t| t.path.clone()).unwrap_or_default())
            .collect::<Vec<_>>()
    })
    .estimated_item_size(px(ROW))
    .content_padding_bottom(px(BAR_SPACE))
    .content_padding_x(px(20.0))
    .flex_1()
    .empty(dynamic(move || {
        let has_active =
            filter_open.get() && filter_conditions.with(|cs| cs.iter().any(|c| c.is_complete()));
        if has_active {
            empty_state(Icon::Search, "没有符合条件的曲目", "尝试更换筛选条件或值。")
        } else {
            empty_state(
                Icon::Folder,
                "这个文件夹没有曲目",
                "它包含的音乐可能在子文件夹里，在侧栏树里选一个子文件夹。",
            )
        }
    }));
    view()
        .flex_1()
        .min_w(px(0.0))
        .child(header)
        .child(file_list)
        .into_element()
}
