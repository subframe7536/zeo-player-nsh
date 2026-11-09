use super::*;

/// `Folders` 行构建共享的上下文（信号与把手都是 Copy/共享克隆）。
pub(crate) struct FileRowCtx {
    pub(crate) s: store::Store,
    pub(crate) filtered_files: Memo<Rc<Vec<usize>>>,
    pub(crate) files: Memo<Rc<Vec<usize>>>,
    pub(crate) selected_paths: Signal<BTreeSet<String>>,
    pub(crate) anchor_path: Signal<Option<String>>,
    pub(crate) list_ctx: Rc<DragCtx>,
    pub(crate) list_ops: Rc<Ops>,
}

pub(crate) fn file_row(at: Position, c: &FileRowCtx) -> AnyElement {
    let s = c.s;
    let filtered_files = c.filtered_files;
    let files = c.files;
    let selected_paths = c.selected_paths;
    let anchor_path = c.anchor_path;
    let list_ctx = c.list_ctx.clone();
    let list_ops = c.list_ops.clone();
    let index = computed(move || filtered_files.with(|f| f.get(at.index()).copied()));
    let track =
        computed(move || index.with(|i| i.and_then(|i| s.tracks.with(|t| t.get(i).cloned()))));
    let number = computed(move || (at.index() + 1).to_string());
    let play = Rc::new(move || {
        let files = filtered_files.get_untracked();
        play_indices(&files, at.index());
    });
    let is_selected = {
        let track = track.clone();
        let selected_paths = selected_paths;
        move || {
            track.with(|t| {
                t.as_ref()
                    .is_some_and(|t| selected_paths.with(|paths| paths.contains(&t.path)))
            })
        }
    };
    let on_select_press = {
        let track = track.clone();
        let selected_paths = selected_paths;
        let anchor_path = anchor_path;
        let filtered_files = filtered_files;
        let s = s.clone();
        move |e: &PressEvent| {
            let Some(t) = track.get_untracked() else {
                return;
            };
            let p = t.path.clone();
            // macOS 按 Cmd、其余按 Ctrl，两个都认（无头测试
            // 的 `mod` 按 environment.os 映射，跟编译平台无关）。
            let toggle = e.modifiers.cmd || e.modifiers.ctrl;
            let range = e.modifiers.shift;

            if range {
                let files = filtered_files.get_untracked();
                let all = s.tracks.get_untracked();
                let cur_idx = files
                    .iter()
                    .position(|&idx| all.get(idx).map(|tr| &tr.path) == Some(&p));
                let a_idx = anchor_path.with_untracked(|a| {
                    a.as_ref().and_then(|a_path| {
                        files
                            .iter()
                            .position(|&idx| all.get(idx).map(|tr| &tr.path) == Some(a_path))
                    })
                });
                if let (Some(cur_idx), Some(a_idx)) = (cur_idx, a_idx) {
                    let start = cur_idx.min(a_idx);
                    let end = cur_idx.max(a_idx);
                    let range_paths: BTreeSet<String> = files[start..=end]
                        .iter()
                        .filter_map(|&idx| all.get(idx).map(|tr| tr.path.clone()))
                        .collect();
                    selected_paths.update(|set| {
                        if toggle {
                            set.extend(range_paths);
                        } else {
                            *set = range_paths;
                        }
                    });
                } else {
                    selected_paths.set(BTreeSet::from([p.clone()]));
                    anchor_path.set(Some(p));
                }
            } else if toggle {
                selected_paths.update(|set| {
                    if !set.remove(&p) {
                        set.insert(p.clone());
                    }
                });
                anchor_path.set(Some(p));
            } else {
                selected_paths.set(BTreeSet::from([p.clone()]));
                anchor_path.set(Some(p));
            }
        }
    };
    let play_action = {
        let play = play.clone();
        let selected_paths = selected_paths;
        let files = files.clone();
        let track = track.clone();
        move || {
            let (is_multi, sel_set) = selected_paths.with_untracked(|sel| {
                let cur = track.with_untracked(|t| t.as_ref().map(|t| t.path.clone()));
                let is_multi = cur.as_ref().is_some_and(|p| sel.contains(p)) && sel.len() > 1;
                (is_multi, sel.clone())
            });
            if is_multi {
                let all_tracks = s.tracks.get_untracked();
                let selected_tracks: Vec<TrackEntry> = files.with_untracked(|f| {
                    f.iter()
                        .filter_map(|&idx| all_tracks.get(idx).cloned())
                        .filter(|t| sel_set.contains(&t.path))
                        .collect()
                });
                if !selected_tracks.is_empty() {
                    let cur_path = track.with_untracked(|t| t.as_ref().map(|t| t.path.clone()));
                    let start_idx = selected_tracks
                        .iter()
                        .position(|t| Some(&t.path) == cur_path.as_ref())
                        .unwrap_or(0);
                    store::play_list(&selected_tracks, start_idx);
                    return;
                }
            }
            play();
        }
    };
    let row = track_row_selectable(track, number, FOLDER_COLS, is_selected, on_select_press, {
        let play = play.clone();
        move || play()
    })
    .pan_values(Some(&list_ctx.gx), Some(&list_ctx.gy))
    .claim_touches(list_ctx.claim())
    .keep_touches(true)
    .on_responder_grant({
        let ctx = list_ctx.clone();
        let selected_paths = selected_paths;
        let anchor_path = anchor_path;
        let files = files.clone();
        let track = track.clone();
        move |_: &ResponderEvent| {
            if let Some(t) = track.get_untracked() {
                let (sources, name) = selected_paths.with_untracked(|sel| {
                    if sel.contains(&t.path) && sel.len() > 1 {
                        let all_tracks = s.tracks.get_untracked();
                        let ordered_paths: Vec<String> = files.with_untracked(|f| {
                            f.iter()
                                .filter_map(|&idx| all_tracks.get(idx))
                                .filter(|track| sel.contains(&track.path))
                                .map(|track| track.path.clone())
                                .collect()
                        });
                        let count = ordered_paths.len();
                        (ordered_paths, format!("{} 等 {} 个文件", t.title, count))
                    } else {
                        (vec![t.path.clone()], t.title.clone())
                    }
                });
                if !sources.contains(&t.path) {
                    selected_paths.set(BTreeSet::from([t.path.clone()]));
                    anchor_path.set(Some(t.path.clone()));
                }
                ctx.begin(sources, false, name);
            }
        }
    })
    .on_responder_move({
        let ctx = list_ctx.clone();
        move |e: &ResponderEvent| ctx.move_to(e.page)
    })
    .on_responder_release({
        let ctx = list_ctx.clone();
        move |e: &ResponderEvent| ctx.finish(e.page)
    })
    .on_responder_terminate({
        let ctx = list_ctx.clone();
        move |_: &ResponderEvent| ctx.cancel()
    })
    .on_context_menu({
        let selected_paths = selected_paths;
        let anchor_path = anchor_path;
        let track = track.clone();
        move |_: &ContextMenuEvent| {
            if let Some(t) = track.get_untracked() {
                selected_paths.update(|set| {
                    if !set.contains(&t.path) {
                        set.clear();
                        set.insert(t.path.clone());
                        anchor_path.set(Some(t.path.clone()));
                    }
                });
            }
        }
    });
    let get_selected = {
        let selected_paths = selected_paths;
        let files = files.clone();
        Rc::new(move || {
            let set = selected_paths.get();
            let all_tracks = s.tracks.get_untracked();
            files.with_untracked(|f| {
                f.iter()
                    .filter_map(|&idx| all_tracks.get(idx))
                    .filter(|t| set.contains(&t.path))
                    .map(|t| t.path.clone())
                    .collect::<Vec<_>>()
            })
        }) as Rc<dyn Fn() -> Vec<String>>
    };
    zeo_ui_views_base::track_menu::track_menu_selectable(
        row,
        track,
        None,
        play_action,
        {
            let ops = list_ops.clone();
            Some(Rc::new(move |p: &str| {
                ops.op.set(Some(FileOp::Rename {
                    path: p.to_owned(),
                    dir: false,
                }))
            }) as Rc<dyn Fn(&str)>)
        },
        Some(get_selected),
    )
    .into_element()
}
