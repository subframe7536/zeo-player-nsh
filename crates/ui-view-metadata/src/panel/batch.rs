use super::*;

/// 字段三态。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Tri {
    /// 保持原值。
    Keep,
    /// 统一设为输入值。
    Set,
    /// 清空。
    Clear,
}

/// 英文大小写格式化。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CaseOp {
    /// 首字母大写（每个 ASCII 字母词）。
    Title,
    /// 全部小写。
    Lower,
    /// 全部大写。
    Upper,
}

/// 批量封面改动。
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum CoverMode {
    #[default]
    Keep,
    Replace {
        data: Vec<u8>,
        mime: String,
        src: String,
    },
    Remove,
}

/// 三态字段（spec §四：艺术家、专辑、流派、年份、碟号、作曲、发行商等）。
pub(crate) const TRI_FIELDS: &[TagField] = &[
    TagField::Artist,
    TagField::Album,
    TagField::AlbumArtist,
    TagField::Genre,
    TagField::Year,
    TagField::TrackTotal,
    TagField::Disc,
    TagField::DiscTotal,
    TagField::Composer,
    TagField::Conductor,
    TagField::Lyricist,
    TagField::Publisher,
    TagField::Copyright,
    TagField::Comment,
    TagField::Rating,
];

/// 批量草稿。三个工具都是「实时」的：填了就生效（空 / 不改动 = 不启用），
/// 效果即时预览在工具行下方，随「应用」一并写回——没有「暂存」这一步。
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct BatchDraft {
    pub(crate) modes: HashMap<TagField, Tri>,
    pub(crate) values: HashMap<TagField, String>,
    /// 音轨编号起始号（空 = 不改编号）与「同时写入音轨总数」。
    pub(crate) num_start: String,
    pub(crate) num_total: bool,
    /// 英文大小写格式化（None = 不改；作用于标题 / 歌手 / 专辑）。
    pub(crate) case: Option<CaseOp>,
    /// 文件名模板（空 = 不改名）。
    pub(crate) rename_tpl: String,
    pub(crate) cover: CoverMode,
    pub(crate) applying: bool,
    pub(crate) message: Option<String>,
}

impl BatchDraft {
    /// 编号设置：起始号能解析才算数（解析不了由预览报错）。
    pub(crate) fn numbering(&self) -> Option<(u32, bool)> {
        let start = self.num_start.trim().parse::<u32>().ok()?;
        Some((start, self.num_total))
    }
}

/// 大小写下拉的选项（`None` = 不改动）。
const CASE_CHOICES: [(&str, Option<CaseOp>); 4] = [
    ("不改动", None),
    ("首字母大写", Some(CaseOp::Title)),
    ("全部小写", Some(CaseOp::Lower)),
    ("全部大写", Some(CaseOp::Upper)),
];

