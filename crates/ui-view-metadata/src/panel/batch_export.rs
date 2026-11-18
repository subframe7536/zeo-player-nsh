use super::*;

use std::path::Path;

use zeo_metadata::export::{CoverFormat, ExportKind, LyricsFormat, export_files, format_lyrics};
use zeo_ui_components::form::{dialog_content, dialog_footer, dialog_header, modal};

/// 导出什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportWhat {
    Lyrics,
    Cover,
}

impl ExportWhat {
    fn title(self) -> &'static str {
        match self {
            ExportWhat::Lyrics => "导出歌词",
            ExportWhat::Cover => "导出封面",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            ExportWhat::Lyrics => "歌词",
            ExportWhat::Cover => "封面",
        }
    }

    fn description(self) -> &'static str {
        match self {
            ExportWhat::Lyrics => "把内嵌歌词写成外部文本文件（UTF-8，无 BOM），不改动标签。",
            ExportWhat::Cover => "把内嵌封面保存成图片文件，不改动标签。",
        }
    }
}

/// 导出弹窗的选项（弹窗关掉再开仍保留，随批量面板存活）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExportOpts {
    /// `false` 写在音频旁边、与音频同名；`true` 另存到 `folder`、按模板命名。
    pub(crate) to_folder: bool,
    pub(crate) folder: String,
    /// 另存时的文件名模板（占位符同重命名）。
    pub(crate) template: String,
    /// 目标已存在时覆盖（默认跳过）。
    pub(crate) overwrite: bool,
    /// 歌词：写成 `.txt`（否则 `.lrc`）。
    pub(crate) txt: bool,
    pub(crate) strip_timestamps: bool,
    pub(crate) remove_blank_lines: bool,
    /// 封面：图片格式。
    pub(crate) cover_format: CoverFormat,
    pub(crate) running: bool,
    pub(crate) result: Option<String>,
}

impl Default for ExportOpts {
    fn default() -> Self {
        Self {
            to_folder: false,
            folder: String::new(),
            template: "{artist} - {title}".to_owned(),
            overwrite: false,
            txt: false,
            strip_timestamps: false,
            remove_blank_lines: false,
            cover_format: CoverFormat::Original,
            running: false,
            result: None,
        }
    }
}

impl ExportOpts {
    fn kind(&self, what: ExportWhat) -> ExportKind {
        match what {
            ExportWhat::Lyrics => ExportKind::Lyrics {
                ext: if self.txt { "txt" } else { "lrc" },
                format: self.lyrics_format(),
            },
            ExportWhat::Cover => ExportKind::Cover(self.cover_format),
        }
    }

    fn lyrics_format(&self) -> LyricsFormat {
        LyricsFormat {
            strip_timestamps: self.strip_timestamps,
            remove_blank_lines: self.remove_blank_lines,
        }
    }

    /// 另存模式的校验：要选了文件夹、模板能解析。
    fn folder_error(&self) -> Option<String> {
        if !self.to_folder {
            return None;
        }
        if self.folder.trim().is_empty() {
            return Some("请先选择导出文件夹".to_owned());
        }
        template_error(&self.template).or_else(|| {
            self.template
                .trim()
                .is_empty()
                .then(|| "文件名模板不能为空".to_owned())
        })
    }
}

/// 面板里打开某个导出弹窗的按钮行（歌词 / 封面各一颗）。
pub(crate) fn export_buttons(
    lyrics_open: Signal<bool>,
    cover_open: Signal<bool>,
) -> Vec<AnyElement> {
    [
        (ExportWhat::Lyrics, lyrics_open),
        (ExportWhat::Cover, cover_open),
    ]
    .into_iter()
    .map(|(what, open)| {
        text_button(
            Variant::Outline,
            Size::Sm,
            None,
            format!("{}…", what.title()),
            move || open.set(true),
        )
        .into_element()
    })
    .collect()
}

/// 带文字的复选框（点框触发，文字只是说明）。
pub(crate) fn check_with_label(
    label: &'static str,
    get: impl Fn() -> bool + 'static,
    set: impl Fn(bool) + 'static,
) -> AnyElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(checkbox(get, || false, label, set))
        .child(
            text(label)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(fg()),
        )
        .into_element()
}

/// 导出弹窗的材料：所选曲目（显示序）与它们的字段来源。
pub(crate) struct ExportSource {
    pub(crate) displayed: Memo<Rc<Vec<TrackEntry>>>,
    pub(crate) rows: Signal<HashMap<String, Arc<MetaRow>>>,
    pub(crate) paths: Rc<Vec<String>>,
    /// 所选前几首（预览用）。
    pub(crate) sample: Memo<Rc<Vec<TrackEntry>>>,
}

impl ExportSource {
    /// 所选曲目按当前显示序。
    fn ordered(&self) -> Vec<TrackEntry> {
        let set: std::collections::HashSet<&str> = self.paths.iter().map(String::as_str).collect();
        self.displayed.with_untracked(|v| {
            v.iter()
                .filter(|t| set.contains(t.path.as_str()))
                .cloned()
                .collect()
        })
    }
}

