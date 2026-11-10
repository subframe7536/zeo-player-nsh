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
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CoverMode {
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BatchDraft {
    pub(crate) modes: HashMap<TagField, Tri>,
    pub(crate) values: HashMap<TagField, String>,
    /// 音轨编号：起始输入（默认 "1"）与「同时写入音轨总数」。
    pub(crate) num_start: String,
    pub(crate) num_total: bool,
    /// 已暂存的编号（「编号」按下后）；起始 + 是否写总数。
    pub(crate) staged_num: Option<(u32, bool)>,
    /// 已暂存的大小写格式化（作用于标题 / 歌手 / 专辑）。
    pub(crate) staged_case: Option<CaseOp>,
    pub(crate) cover: CoverMode,
    pub(crate) applying: bool,
    pub(crate) message: Option<String>,
}

pub(crate) fn batch_panel(
    paths: Rc<Vec<String>>,
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    close: Rc<dyn Fn()>,
    open_rename: Rc<dyn Fn()>,
) -> View {
    let draft = signal(BatchDraft {
        modes: HashMap::new(),
        values: HashMap::new(),
        num_start: "1".to_owned(),
        num_total: false,
        staged_num: None,
        staged_case: None,
        cover: CoverMode::Keep,
        applying: false,
        message: None,
    });
    let m = paths.len();
    let busy = move || draft.with(|d| d.applying);
    let mut tools = view().gap(px(8.0));

    // 工具行：音轨连续编号（按当前显示序）与英文大小写——按下只暂存，
    // 「应用」统一写回。
    let case_sel = signal(CaseOp::Title);
    tools = tools
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(label_cell("音轨编号"))
                .child(
                    input_field(true, |i| {
                        i.label("编号起始")
                            .value(move || draft.with(|d| d.num_start.clone()))
                            .readonly(busy)
                            .on_change(move |t: &str| draft.update(|d| d.num_start = t.to_owned()))
                    })
                    .w(px(56.0)),
                )
                .child(checkbox(
                    move || draft.with(|d| d.num_total),
                    || false,
                    "同时写入音轨总数",
                    move |next| draft.update(|d| d.num_total = next),
                ))
                .child(
                    text("写总数")
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(fg()),
                )
                .child(text_button(
                    Variant::Outline,
                    Size::Xs,
                    None,
                    "编号",
                    move || {
                        let start = draft
                            .with(|d| d.num_start.trim().parse::<u32>().ok())
                            .unwrap_or(1);
                        draft.update(|d| {
                            d.staged_num = Some((start, d.num_total));
                            d.message = None;
                        });
                    },
                )),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(label_cell("大小写"))
                .child(select(
                    move || {
                        match case_sel.get() {
                            CaseOp::Title => "首字母大写",
                            CaseOp::Lower => "全部小写",
                            CaseOp::Upper => "全部大写",
                        }
                        .to_owned()
                    },
                    "大小写",
                    vec![
                        SelectOption::new("首字母大写", move || case_sel.set(CaseOp::Title)),
                        SelectOption::new("全部小写", move || case_sel.set(CaseOp::Lower)),
                        SelectOption::new("全部大写", move || case_sel.set(CaseOp::Upper)),
                    ],
                    120.0,
                ))
                .child(text_button(
                    Variant::Outline,
                    Size::Xs,
                    None,
                    "格式化",
                    move || {
                        draft.update(|d| {
                            d.staged_case = Some(case_sel.get());
                            d.message = None;
                        });
                    },
                )),
        )
        // 提示跟控件列对齐（72 标签 + 8 间隔），行内不再挤。
        .child(
            text("作用于标题 / 歌手 / 专辑")
                .pl(px(80.0))
                .text_size(px(11.0))
                .line_height(px(11.0))
                .text_color(faint()),
        )
        .child(when(
            move || draft.with(|d| d.staged_num.is_some() || d.staged_case.is_some()),
            move || {
                let n = m;
                view().gap(px(2.0)).children(
                    [
                        draft.with(|d| d.staged_num).map(|_| {
                            text(format!("将修改音轨号 ×{n}"))
                                .text_size(px(11.0))
                                .line_height(px(11.0))
                                .text_color(muted_fg())
                                .into_element()
                        }),
                        draft.with(|d| d.staged_case).map(|_| {
                            text("将对标题 / 歌手 / 专辑做大小写格式化")
                                .text_size(px(11.0))
                                .line_height(px(11.0))
                                .text_color(muted_fg())
                                .into_element()
                        }),
                    ]
                    .into_iter()
                    .flatten(),
                )
            },
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(label_cell("重命名"))
                .child(text_button(
                    Variant::Outline,
                    Size::Xs,
                    None,
                    "重命名文件…",
                    move || open_rename(),
                )),
        )
        // 提示跟控件列对齐（同上行的大小写提示）。
        .child(
            text("按模板重命名所选文件（文件留在原文件夹）")
                .pl(px(80.0))
                .text_size(px(11.0))
                .line_height(px(11.0))
                .text_color(faint()),
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
    let mut fields = view().gap(px(8.0));
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
        .child(section("封面", vec![cover_mode_row(draft)]));
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
                    {
                        let paths = paths.clone();
                        let selected_entries = selected_entries.clone();
                        move || {
                            apply_batch(
                                paths.clone(),
                                selected_entries.clone(),
                                displayed,
                                rows,
                                draft,
                            )
                        }
                    },
                )
                .disabled(move || draft.with(|d| d.applying))
                .into_element(),
            ],
            close,
        ))
        .child(scroll().key("batch").flex_1().min_h(px(0.0)).child(form))
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
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell(label))
        .child(select(
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
        ))
        .child(
            input_field(true, |i| {
                i.label(label)
                    .placeholder(placeholder)
                    .default_value(initial)
                    .readonly(move || mode() != Tri::Set || draft.with(|d| d.applying))
                    .on_change(move |t: &str| {
                        draft.update(|d| {
                            d.values.insert(field, t.to_owned());
                        })
                    })
            })
            .flex_1()
            .min_w(px(0.0)),
        )
}
