use super::*;

/// 顶栏改名模式的状态：文件名主干输入与标签草稿分家——改名是文件
/// 操作（`rename::apply`，含同名侧车），不走 `TagEdits` / 保存钮。
#[derive(Clone, Copy)]
pub(crate) struct RenameState {
    /// 改名模式开着（顶栏整行换成输入 + ✓ / ✕）。
    pub(crate) renaming: Signal<bool>,
    /// 文件名主干（不含扩展名）输入。
    pub(crate) name_input: Signal<String>,
    /// `plan_name` 预检 / `apply` 执行报错（状态行显示，✓ 禁用）。
    pub(crate) err: Signal<Option<String>>,
    /// 后台改名进行中（输入只读、✓ ✕ 禁用）。
    pub(crate) busy: Signal<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SingleDraft {
    pub(crate) path: String,
    pub(crate) original: HashMap<TagField, String>,
    pub(crate) values: HashMap<TagField, String>,
    /// 内嵌封面改动（`None` 不动）。
    pub(crate) cover: Option<CoverEdit>,
    /// `Replace` 时新图的本地路径（预览用）。
    pub(crate) cover_src: Option<String>,
    pub(crate) saving: bool,
    pub(crate) saved: bool,
    pub(crate) error: Option<String>,
}

pub(crate) fn track_seed(t: &TrackEntry) -> HashMap<TagField, String> {
    TagField::ALL
        .iter()
        .map(|&f| (f, entry_field_of(t, f)))
        .collect()
}

pub(crate) fn single_panel(
    path: String,
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    close: Rc<dyn Fn()>,
) -> View {
    // 完整字段值优先读 LRU 缓存与曲库索引条目（第 0 帧秒开，歌词与注释零延迟呈现）；
    // 未命中走后台异步读取（Lofty 跳过大封面与音频属性解析，亚毫秒完成）。
    let entry = displayed
        .with_untracked(|list| list.iter().find(|t| t.path == path).cloned())
        .or_else(|| {
            store::store()
                .tracks
                .with_untracked(|list| list.iter().find(|t| t.path == path).cloned())
        });
    let cached = get_cached_full(&path);
    let initial_values = cached
        .as_ref()
        .map(|(fields, _, _)| fields.clone())
        .or_else(|| entry.as_ref().map(track_seed))
        .unwrap_or_default();
    let full = signal(cached.clone());
    let alive = Arc::new(AtomicBool::new(true));
    cleanup({
        let alive = alive.clone();
        move || alive.store(false, Ordering::Relaxed)
    });
    let draft = signal(SingleDraft {
        path: path.clone(),
        original: initial_values.clone(),
        values: initial_values,
        cover: None,
        cover_src: None,
        saving: false,
        saved: false,
        error: None,
    });
    let rename_state = RenameState {
        renaming: signal(false),
        name_input: signal(
            Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        err: signal(None),
        busy: signal(false),
    };

    if full.get_untracked().is_none() {
        let read = path.clone();
        let alive_t = alive.clone();
        spawn(async move {
            let read_p = PathBuf::from(&read);
            let read_target = read.clone();
            let Ok(res) = blocking(move || {
                let (fields, tag_type) = write::read_row(&read_p).unwrap_or_default();
                let added = zeo_db::added_at(&read_target);
                (fields, tag_type, added)
            })
            .await
            else {
                return;
            };
            if !alive_t.load(Ordering::Relaxed) {
                return;
            }
            put_cached_full(&read, res.clone());
            let (fields, tag_type, added) = res;
            draft.update(|d| {
                for (f, v) in &fields {
                    if d.values.get(f) == d.original.get(f) {
                        d.values.insert(*f, v.clone());
                    }
                }
                d.original = fields.clone();
            });
            full.set(Some((fields, tag_type, added)));
        });
    }

    let file_label = Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| zeo_ui_views_base::last_segment(&path).to_owned());
    view()
        .flex_1()
        .min_h(px(0.0))
        .child(single_header(
            &path,
            file_label,
            rename_state,
            draft,
            full,
            rows,
            selected,
            alive.clone(),
            close,
        ))
        .child(
            scroll().key(path.clone()).flex_1().min_h(px(0.0)).child(
                view()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(16.0))
                    .gap(px(12.0))
                    .overflow_hidden()
                    // 状态行是滚动区第一条：写回 / 改名报错（红）与
                    // 「已保存」（淡色），空着不占位。
                    .child(when(
                        move || {
                            draft.with(|d| d.error.is_some() || d.saved)
                                || rename_state.err.with(|e| e.is_some())
                        },
                        move || {
                            text(move || {
                                let d = draft.get();
                                if let Some(e) = &d.error {
                                    e.clone()
                                } else if let Some(e) = rename_state.err.get() {
                                    e
                                } else if d.saved {
                                    "已保存".to_owned()
                                } else {
                                    String::new()
                                }
                            })
                            .number_of_lines(1)
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .text_color(move || {
                                if draft.with(|d| d.error.is_some())
                                    || rename_state.err.with(|e| e.is_some())
                                {
                                    nsh_base::theme().destructive
                                } else {
                                    muted_fg()
                                }
                            })
                        },
                    ))
                    .child(single_form(path.clone(), draft, rows, full)),
            ),
        )
}

