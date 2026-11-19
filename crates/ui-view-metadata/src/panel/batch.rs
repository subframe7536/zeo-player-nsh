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

/// 批量草稿：三态字段与封面改动，随「应用」一并写回——没有「暂存」这一步。
/// 乱码修复与模板重命名不在草稿里：它们是工具栏入口的独立弹窗，各自预览、
/// 各自应用（见 `tools.rs` / `batch_rename.rs`）。
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct BatchDraft {
    pub(crate) modes: HashMap<TagField, Tri>,
    pub(crate) values: HashMap<TagField, String>,
    pub(crate) cover: CoverMode,
    pub(crate) applying: bool,
    pub(crate) message: Option<String>,
}

impl BatchDraft {
    /// 草稿里有没有要写的改动（「应用到 N 首」可按与否）：三态非
    /// 「保持原值」、封面不是「保持」。
    pub(crate) fn has_changes(&self) -> bool {
        self.modes.values().any(|m| *m != Tri::Keep) || self.cover != CoverMode::Keep
    }
}

pub(crate) fn batch_panel(
    paths: Rc<Vec<String>>,
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    close: Rc<dyn Fn()>,
) -> View {
    let draft = signal(BatchDraft::default());
    // 应用成功的反馈计数器：非 0 时「应用到 N 首」钮显示 ✓（见顶栏按钮）。
    let applied = signal(0u64);
    let m = paths.len();

    // 导出预览只取所选里的前几首（显示序）；全量遍历在导出时做。
    let path_set: Rc<std::collections::HashSet<String>> = Rc::new(paths.iter().cloned().collect());
    let sample = computed(move || Rc::new(displayed.with(|v| sample_of(v, &path_set))));
    // 导出（歌词 / 封面）弹窗：开关与导出选项随面板存活。
    let lyrics_open = signal(false);
    let cover_open = signal(false);
    let lyrics_export = signal(ExportOpts::default());
    let cover_export = signal(ExportOpts::default());
    let export_source = Rc::new(ExportSource {
        displayed,
        rows,
        paths: paths.clone(),
        sample,
    });

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
        .child(section("字段", vec![fields]))
        // 封面只有一行，行首标签就是标题，不再另起节标题。
        .child(cover_mode_row(draft))
        .child(section(
            "文件",
            vec![wrap_row("导出", export_buttons(lyrics_open, cover_open))],
        ));
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
                dynamic(move || {
                    // 应用成功：文字换 ✓ 三秒再还原（`applied` 是计数器，
                    // 三秒内又应用成功只重启计时），不在下方另出「已保存」。
                    if applied.get() != 0 {
                        return shell(Variant::Default, Size::Xs)
                            .accessibility_label("应用成功")
                            .disabled(true)
                            .child(icon_dyn(
                                || Icon::Check,
                                Size::Xs.glyph(),
                                move || foreground(Variant::Default),
                            ))
                            .into_element();
                    }
                    let (paths, entries) = (paths.clone(), selected_entries.clone());
                    text_button(
                        Variant::Default,
                        Size::Xs,
                        None,
                        format!("应用到 {m} 首"),
                        move || {
                            apply_batch(
                                paths.clone(),
                                entries.clone(),
                                displayed,
                                rows,
                                draft,
                                applied,
                            )
                        },
                    )
                    .accessibility_label("应用批量修改")
                    // 草稿里没有任何改动时不让按（点了只会得到「没有要应用的修改」）。
                    .disabled(move || draft.with(|d| d.applying || !d.has_changes()))
                    .into_element()
                })
                .into_element(),
            ],
            close,
        ))
        .child(scroll().key("batch").flex_1().min_h(px(0.0)).child(form))
        .child(when(move || lyrics_open.get(), {
            let source = export_source.clone();
            move || {
                export_dialog(
                    ExportWhat::Lyrics,
                    lyrics_export,
                    source.clone(),
                    lyrics_open,
                )
            }
        }))
        .child(when(move || cover_open.get(), {
            let source = export_source.clone();
            move || export_dialog(ExportWhat::Cover, cover_export, source.clone(), cover_open)
        }))
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