pub(crate) fn batch_panel(
    paths: Rc<Vec<String>>,
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    close: Rc<dyn Fn()>,
) -> View {
    let draft = signal(BatchDraft::default());
    let m = paths.len();
    let busy = move || draft.with(|d| d.applying);

    // 效果预览：只算所选里的前几首（显示序），每次按键重算也便宜；
    // 全量校验（重名 / 磁盘冲突）在「应用」时做。
    let path_set: Rc<std::collections::HashSet<String>> = Rc::new(paths.iter().cloned().collect());
    let sample = computed(move || Rc::new(displayed.with(|v| sample_of(v, &path_set))));
    let preview = computed(move || {
        let sample = sample.get();
        let map = rows.get();
        draft.with(|d| Rc::new(build_preview(d, &sample, m, &map)))
    });
    let template_err = computed(move || draft.with(|d| template_error(&d.rename_tpl)));
    let rename_open = signal(false);

    // 工具块：控件行 + 下方的效果预览 / 提示；块之间留 14pt。
    let tools = view()
        .gap(px(14.0))
        .child(
            view()
                .child(wrap_row(
                    "音轨编号",
                    vec![
                        input_field(true)
                            .w(px(72.0))
                            .label("编号起始")
                            .placeholder("不改动")
                            .value(move || draft.with(|d| d.num_start.clone()))
                            .readonly(busy)
                            .on_change(move |t: &str| {
                                draft.update(|d| t.clone_into(&mut d.num_start))
                            })
                            .into_element(),
                        // 复选框与它的文字是一组，折行时不拆开。
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .child(checkbox(
                                move || draft.with(|d| d.num_total),
                                || false,
                                "同时写入音轨总数",
                                move |next| draft.update(|d| d.num_total = next),
                            ))
                            .child(
                                text("写入总数")
                                    .text_size(px(12.0))
                                    .line_height(px(12.0))
                                    .text_color(fg()),
                            )
                            .into_element(),
                    ],
                ))
                .child(preview_block(
                    move || preview.with(|p| p.number.clone()),
                    "按当前显示序连续编号",
                )),
        )
        .child(
            view()
                .child(wrap_row(
                    "大小写",
                    vec![
                        select(
                            move || {
                                let now = draft.with(|d| d.case);
                                CASE_CHOICES
                                    .iter()
                                    .find(|(_, op)| *op == now)
                                    .map_or("不改动", |(name, _)| name)
                                    .to_owned()
                            },
                            "大小写",
                            CASE_CHOICES
                                .iter()
                                .map(|(name, op)| {
                                    let op = *op;
                                    SelectOption::new(name, move || draft.update(|d| d.case = op))
                                })
                                .collect(),
                            128.0,
                        )
                        .into_element(),
                    ],
                ))
                .child(preview_block(
                    move || preview.with(|p| p.case.clone()),
                    "作用于标题 / 歌手 / 专辑",
                )),
        )
        .child(
            view()
                .child(wrap_row("重命名", vec![rename_button(draft, rename_open)]))
                .child(preview_block(
                    move || preview.with(|p| p.rename.clone()),
                    "点按钮设置模板；只改文件名，同名 .lrc 与封面一起改",
                )),
        );
    // 建立所选路径的 TrackEntry 快速查找表（一次 pass），避免三态字段
    // 计算占位符与批量应用时对全表进行 O(M × N) 线性扫描。
    let selected_entries: Rc<HashMap<String, TrackEntry>> = Rc::new({
        let path_set: std::collections::HashSet<&str> = paths.iter().map(|s| s.as_str()).collect();
        displayed.with_untracked(|v| {
            v.iter()
                .filter(|t| path_set.contains(t.path.as_str()))
                .map(|t| (t.path.clone(), t.clone()))
                .collect()
        })
    });
    let mut fields = view().gap(px(10.0));
    for field in TRI_FIELDS {
        fields = fields.child(tri_row(
            *field,
            paths.clone(),
            selected_entries.clone(),
            rows,
            draft,
        ));
    }
    let form = view()
        .px(px(16.0))
        .pt(px(16.0))
        .pb(px(16.0))
        .gap(px(18.0))
        .overflow_hidden()
        // 消息行是滚动区第一条（同单选面板的状态行），空着不占位。
        .child(when(
            move || draft.with(|d| d.message.is_some()),
            move || {
                text(move || draft.with(|d| d.message.clone().unwrap_or_default()))
                    .number_of_lines(1)
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(muted_fg())
            },
        ))
        .child(section("工具", vec![tools]))
        .child(section("字段", vec![fields]))
        // 封面只有一行，行首标签就是标题，不再另起节标题。
        .child(cover_mode_row(draft));
    view()
        .flex_1()
        .min_h(px(0.0))
        .child(panel_header(
            vec![
                text("批量修改")
                    .text_size(px(15.0))
                    .line_height(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(fg())
                    .into_element(),
                text(format!("已选 {m} 首"))
                    .flex_1()
                    .min_w(px(0.0))
                    .number_of_lines(1)
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(muted_fg())
                    .into_element(),
                text_button(
                    Variant::Default,
                    Size::Xs,
                    None,
                    format!("应用到 {m} 首"),
                    move || {
                        apply_batch(
                            paths.clone(),
                            selected_entries.clone(),
                            displayed,
                            rows,
                            selected,
                            draft,
                        )
                    },
                )
                .accessibility_label("应用批量修改")
                // 模板写错时不让按（报错已在重命名行下方标红）。
                .disabled(move || draft.with(|d| d.applying) || template_err.get().is_some())
                .into_element(),
            ],
            close,
        ))
        .child(scroll().key("batch").flex_1().min_h(px(0.0)).child(form))
        .child(when(
            move || rename_open.get(),
            move || rename_dialog(draft, preview, template_err, rename_open),
        ))
}

/// 单首曲目的现有索引字段值（`TrackEntry` 兜底）。
pub(crate) fn entry_field_of(t: &TrackEntry, field: TagField) -> String {
    match field {
        TagField::Title => t.title.clone(),
        TagField::Artist => t.artist.clone(),
        TagField::Album => t.album.clone(),
        TagField::AlbumArtist => t.meta.album_artist.clone(),
        TagField::Genre => t.genres.join("; "),
        TagField::Year => t.year.map(|n| n.to_string()).unwrap_or_default(),
        TagField::Track => t.track.map(|n| n.to_string()).unwrap_or_default(),
        TagField::TrackTotal => t.track_total.map(|n| n.to_string()).unwrap_or_default(),
        TagField::Disc => t.disk.map(|n| n.to_string()).unwrap_or_default(),
        TagField::DiscTotal => t.disk_total.map(|n| n.to_string()).unwrap_or_default(),
        TagField::Composer => t.composers.join("; "),
        TagField::Conductor => t.conductor.clone().unwrap_or_default(),
        TagField::Lyricist => t.lyricist.clone().unwrap_or_default(),
        TagField::Publisher => t.publisher.clone().unwrap_or_default(),
        TagField::Copyright => t.copyright.clone().unwrap_or_default(),
        TagField::Comment => t.comment.clone().unwrap_or_default(),
        TagField::Rating => t.rating.map(|r| r.to_string()).unwrap_or_default(),
        TagField::Lyrics => t.lyrics.clone().unwrap_or_default(),
        TagField::TrackGain => t
            .replaygain
            .track_gain
            .map(|g| format!("{g:.2}"))
            .unwrap_or_default(),
        TagField::TrackPeak => t
            .replaygain
            .track_peak
            .map(|p| format!("{p:.6}"))
            .unwrap_or_default(),
        TagField::AlbumGain => t
            .replaygain
            .album_gain
            .map(|g| format!("{g:.2}"))
            .unwrap_or_default(),
        TagField::AlbumPeak => t
            .replaygain
            .album_peak
            .map(|p| format!("{p:.6}"))
            .unwrap_or_default(),
    }
}

/// 三态行：标签 + 模式下拉（保持原值 / 统一设为 / 清空）+ 输入框
/// （仅「统一设为」可写，占位显示共同值或「多个值」）。
pub(crate) fn tri_row(
    field: TagField,
    paths: Rc<Vec<String>>,
    selected_entries: Rc<HashMap<String, TrackEntry>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    draft: Signal<BatchDraft>,
) -> View {
    let label: &'static str = field.label();
    let mode = move || draft.with(|d| d.modes.get(&field).copied().unwrap_or(Tri::Keep));
    // 占位：全部选中行同值显示它，否则「多个值」（行加载完会修正）。
    let placeholder = {
        let map = rows.get_untracked();
        let mut it = paths.iter().map(|p| {
            if let Some(row) = map.get(p) {
                return row.fields.get(&field).cloned().unwrap_or_default();
            }
            if let Some(t) = selected_entries.get(p) {
                return entry_field_of(t, field);
            }
            String::new()
        });
        match it.next() {
            Some(first) if it.all(|v| v == first) && !first.is_empty() => first,
            _ => "多个值".to_owned(),
        }
    };
    let initial = untracked(|| {
        draft
            .with(|d| d.values.get(&field).cloned())
            .unwrap_or_default()
    });
    // 输入框 `min_w(TRI_INPUT_MIN_W)` 是折行门槛：控件列放不下「下拉 + 输入框」
    // 时输入框落到下一行（占满整行），不再被面板右缘裁掉。
    wrap_row(
        label,
        vec![
            select(
                move || {
                    match mode() {
                        Tri::Keep => "保持原值",
                        Tri::Set => "统一设为",
                        Tri::Clear => "清空",
                    }
                    .to_owned()
                },
                label,
                vec![
                    SelectOption::new("保持原值", move || {
                        draft.update(|d| {
                            d.modes.insert(field, Tri::Keep);
                        })
                    }),
                    SelectOption::new("统一设为", move || {
                        draft.update(|d| {
                            d.modes.insert(field, Tri::Set);
                        })
                    }),
                    SelectOption::new("清空", move || {
                        draft.update(|d| {
                            d.modes.insert(field, Tri::Clear);
                        })
                    }),
                ],
                112.0,
            )
            .into_element(),
            input_field(true)
                .flex_1()
                .min_w(px(TRI_INPUT_MIN_W))
                .label(label)
                .placeholder(placeholder)
                .default_value(initial)
                .readonly(move || mode() != Tri::Set || draft.with(|d| d.applying))
                .on_change(move |t: &str| {
                    draft.update(|d| {
                        d.values.insert(field, t.to_owned());
                    })
                })
                .into_element(),
        ],
    )
}

/// 三态行输入框的最小宽（也是折行门槛）。
const TRI_INPUT_MIN_W: f32 = 120.0;