/// 单选面板顶栏（与 [`panel_header`] 同款 `px-4 pt-3 pb-2` + 底部分
/// 隔线）：常态「文件名 + 铅笔 + 保存 + ✕」；改名模式整行换成「主干
/// 输入 + `.扩展名` + ✓ + ✕」（`Input` 没有取消事件，Esc 由 ✕ 承担）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn single_header(
    path: &str,
    file_label: String,
    rename_state: RenameState,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    alive: Arc<AtomicBool>,
    close: Rc<dyn Fn()>,
) -> View {
    let stem: Rc<String> = Rc::new(
        Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    let ext = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().into_owned());
    let path = path.to_owned();
    let commit: Rc<dyn Fn()> = Rc::new({
        let path = path.clone();
        let alive = alive.clone();
        move || commit_rename(&path, rename_state, rows, selected, alive.clone())
    });
    view()
        .h(px(36.0))
        .px(px(16.0))
        .border_b_1()
        .border_b_color(border())
        .child(dynamic(move || {
            let parts: Vec<AnyElement> = if rename_state.renaming.get() {
                // 改名模式：主干可编（扩展名独立显示、完成时改名），回车
                // 或 ✓ 提交；输入即时 `plan_name` 预检（分隔符硬规则 /
                // 撞名先报，✓ 随之禁用）。
                let mut parts: Vec<AnyElement> = vec![
                    input_field(true, |i| {
                        i.py(px(0.0))
                            .text_size(px(12.0))
                            .label("文件名")
                            .auto_focus(true)
                            .default_value(rename_state.name_input.get_untracked())
                            .readonly(move || rename_state.busy.get())
                            .on_change({
                                let path = path.clone();
                                move |t: &str| {
                                    rename_state.name_input.set(t.to_owned());
                                    rename_state.err.set(
                                        match &rename::plan_name(&path, t).status {
                                            Status::Ok | Status::Unchanged => None,
                                            Status::Invalid(m) | Status::Conflict(m) => {
                                                Some(m.clone())
                                            }
                                        },
                                    );
                                }
                            })
                            .on_submit({
                                let commit = commit.clone();
                                move |_: &str| commit()
                            })
                            .on_blur({
                                let commit = commit.clone();
                                move || commit()
                            })
                    })
                    .flex_1()
                    .min_w(px(0.0))
                    .h(px(24.0))
                    .into_element(),
                ];
                if let Some(e) = &ext {
                    parts.push(
                        text(format!(".{e}"))
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .text_color(muted_fg())
                            .into_element(),
                    );
                }
                parts.push(
                    icon_button(|| Icon::Check, 12.0, "完成重命名", muted_fg, {
                        let commit = commit.clone();
                        move || commit()
                    })
                    .disabled(move || {
                        rename_state.busy.get() || rename_state.err.with(|e| e.is_some())
                    })
                    .into_element(),
                );
                parts.push(
                    icon_button(|| Icon::Close, 12.0, "取消重命名", muted_fg, {
                        let stem = stem.clone();
                        move || {
                            rename_state.err.set(None);
                            rename_state.name_input.set((*stem).clone());
                            rename_state.renaming.set(false);
                        }
                    })
                    .disabled(move || rename_state.busy.get())
                    .into_element(),
                );
                parts
            } else {
                vec![
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            text(file_label.clone())
                                .flex_1()
                                .min_w(px(0.0))
                                .number_of_lines(1)
                                .text_size(px(13.0))
                                .line_height(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(fg()),
                        )
                        .child(
                            icon_button(|| Icon::Edit, 12.0, "重命名", muted_fg, {
                                let stem = stem.clone();
                                move || {
                                    rename_state.name_input.set((*stem).clone());
                                    rename_state.err.set(None);
                                    rename_state.renaming.set(true);
                                }
                            })
                            .disabled(move || {
                                (draft.with(|d| d.values.is_empty()) && full.get().is_none())
                                    || rename_state.busy.get()
                            }),
                        )
                        .into_element(),
                    text_button(Variant::Default, Size::Xs, None, "保存", {
                        let alive = alive.clone();
                        move || save_single(draft, rows, alive.clone())
                    })
                    .accessibility_label("保存")
                    .disabled(move || {
                        (draft.with(|d| d.values.is_empty()) && full.get().is_none())
                            || draft.with(|d| {
                                d.saving || !valid(&d.values) || build_edits(d).is_empty()
                            })
                    })
                    .into_element(),
                    icon_button(|| Icon::Close, 12.0, "关闭", muted_fg, {
                        let close = close.clone();
                        move || close()
                    })
                    .into_element(),
                ]
            };
            view()
                .h_full()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .children(parts)
                .into_element()
        }))
}