/// 另存模式下一首的文件名主干（模板渲染；同目录模式用不上）。
fn folder_stem(
    template: &rename::Template,
    t: &TrackEntry,
    rows: &HashMap<String, Arc<MetaRow>>,
) -> String {
    let stem = Path::new(&t.path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    template.render(&effective_fields(t, rows, None), &stem)
}

/// 预览：前几首导出后的文件名；歌词再附第一首的整理效果。
fn target_lines(
    what: ExportWhat,
    o: &ExportOpts,
    sample: &[TrackEntry],
    rows: &HashMap<String, Arc<MetaRow>>,
    first_lyrics: Option<&str>,
) -> Vec<PreviewLine> {
    let ext = match what {
        ExportWhat::Lyrics => if o.txt { "txt" } else { "lrc" }.to_owned(),
        ExportWhat::Cover => match o.cover_format {
            CoverFormat::Original => "（原格式）".to_owned(),
            CoverFormat::Jpeg => "jpg".to_owned(),
            CoverFormat::Png => "png".to_owned(),
            CoverFormat::Webp => "webp".to_owned(),
        },
    };
    let template = if o.to_folder {
        match rename::Template::parse(&o.template) {
            Ok(t) => Some(t),
            Err(message) => return vec![PreviewLine::bad(message)],
        }
    } else {
        None
    };
    let mut lines: Vec<PreviewLine> = sample
        .iter()
        .map(|t| {
            let stem = match &template {
                Some(tpl) => folder_stem(tpl, t, rows),
                None => Path::new(&t.path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            };
            PreviewLine::ok(format!("{} → {stem}.{ext}", t.file_name))
        })
        .collect();
    if what == ExportWhat::Lyrics
        && let Some(raw) = first_lyrics
    {
        let formatted = format_lyrics(raw, o.lyrics_format());
        lines.extend(
            formatted
                .lines()
                .take(4)
                .map(|l| PreviewLine::ok(format!("  {l}"))),
        );
    }
    lines
}

/// 导出弹窗：位置（同目录同名 / 另存文件夹 + 文件名模板）、格式选项、
/// 覆盖开关、效果预览与执行结果。导出不改标签，也不进「应用」。
pub(crate) fn export_dialog(
    what: ExportWhat,
    opts: Signal<ExportOpts>,
    source: Rc<ExportSource>,
    open: Signal<bool>,
) -> AnyElement {
    let close = move || open.set(false);
    let busy = move || opts.with(|o| o.running);
    // 歌词整理效果拿第一首当样本（弹窗每次打开读一次内嵌歌词）。
    let first_lyrics: Option<String> = (what == ExportWhat::Lyrics)
        .then(|| {
            source
                .sample
                .with_untracked(|s| s.first().map(|t| t.path.clone()))
                .and_then(|p| zeo_metadata::read_lyrics(Path::new(&p)))
        })
        .flatten();
    let preview = {
        let source = source.clone();
        move || {
            let o = opts.get();
            let rows = source.rows.get();
            let sample = source.sample.get();
            target_lines(what, &o, &sample, &rows, first_lyrics.as_deref())
        }
    };

    let pick_folder = move || {
        spawn(async move {
            let picked = blocking(|| {
                rfd::FileDialog::new()
                    .set_title("选择导出文件夹")
                    .pick_folder()
            })
            .await
            .ok()
            .flatten();
            if let Some(dir) = picked {
                opts.update(|o| o.folder = dir.to_string_lossy().into_owned());
            }
        });
    };

    let run = {
        let source = source.clone();
        move || {
            let o = opts.get_untracked();
            if o.running {
                return;
            }
            if let Some(message) = o.folder_error() {
                opts.update(|o| o.result = Some(message));
                return;
            }
            let tracks = source.ordered();
            let jobs: Vec<(String, String)> = if o.to_folder {
                let rows = source.rows.get_untracked();
                // 模板已在 `folder_error` 校验过。
                let Ok(template) = rename::Template::parse(&o.template) else {
                    return;
                };
                tracks
                    .iter()
                    .map(|t| (t.path.clone(), folder_stem(&template, t, &rows)))
                    .collect()
            } else {
                tracks
                    .iter()
                    .map(|t| (t.path.clone(), String::new()))
                    .collect()
            };
            opts.update(|o| {
                o.running = true;
                o.result = None;
            });
            let kind = o.kind(what);
            let dir = o.to_folder.then(|| o.folder.clone());
            let overwrite = o.overwrite;
            let post = poster();
            std::thread::spawn(move || {
                let summary = export_files(kind, dir.as_deref().map(Path::new), overwrite, &jobs);
                let message = summary.describe(what.noun());
                post.post(move || {
                    opts.update(|o| {
                        o.running = false;
                        o.result = Some(message);
                    })
                });
            });
        }
    };

    let position = wrap_row(
        "位置",
        vec![
            select(
                move || {
                    if opts.with(|o| o.to_folder) {
                        "另存到文件夹"
                    } else {
                        "与音频同目录（同名）"
                    }
                    .to_owned()
                },
                "导出位置",
                vec![
                    SelectOption::new("与音频同目录（同名）", move || {
                        opts.update(|o| o.to_folder = false)
                    }),
                    SelectOption::new("另存到文件夹", move || {
                        opts.update(|o| o.to_folder = true)
                    }),
                ],
                190.0,
            )
            .into_element(),
        ],
    );
    let folder_rows = when(
        move || opts.with(|o| o.to_folder),
        move || {
            view()
                .gap(px(10.0))
                .child(wrap_row(
                    "文件夹",
                    vec![
                        text_button(Variant::Outline, Size::Sm, None, "选择文件夹…", pick_folder)
                            .into_element(),
                        text(move || {
                            let folder = opts.with(|o| o.folder.clone());
                            if folder.is_empty() {
                                "未选择".to_owned()
                            } else {
                                folder
                            }
                        })
                        .flex_1()
                        .min_w(px(0.0))
                        .number_of_lines(1)
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(muted_fg())
                        .into_element(),
                    ],
                ))
                .child(wrap_row(
                    "文件名",
                    vec![
                        input_field(true)
                            .flex_1()
                            .min_w(px(160.0))
                            .label("导出文件名模板")
                            .placeholder("{artist} - {title}")
                            .value(move || opts.with(|o| o.template.clone()))
                            .on_change(move |t: &str| {
                                opts.update(|o| t.clone_into(&mut o.template))
                            })
                            .into_element(),
                    ],
                ))
        },
    );
    let format_rows: Vec<AnyElement> = match what {
        ExportWhat::Lyrics => vec![
            wrap_row(
                "格式",
                vec![
                    select(
                        move || if opts.with(|o| o.txt) { "TXT" } else { "LRC" }.to_owned(),
                        "歌词文件格式",
                        vec![
                            SelectOption::new("LRC", move || opts.update(|o| o.txt = false)),
                            SelectOption::new("TXT", move || opts.update(|o| o.txt = true)),
                        ],
                        96.0,
                    )
                    .into_element(),
                ],
            )
            .into_element(),
            wrap_row(
                "整理",
                vec![
                    check_with_label(
                        "去除时间戳",
                        move || opts.with(|o| o.strip_timestamps),
                        move |v| {
                            opts.update(|o| {
                                o.strip_timestamps = v;
                                // 去掉时间戳后 LRC 没有意义，顺手切成 TXT。
                                o.txt = o.txt || v;
                            })
                        },
                    ),
                    check_with_label(
                        "去除空行",
                        move || opts.with(|o| o.remove_blank_lines),
                        move |v| opts.update(|o| o.remove_blank_lines = v),
                    ),
                ],
            )
            .into_element(),
        ],
        ExportWhat::Cover => vec![
            wrap_row(
                "图片格式",
                vec![
                    select(
                        move || opts.with(|o| o.cover_format.label()).to_owned(),
                        "封面图片格式",
                        CoverFormat::ALL
                            .into_iter()
                            .map(|f| {
                                SelectOption::new(f.label(), move || {
                                    opts.update(|o| o.cover_format = f)
                                })
                            })
                            .collect(),
                        150.0,
                    )
                    .into_element(),
                ],
            )
            .into_element(),
        ],
    };
    let overwrite = wrap_row(
        "已有文件",
        vec![check_with_label(
            "覆盖",
            move || opts.with(|o| o.overwrite),
            move |v| opts.update(|o| o.overwrite = v),
        )],
    );

    let content = dialog_content(520.0)
        .child(dialog_header(what.title(), Some(what.description())))
        .child(
            view()
                .gap(px(10.0))
                .child(position)
                .child(folder_rows)
                .children(format_rows)
                .child(overwrite),
        )
        .child(
            view()
                .gap(px(4.0))
                .child(
                    text("预览")
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(muted_fg()),
                )
                .child(preview_list(preview)),
        )
        .child(when(
            move || opts.with(|o| o.result.is_some()),
            move || {
                text(move || opts.with(|o| o.result.clone().unwrap_or_default()))
                    .text_size(px(12.0))
                    .text_color(muted_fg())
            },
        ))
        .child(
            dialog_footer()
                .child(text_button(
                    Variant::Ghost,
                    Size::Default,
                    None,
                    "完成",
                    close,
                ))
                .child(
                    text_button(
                        Variant::Default,
                        Size::Default,
                        None,
                        format!("导出 {} 首", source.paths.len()),
                        run,
                    )
                    .accessibility_label(format!("开始{}", what.title()))
                    .disabled(busy),
                ),
        );
    modal(content, close)
}
