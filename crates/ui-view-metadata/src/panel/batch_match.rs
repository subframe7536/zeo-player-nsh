//! 批量自动匹配弹窗（元数据视图工具栏入口，作用于当前所选曲目）：
//! 三阶段交互：配置（数据源/严格度/写入策略/字段） → 后台匹配与预览微调 →
//! 经 `store::apply_batch_matches` / `save_tags_batch` 原子写回并记录标签历史。

use zeo_matcher::{
    BatchConfig, BatchItemResult, BatchMatchStatus, BatchProgress, BatchTrackInput, MatchCandidate,
};
use zeo_types::config::{MatchFields, MatchPrefs, MatchSource, MatchStrictness, WritePolicy};
use zeo_ui_components::form::{dialog_content, dialog_footer, dialog_header, modal};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Config,
    Matching,
    Preview,
    Applying,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterTab {
    All,
    Matched,
    NeedsReview,
    Missed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewRow {
    result: BatchItemResult,
    checked: bool,
}

/// 工具栏「批量自动匹配」入口按钮：选中至少一首才可点。
pub(crate) fn match_button(open: Signal<bool>, can: Memo<bool>) -> AnyElement {
    tipped(
        shell(Variant::Outline, Size::IconSm)
            .accessibility_label("批量自动匹配")
            .disabled(move || !can.get())
            .on_press(move |_: &()| open.set(true))
            .child(icon(Icon::Smart, 14.0, muted_fg)),
        "批量自动匹配",
    )
    .into_element()
}

fn check_row(
    label: &'static str,
    checked: impl Fn() -> bool + Copy + 'static,
    on_toggle: impl Fn(bool) + 'static,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(checkbox(checked, || false, label, on_toggle))
        .child(
            text(label)
                .text_size(px(13.0))
                .line_height(px(14.0))
                .text_color(fg()),
        )
}

fn section_title(label: &'static str) -> Text {
    text(label)
        .text_size(px(12.0))
        .line_height(px(12.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(muted_fg())
}

fn status_label(status: BatchMatchStatus) -> &'static str {
    match status {
        BatchMatchStatus::Matched => "已命中",
        BatchMatchStatus::NeedsReview => "待确认",
        BatchMatchStatus::NotFound => "未命中",
        BatchMatchStatus::Failed => "失败",
    }
}

/// 批量自动匹配弹窗。
pub(crate) fn batch_match_dialog(
    order: Rc<Vec<String>>,
    entries: Rc<HashMap<String, TrackEntry>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    open: Signal<bool>,
) -> AnyElement {
    let total = order.len();
    let prefs = signal(untracked(store::match_prefs));
    let stage = signal(Stage::Config);
    let filter_tab = signal(FilterTab::All);
    let progress_text = signal(String::new());
    let progress_ratio = signal(0.0f32);
    let preview_rows = signal(Vec::<PreviewRow>::new());
    let note = signal(Option::<String>::None);

    let alive = Arc::new(AtomicBool::new(true));
    let cancel_flag = signal(store::Same(Arc::new(AtomicBool::new(false))));

    cleanup({
        let alive = alive.clone();
        move || {
            alive.store(false, Ordering::Relaxed);
            cancel_flag.get_untracked().store(true, Ordering::Relaxed);
        }
    });

    let close = move || {
        cancel_flag.get_untracked().store(true, Ordering::Relaxed);
        open.set(false);
    };

    let start_matching = {
        let order = order.clone();
        let entries = entries.clone();
        let alive = alive.clone();
        move || {
            let p = prefs.get_untracked();
            let active_sources: Vec<MatchSource> = p
                .batch_sources
                .iter()
                .copied()
                .filter(|s| *s != MatchSource::Smart)
                .collect();
            if active_sources.is_empty() {
                note.set(Some("请至少选择一个数据源".to_owned()));
                return;
            }
            store::set_match_prefs(p.clone());

            let map = rows.get_untracked();
            let inputs: Vec<BatchTrackInput> = order
                .iter()
                .filter_map(|path| {
                    let t = entries.get(path)?;
                    let title = current_value(t, &map, TagField::Title);
                    let artist = current_value(t, &map, TagField::Artist);
                    let album = current_value(t, &map, TagField::Album);
                    let duration_ms =
                        (t.duration > 0.0).then(|| (t.duration * 1000.0).round() as u64);
                    Some(BatchTrackInput {
                        path: t.path.clone(),
                        file_name: t.file_name.clone(),
                        title,
                        artist,
                        album,
                        duration_ms,
                    })
                })
                .collect();

            let cancel = Arc::new(AtomicBool::new(false));
            cancel_flag.set(store::Same(cancel.clone()));
            preview_rows.set(Vec::new());
            progress_ratio.set(0.0);
            progress_text.set(format!("准备匹配 0 / {} 首…", inputs.len()));
            note.set(None);
            stage.set(Stage::Matching);

            let cfg = BatchConfig {
                smart_aggregate: p.batch_smart,
                sources: active_sources,
                strictness: p.strictness,
                concurrency: 2,
                min_delay_ms: 180,
            };

            let alive_prog = alive.clone();
            let alive_done = alive.clone();
            store::start_batch_match(
                inputs,
                cfg,
                cancel,
                move |prog: BatchProgress| {
                    if !alive_prog.load(Ordering::Relaxed) {
                        return;
                    }
                    let ratio = if prog.total == 0 {
                        1.0
                    } else {
                        prog.done as f32 / prog.total as f32
                    };
                    progress_ratio.set(ratio);
                    progress_text.set(format!(
                        "已处理 {} / {} · 命中 {} · 待确认 {} · 未命中 {}",
                        prog.done,
                        prog.total,
                        prog.matched,
                        prog.needs_review,
                        prog.not_found + prog.failed
                    ));
                    let checked = prog.latest.status == BatchMatchStatus::Matched;
                    preview_rows.update(|list| {
                        list.push(PreviewRow {
                            result: prog.latest,
                            checked,
                        });
                    });
                },
                move |_all| {
                    if !alive_done.load(Ordering::Relaxed) {
                        return;
                    }
                    progress_ratio.set(1.0);
                    stage.set(Stage::Preview);
                },
            );
        }
    };

    let apply_selected = {
        let alive = alive.clone();
        move || {
            let plans: Vec<(String, MatchCandidate)> = preview_rows.with_untracked(|list| {
                list.iter()
                    .filter(|r| r.checked)
                    .filter_map(|r| {
                        let idx = r.result.selected?;
                        let cand = r.result.candidates.get(idx)?.candidate.clone();
                        Some((r.result.path.clone(), cand))
                    })
                    .collect()
            });
            if plans.is_empty() {
                note.set(Some("没有勾选可写入的匹配项".to_owned()));
                return;
            }
            let p = prefs.get_untracked();
            let sep = store::store().artist_seps.with_untracked(|seps| {
                seps.first()
                    .map(|s| if s == ";" { "; ".to_owned() } else { s.clone() })
                    .unwrap_or_else(|| "; ".to_owned())
            });
            stage.set(Stage::Applying);
            note.set(None);
            progress_ratio.set(0.0);
            progress_text.set(format!("正在下载封面与歌词 0 / {}…", plans.len()));

            let alive_p = alive.clone();
            let alive_d = alive.clone();
            store::apply_batch_matches(
                plans,
                p.fields,
                p.write_policy,
                sep,
                move |done, total| {
                    if !alive_p.load(Ordering::Relaxed) {
                        return;
                    }
                    progress_ratio.set(if total == 0 {
                        1.0
                    } else {
                        done as f32 / total as f32
                    });
                    progress_text.set(format!("正在准备写盘资源 {done} / {total}…"));
                },
                move |saved| {
                    if !alive_d.load(Ordering::Relaxed) {
                        return;
                    }
                    forget_rows(saved.written.iter(), rows);
                    if saved.errors.is_empty() {
                        open.set(false);
                    } else {
                        stage.set(Stage::Preview);
                        let lines: Vec<String> = saved
                            .errors
                            .iter()
                            .map(|(p, e)| format!("{}：{e}", zeo_ui_views_base::last_segment(p)))
                            .collect();
                        note.set(Some(format!(
                            "已写入 {} 首，{} 首失败：{}",
                            saved.written.len(),
                            saved.errors.len(),
                            brief(&lines)
                        )));
                    }
                },
            );
        }
    };

    let selected_write_count = computed(move || {
        preview_rows.with(|list| {
            list.iter()
                .filter(|r| r.checked && r.result.selected.is_some())
                .count()
        })
    });

    let body = dynamic({
        let entries = entries.clone();
        move || match stage.get() {
            Stage::Config => config_view(prefs).into_element(),
            Stage::Matching | Stage::Preview | Stage::Applying => preview_view(
                stage,
                filter_tab,
                progress_text,
                progress_ratio,
                preview_rows,
                entries.clone(),
            )
            .into_element(),
        }
    });

    let footer = dynamic(move || match stage.get() {
        Stage::Config => dialog_footer()
            .child(text_button(
                Variant::Ghost,
                Size::Default,
                None,
                "取消",
                close,
            ))
            .child(text_button(
                Variant::Default,
                Size::Default,
                Some(Icon::Smart),
                format!("开始匹配 {total} 首"),
                start_matching.clone(),
            ))
            .into_element(),
        Stage::Matching => dialog_footer()
            .child(text_button(
                Variant::Outline,
                Size::Default,
                None,
                "停止匹配",
                move || {
                    cancel_flag.get_untracked().store(true, Ordering::Relaxed);
                },
            ))
            .into_element(),
        Stage::Preview => {
            let m = selected_write_count.get();
            dialog_footer()
                .child(text_button(
                    Variant::Ghost,
                    Size::Default,
                    None,
                    "返回配置",
                    move || stage.set(Stage::Config),
                ))
                .child(text_button(
                    Variant::Outline,
                    Size::Default,
                    None,
                    "取消",
                    close,
                ))
                .child(
                    text_button(
                        Variant::Default,
                        Size::Default,
                        None,
                        format!("写入已选 {m} 首"),
                        apply_selected.clone(),
                    )
                    .disabled(move || selected_write_count.get() == 0),
                )
                .into_element()
        }
        Stage::Applying => dialog_footer()
            .child(
                text_button(Variant::Default, Size::Default, None, "正在写回…", || {})
                    .disabled(true),
            )
            .into_element(),
    });

    let content = dialog_content(680.0)
        .child(
            view()
                .absolute()
                .top(px(8.0))
                .right(px(8.0))
                .child(icon_button(
                    || Icon::Close,
                    12.0,
                    "关闭批量自动匹配",
                    muted_fg,
                    close,
                )),
        )
        .child(dialog_header(
            "批量自动匹配元数据",
            Some("从在线音乐库自动搜索并匹配标签、封面与歌词；先预览确认后再原子写盘，可在标签历史中撤销。"),
        ))
        .child(body)
        .child(when(
            move || note.with(Option::is_some),
            move || {
                text(move || note.with(|n| n.clone().unwrap_or_default()))
                    .number_of_lines(2)
                    .text_size(px(12.0))
                    .text_color(nsh_base::theme().destructive)
            },
        ))
        .child(footer);

    modal(content, close)
}

fn config_view(prefs: Signal<MatchPrefs>) -> View {
    let strict_options = vec![
        SelectOption::new(MatchStrictness::Standard.label(), move || {
            prefs.update(|p| p.strictness = MatchStrictness::Standard);
        }),
        SelectOption::new(MatchStrictness::Relaxed.label(), move || {
            prefs.update(|p| p.strictness = MatchStrictness::Relaxed);
        }),
    ];
    let policy_options = vec![
        SelectOption::new(WritePolicy::FillEmpty.label(), move || {
            prefs.update(|p| p.write_policy = WritePolicy::FillEmpty);
        }),
        SelectOption::new(WritePolicy::Overwrite.label(), move || {
            prefs.update(|p| p.write_policy = WritePolicy::Overwrite);
        }),
    ];

    let source_checks = view().flex_row().flex_wrap().gap(px(12.0)).children(
        MatchSource::PROVIDERS
            .into_iter()
            .map(|src| {
                check_row(
                    src.label(),
                    move || prefs.with(|p| p.batch_sources.contains(&src)),
                    move |on| {
                        prefs.update(|p| {
                            if on {
                                if !p.batch_sources.contains(&src) {
                                    p.batch_sources.push(src);
                                }
                            } else {
                                p.batch_sources.retain(|s| *s != src);
                            }
                        });
                    },
                )
                .into_element()
            })
            .collect::<Vec<_>>(),
    );

    let field_toggle =
        |label: &'static str, get: fn(&MatchFields) -> bool, set: fn(&mut MatchFields, bool)| {
            check_row(
                label,
                move || prefs.with(|p| get(&p.fields)),
                move |v| prefs.update(|p| set(&mut p.fields, v)),
            )
            .into_element()
        };

    let field_checks = view().flex_row().flex_wrap().gap(px(12.0)).children(vec![
        field_toggle("标题", |f| f.title, |f, v| f.title = v),
        field_toggle("艺术家", |f| f.artist, |f, v| f.artist = v),
        field_toggle("专辑", |f| f.album, |f, v| f.album = v),
        field_toggle("专辑艺术家", |f| f.album_artist, |f, v| f.album_artist = v),
        field_toggle("年份", |f| f.year, |f, v| f.year = v),
        field_toggle("音轨号", |f| f.track, |f, v| f.track = v),
        field_toggle("碟号", |f| f.disc, |f, v| f.disc = v),
        field_toggle("封面", |f| f.cover, |f, v| f.cover = v),
        field_toggle("歌词", |f| f.lyrics, |f, v| f.lyrics = v),
    ]);

    let sidecar_checks = view().flex_row().flex_wrap().gap(px(16.0)).children(vec![
        field_toggle(
            "封面保存为同名图片文件",
            |f| f.cover_sidecar,
            |f, v| f.cover_sidecar = v,
        ),
        field_toggle(
            "歌词保存为同名 .lrc 文件",
            |f| f.lyrics_sidecar,
            |f, v| f.lyrics_sidecar = v,
        ),
    ]);

    view()
        .gap(px(14.0))
        .child(
            view()
                .gap(px(8.0))
                .child(section_title("数据源"))
                .child(source_checks)
                .child(check_row(
                    "多源智能聚合（同时搜索全部已选平台取最高分；关闭则按顺序命中即停）",
                    move || prefs.with(|p| p.batch_smart),
                    move |v| prefs.update(|p| p.batch_smart = v),
                )),
        )
        .child(
            view()
                .flex_row()
                .gap(px(16.0))
                .child(
                    view()
                        .flex_1()
                        .gap(px(6.0))
                        .child(section_title("匹配严格度"))
                        .child(select(
                            move || prefs.with(|p| p.strictness.label().to_owned()),
                            "匹配严格度",
                            strict_options,
                            0.0,
                        )),
                )
                .child(
                    view()
                        .flex_1()
                        .gap(px(6.0))
                        .child(section_title("写入策略"))
                        .child(select(
                            move || prefs.with(|p| p.write_policy.label().to_owned()),
                            "写入策略",
                            policy_options,
                            0.0,
                        )),
                ),
        )
        .child(
            view()
                .gap(px(8.0))
                .child(section_title("写入字段"))
                .child(field_checks)
                .child(sidecar_checks),
        )
}

fn preview_view(
    stage: Signal<Stage>,
    filter_tab: Signal<FilterTab>,
    progress_text: Signal<String>,
    progress_ratio: Signal<f32>,
    preview_rows: Signal<Vec<PreviewRow>>,
    entries: Rc<HashMap<String, TrackEntry>>,
) -> View {
    let tab_btn = move |tab: FilterTab, label: &'static str| {
        text_button(
            if filter_tab.get() == tab {
                Variant::Secondary
            } else {
                Variant::Ghost
            },
            Size::Xs,
            None,
            label,
            move || filter_tab.set(tab),
        )
    };

    view()
        .gap(px(10.0))
        .child(
            view()
                .gap(px(6.0))
                .child(
                    text(move || progress_text.get())
                        .text_size(px(12.0))
                        .line_height(px(14.0))
                        .text_color(muted_fg()),
                )
                .child(
                    view()
                        .h(px(6.0))
                        .w_full()
                        .rounded_full()
                        .bg(muted())
                        .overflow_hidden()
                        .child(
                            view()
                                .h_full()
                                .w(move || px(progress_ratio.get().clamp(0.0, 1.0) * 632.0))
                                .bg(primary()),
                        ),
                ),
        )
        .child(when(
            move || stage.get() == Stage::Preview,
            move || {
                dynamic(move || {
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(tab_btn(FilterTab::All, "全部"))
                        .child(tab_btn(FilterTab::Matched, "已命中"))
                        .child(tab_btn(FilterTab::NeedsReview, "待确认"))
                        .child(tab_btn(FilterTab::Missed, "未命中"))
                        .into_element()
                })
            },
        ))
        .child(
            scroll()
                .h(px(300.0))
                .min_h(px(140.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(border())
                .child(dynamic(move || {
                    let tab = filter_tab.get();
                    let list = preview_rows.get();
                    let items: Vec<AnyElement> = list
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| match tab {
                            FilterTab::All => true,
                            FilterTab::Matched => r.result.status == BatchMatchStatus::Matched,
                            FilterTab::NeedsReview => {
                                r.result.status == BatchMatchStatus::NeedsReview
                            }
                            FilterTab::Missed => matches!(
                                r.result.status,
                                BatchMatchStatus::NotFound | BatchMatchStatus::Failed
                            ),
                        })
                        .map(|(idx, row)| {
                            let file_name = entries
                                .get(&row.result.path)
                                .map(|t| t.display_title().to_owned())
                                .unwrap_or_else(|| {
                                    zeo_ui_views_base::last_segment(&row.result.path).to_owned()
                                });
                            let cand = row
                                .result
                                .selected
                                .and_then(|i| row.result.candidates.get(i))
                                .or_else(|| row.result.candidates.first());
                            let summary = if let Some(rc) = cand {
                                let a = rc.candidate.joined_artists(" / ");
                                let alb = rc.candidate.album.as_deref().unwrap_or("");
                                format!(
                                    "[{}] {} - {}{}",
                                    rc.candidate.source.short_label(),
                                    rc.candidate.title,
                                    a,
                                    if alb.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" · {alb}")
                                    }
                                )
                            } else {
                                row.result
                                    .error
                                    .clone()
                                    .unwrap_or_else(|| "无匹配结果".to_owned())
                            };
                            let has_cands = !row.result.candidates.is_empty();
                            let multi_cands = row.result.candidates.len() > 1;
                            let st_text = status_label(row.result.status);
                            let checked_now = row.checked;

                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .px(px(10.0))
                                .py(px(7.0))
                                .border_b_1()
                                .border_color(border().opacity(0.5))
                                .child(checkbox(
                                    move || checked_now,
                                    || false,
                                    "选择写入",
                                    move |next| {
                                        if !has_cands {
                                            return;
                                        }
                                        preview_rows.update(|l| {
                                            if let Some(r) = l.get_mut(idx) {
                                                r.checked = next;
                                                if next && r.result.selected.is_none() {
                                                    r.result.selected = Some(0);
                                                }
                                            }
                                        });
                                    },
                                ))
                                .child(
                                    view()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .gap(px(2.0))
                                        .child(
                                            text(file_name)
                                                .number_of_lines(1)
                                                .text_size(px(13.0))
                                                .line_height(px(14.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(fg()),
                                        )
                                        .child(
                                            text(summary)
                                                .number_of_lines(1)
                                                .text_size(px(12.0))
                                                .line_height(px(14.0))
                                                .text_color(muted_fg()),
                                        ),
                                )
                                .child(
                                    text(st_text)
                                        .text_size(px(11.0))
                                        .line_height(px(13.0))
                                        .text_color(muted_fg()),
                                )
                                .child(multi_cands.then(|| {
                                    text_button(
                                        Variant::Ghost,
                                        Size::Xs,
                                        None,
                                        "换候选",
                                        move || {
                                            preview_rows.update(|l| {
                                                if let Some(r) = l.get_mut(idx)
                                                    && !r.result.candidates.is_empty()
                                                {
                                                    let next = match r.result.selected {
                                                        Some(cur) => {
                                                            (cur + 1) % r.result.candidates.len()
                                                        }
                                                        None => 0,
                                                    };
                                                    r.result.selected = Some(next);
                                                    r.checked = true;
                                                }
                                            });
                                        },
                                    )
                                }))
                                .into_element()
                        })
                        .collect();
                    view().children(items).into_element()
                })),
        )
}