/// 单选表单本体（数据就绪后挂载，输入框非受控——`default_value` 只在
/// 挂载时生效，所以全部种子必须先落定）。
pub(crate) fn single_form(
    path: String,
    draft: Signal<SingleDraft>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    full: Signal<Option<FullRow>>,
) -> View {
    view()
        .gap(px(18.0))
        .child(section(
            "基本信息",
            vec![
                field_row("标题", TagField::Title, draft, full),
                field_row("歌手", TagField::Artist, draft, full),
                field_row("专辑", TagField::Album, draft, full),
                field_row("专辑歌手", TagField::AlbumArtist, draft, full),
                genre_row(draft, full),
                narrow_row("年份", TagField::Year, draft, full, 64.0),
                pair_row(
                    "音轨",
                    "音轨号",
                    TagField::Track,
                    "音轨总数",
                    TagField::TrackTotal,
                    draft,
                    full,
                ),
                pair_row(
                    "碟号",
                    "碟号",
                    TagField::Disc,
                    "碟总数",
                    TagField::DiscTotal,
                    draft,
                    full,
                ),
            ],
        ))
        .child(section(
            "人员与版权",
            vec![
                field_row("作曲", TagField::Composer, draft, full),
                field_row("指挥", TagField::Conductor, draft, full),
                field_row("作词", TagField::Lyricist, draft, full),
                field_row("发行商", TagField::Publisher, draft, full),
                field_row("版权", TagField::Copyright, draft, full),
                rating_row(draft, full),
            ],
        ))
        .child(section(
            "封面与歌词",
            vec![
                cover_row(draft, full),
                multi_row("歌词", TagField::Lyrics, draft, full, 140.0, 260.0),
                multi_row("注释", TagField::Comment, draft, full, 64.0, 120.0),
            ],
        ))
        .child(section(
            "ReplayGain",
            vec![
                narrow_row("音轨增益", TagField::TrackGain, draft, full, 88.0),
                narrow_row("音轨峰值", TagField::TrackPeak, draft, full, 88.0),
                narrow_row("专辑增益", TagField::AlbumGain, draft, full, 88.0),
                narrow_row("专辑峰值", TagField::AlbumPeak, draft, full, 88.0),
            ],
        ))
        .child(tech_section(&path, rows, full))
}
