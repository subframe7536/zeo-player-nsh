//! 批量自动匹配弹窗（元数据视图工具栏入口，作用于当前所选曲目）：
//! 单弹窗内完成配置（数据源 / 智能聚合 / 严格匹配）、后台匹配与候选展开微调、
//! 写入字段与覆盖策略选择，并经 `store::apply_batch_matches` 原子写回且记录标签历史。

use std::cell::RefCell;
use std::collections::HashSet;

use zeo_matcher::{
    BatchConfig, BatchItemResult, BatchMatchStatus, BatchProgress, BatchTrackInput, MatchCandidate,
    RankedCandidate,
};
use zeo_types::config::{MatchFields, MatchPrefs, MatchSource, MatchStrictness, WritePolicy};
use zeo_ui_components::form::{dialog_content, dialog_footer, dialog_header, modal, tab_list};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
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

/// 弹窗表单用的紧凑开关（`30×18` 轨道 + `12×12` 滑块），高度与 16pt checkbox / 13pt 文字齐平。
fn compact_switch(
    checked: impl Fn() -> bool + 'static,
    label: impl Into<SharedString>,
    on_change: impl Fn(bool) + 'static,
) -> nsh_base::SwitchElement {
    nsh_base::switch()
        .checked(checked)
        .accessibility_label(label.into())
        .on_change(move |next: &bool| on_change(*next))
        .track_style(|st| {
            st.w(px(30.0))
                .h(px(18.0))
                .p(px(2.0))
                .rounded_full()
                .bg(muted())
                .border_1()
                .border_color(border())
                .transition_colors()
                .duration(150.0)
                .checked(|st| st.bg(primary()).border_color(primary()))
        })
        .thumb_style(|st| {
            st.size(px(12.0))
                .rounded_full()
                .bg(AdaptiveColor::rgb(0xffffff, 0xffffff))
                .shadow_sm()
                .transition_transform()
                .duration(150.0)
                .checked(|st| st.translate_x(px(12.0)))
        })
}

fn switch_row(
    label: &'static str,
    checked: impl Fn() -> bool + 'static,
    on_change: impl Fn(bool) + 'static,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(compact_switch(checked, label, on_change))
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

/// 格式化毫秒时长为 `m:ss`。
fn format_duration_ms(ms: u64) -> String {
    let total_secs = (ms + 500) / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{mins}:{secs:02}")
}

/// 格式化时长差（毫秒），不足 1 秒不显示。
fn format_diff_ms(diff: i64) -> Option<String> {
    if diff.abs() < 1000 {
        return None;
    }
    let secs = (diff as f64 / 1000.0).round() as i64;
    if secs > 0 {
        Some(format!("+{secs}s"))
    } else {
        Some(format!("{secs}s"))
    }
}

/// 候选下拉项内的小徽章（来源、最佳、时长一致），与单曲匹配卡片同款。
fn candidate_badge(label: impl Into<SharedString>, accent: bool) -> View {
    view()
        .px(px(5.0))
        .py(px(1.0))
        .rounded(px(4.0))
        .bg(move || {
            if accent {
                primary().opacity(0.14)
            } else {
                muted()
            }
        })
        .child(
            text(label.into())
                .text_size(px(10.0))
                .line_height(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if accent { primary() } else { muted_fg() }),
        )
}

/// 候选摘要单行文本（下拉触发器与无障碍项标签用）。
fn candidate_summary(rc: &RankedCandidate) -> String {
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
}

/// 按需懒加载某行候选的缩略图（尊重全局 `show_covers` 开关，已请求过的去重）。
fn lazy_load_thumbs(
    candidates: &[RankedCandidate],
    thumbs: Signal<HashMap<String, String>>,
    requested: &Rc<RefCell<HashSet<String>>>,
    temp_dir: &Arc<PathBuf>,
    alive: &Arc<AtomicBool>,
) {
    if !store::covers_on() {
        return;
    }
    let mut req = requested.borrow_mut();
    for item in candidates {
        let Some(url) = item.candidate.cover_thumb_url.clone() else {
            continue;
        };
        let key = format!("{:?}-{}", item.candidate.source, item.candidate.source_id);
        if !req.insert(key.clone()) {
            continue;
        }
        let alive_t = alive.clone();
        let key_cb = key.clone();
        store::download_cover_temp(
            url,
            temp_dir.as_ref().clone(),
            format!("thumb-{key}"),
            move |dl| {
                if !alive_t.load(Ordering::Relaxed) {
                    return;
                }
                if let Ok((path, _, _)) = dl {
                    let p = path.to_string_lossy().into_owned();
                    thumbs.update(|m| {
                        m.insert(key_cb, p);
                    });
                }
            },
        );
    }
}

/// 批量行内展开后的候选列表项（与单曲匹配卡片同款：封面缩略图 + 标题/平台/最佳/时长一致徽章 + 副标题 + 时长差）。
fn candidate_item_row(
    item: RankedCandidate,
    cand_idx: usize,
    selected: bool,
    row_idx: usize,
    thumbs: Signal<HashMap<String, String>>,
    preview_rows: Signal<Vec<PreviewRow>>,
    expanded_rows: Signal<HashSet<usize>>,
) -> AnyElement {
    let cand = item.candidate;
    let key = format!("{:?}-{}", cand.source, cand.source_id);
    let thumb_sig = computed(move || {
        thumbs
            .with(|m| m.get(&key).cloned())
            .map(SharedString::from)
    });
    let sub = {
        let mut parts = Vec::new();
        let a = cand.joined_artists(" / ");
        if !a.is_empty() {
            parts.push(a);
        }
        if let Some(alb) = &cand.album {
            parts.push(alb.clone());
        }
        if let Some(y) = &cand.year {
            parts.push(y.clone());
        }
        parts.join(" · ")
    };
    let dur_label = cand.duration_ms.map(|ms| {
        let base = format_duration_ms(ms);
        if let Some(d) = item.duration_diff_ms.and_then(format_diff_ms) {
            format!("{base} ({d})")
        } else {
            base
        }
    });

    let row = view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .p(px(6.0))
        .rounded(px(RADIUS_MD))
        .bg(move || {
            if selected {
                primary().opacity(0.08)
            } else {
                primary().opacity(0.0)
            }
        })
        .hover(|st| st.bg(hover()))
        .role(AccessibilityRole::Button)
        .label(format!("选择候选 {}", cand.title))
        .on_press(move || {
            preview_rows.update(|l| {
                if let Some(r) = l.get_mut(row_idx)
                    && cand_idx < r.result.candidates.len()
                {
                    r.result.selected = Some(cand_idx);
                    r.checked = true;
                }
            });
            expanded_rows.update(|s| {
                s.remove(&row_idx);
            });
        })
        .child(when(store::covers_on, move || {
            cover(move || thumb_sig.get(), Some(40.0), 6.0)
        }))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            text(cand.title.clone())
                                .flex_shrink(1.0)
                                .min_w(px(0.0))
                                .number_of_lines(1)
                                .text_size(px(13.0))
                                .line_height(px(15.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(fg()),
                        )
                        .child(candidate_badge(cand.source.short_label(), false))
                        .child(item.tags.best.then(|| candidate_badge("最佳", true)))
                        .child(
                            (!item.tags.best && item.tags.duration_close)
                                .then(|| candidate_badge("时长一致", false)),
                        ),
                )
                .child(
                    text(sub)
                        .number_of_lines(1)
                        .text_size(px(12.0))
                        .line_height(px(14.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(dur_label.map(|d| {
            text(d)
                .text_size(px(11.0))
                .line_height(px(13.0))
                .text_color(faint())
        }))
        .child(selected.then(|| icon(Icon::Check, 14.0, primary)));

    if item.duration_far {
        row.opacity(0.45).into_element()
    } else {
        row.into_element()
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
    let stage = signal(Stage::Idle);
    let filter_tab = signal(FilterTab::All);
    let progress_text = signal(format!("已选 {total} 首曲目，配置数据源后点击「开始匹配」"));
    let progress_ratio = signal(0.0f32);
    let preview_rows = signal(Vec::<PreviewRow>::new());
    let expanded_rows = signal(HashSet::<usize>::new());
    let thumbs = signal(HashMap::<String, String>::new());
    let requested_thumbs = Rc::new(RefCell::new(HashSet::<String>::new()));
    let note = signal(Option::<String>::None);

    let temp_dir = Arc::new(zeo_matcher::new_session_temp_dir());
    let alive = Arc::new(AtomicBool::new(true));
    let cancel_flag = signal(store::Same(Arc::new(AtomicBool::new(false))));

    cleanup({
        let alive = alive.clone();
        let temp_dir = temp_dir.clone();
        move || {
            alive.store(false, Ordering::Relaxed);
            cancel_flag.get_untracked().store(true, Ordering::Relaxed);
            zeo_matcher::cleanup_temp_dir(&temp_dir);
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
            expanded_rows.set(HashSet::new());
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
            store::set_match_prefs(p.clone());
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

    let body = preview_view(
        stage,
        total,
        prefs,
        filter_tab,
        progress_text,
        progress_ratio,
        preview_rows,
        expanded_rows,
        thumbs,
        requested_thumbs.clone(),
        temp_dir.clone(),
        alive.clone(),
        entries.clone(),
        start_matching.clone(),
        move || {
            cancel_flag.get_untracked().store(true, Ordering::Relaxed);
        },
    );

    let footer = dynamic(move || {
        let st = stage.get();
        let m = selected_write_count.get();
        let busy = matches!(st, Stage::Matching | Stage::Applying);
        let apply_label = if st == Stage::Applying {
            "正在写回…".to_owned()
        } else {
            format!("写入已选 {m} 首")
        };
        dialog_footer()
            .child(
                text_button(Variant::Outline, Size::Default, None, "取消", close)
                    .disabled(move || stage.get() == Stage::Applying),
            )
            .child(
                text_button(
                    Variant::Default,
                    Size::Default,
                    None,
                    apply_label,
                    apply_selected.clone(),
                )
                .disabled(move || busy || selected_write_count.get() == 0),
            )
            .into_element()
    });

    let content = dialog_content(680.0)
        .gap(px(12.0))
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

fn match_config_card(
    stage: Signal<Stage>,
    total: usize,
    prefs: Signal<MatchPrefs>,
    start_matching: impl Fn() + Clone + 'static,
    stop_matching: impl Fn() + Clone + 'static,
) -> View {
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

    let action_btn = dynamic(move || match stage.get() {
        Stage::Matching => text_button(
            Variant::Outline,
            Size::Sm,
            None,
            "停止匹配",
            stop_matching.clone(),
        )
        .into_element(),
        Stage::Idle => text_button(
            Variant::Default,
            Size::Sm,
            Some(Icon::Smart),
            format!("开始匹配 ({total})"),
            start_matching.clone(),
        )
        .into_element(),
        Stage::Preview | Stage::Applying => text_button(
            Variant::Outline,
            Size::Sm,
            Some(Icon::Refresh),
            "重新匹配",
            start_matching.clone(),
        )
        .disabled(move || stage.get() == Stage::Applying)
        .into_element(),
    });

    view()
        .gap(px(10.0))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(border())
        .bg(muted().opacity(0.3))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(section_title("数据源").w(px(48.0)))
                .child(source_checks.flex_1().min_w(px(0.0))),
        )
        .child(view().h(px(1.0)).w_full().bg(border().opacity(0.5)))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(section_title("策略").w(px(48.0)))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(18.0))
                                .child(switch_row(
                                    "智能聚合",
                                    move || prefs.with(|p| p.batch_smart),
                                    move |v| prefs.update(|p| p.batch_smart = v),
                                ))
                                .child(switch_row(
                                    "严格匹配",
                                    move || {
                                        prefs.with(|p| p.strictness == MatchStrictness::Standard)
                                    },
                                    move |on| {
                                        prefs.update(|p| {
                                            p.strictness = if on {
                                                MatchStrictness::Standard
                                            } else {
                                                MatchStrictness::Relaxed
                                            };
                                        });
                                    },
                                )),
                        ),
                )
                .child(action_btn),
        )
}

fn write_fields_section(prefs: Signal<MatchPrefs>) -> View {
    let field_cell =
        |label: &'static str, get: fn(&MatchFields) -> bool, set: fn(&mut MatchFields, bool)| {
            check_row(
                label,
                move || prefs.with(|p| get(&p.fields)),
                move |v| prefs.update(|p| set(&mut p.fields, v)),
            )
            .flex_1()
            .min_w(px(0.0))
        };

    let row1 = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(field_cell("标题", |f| f.title, |f, v| f.title = v))
        .child(field_cell("艺术家", |f| f.artist, |f, v| f.artist = v))
        .child(field_cell("专辑", |f| f.album, |f, v| f.album = v))
        .child(field_cell(
            "专辑艺术家",
            |f| f.album_artist,
            |f, v| f.album_artist = v,
        ))
        .child(field_cell("年份", |f| f.year, |f, v| f.year = v));

    let row2 = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(field_cell("音轨号", |f| f.track, |f, v| f.track = v))
        .child(field_cell("碟号", |f| f.disc, |f, v| f.disc = v))
        .child(field_cell("封面", |f| f.cover, |f, v| f.cover = v))
        .child(field_cell("歌词", |f| f.lyrics, |f, v| f.lyrics = v))
        .child(view().flex_1().min_w(px(0.0)));

    let options_row = view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(18.0))
        .child(switch_row(
            "覆盖已有字段",
            move || prefs.with(|p| p.write_policy == WritePolicy::Overwrite),
            move |on| {
                prefs.update(|p| {
                    p.write_policy = if on {
                        WritePolicy::Overwrite
                    } else {
                        WritePolicy::FillEmpty
                    };
                });
            },
        ))
        .child(switch_row(
            "封面保存为同名图片文件",
            move || prefs.with(|p| p.fields.cover_sidecar),
            move |v| prefs.update(|p| p.fields.cover_sidecar = v),
        ))
        .child(switch_row(
            "歌词保存为同名 .lrc 文件",
            move || prefs.with(|p| p.fields.lyrics_sidecar),
            move |v| prefs.update(|p| p.fields.lyrics_sidecar = v),
        ));

    view()
        .gap(px(10.0))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(border())
        .bg(muted().opacity(0.3))
        .child(section_title("写入字段"))
        .child(view().gap(px(8.0)).child(row1).child(row2))
        .child(view().h(px(1.0)).w_full().bg(border().opacity(0.6)))
        .child(options_row)
}

#[allow(clippy::too_many_arguments)]
fn preview_view(
    stage: Signal<Stage>,
    total: usize,
    prefs: Signal<MatchPrefs>,
    filter_tab: Signal<FilterTab>,
    progress_text: Signal<String>,
    progress_ratio: Signal<f32>,
    preview_rows: Signal<Vec<PreviewRow>>,
    expanded_rows: Signal<HashSet<usize>>,
    thumbs: Signal<HashMap<String, String>>,
    requested_thumbs: Rc<RefCell<HashSet<String>>>,
    temp_dir: Arc<PathBuf>,
    alive: Arc<AtomicBool>,
    entries: Rc<HashMap<String, TrackEntry>>,
    start_matching: impl Fn() + Clone + 'static,
    stop_matching: impl Fn() + Clone + 'static,
) -> View {
    let counts = computed(move || {
        preview_rows.with(|list| {
            let mut matched = 0usize;
            let mut review = 0usize;
            let mut missed = 0usize;
            for r in list {
                match r.result.status {
                    BatchMatchStatus::Matched => matched += 1,
                    BatchMatchStatus::NeedsReview => review += 1,
                    BatchMatchStatus::NotFound | BatchMatchStatus::Failed => missed += 1,
                }
            }
            (list.len(), matched, review, missed)
        })
    });

    let tab_item = move |tab_id: FilterTab,
                         name: &'static str,
                         pick: fn((usize, usize, usize, usize)) -> usize| {
        let on = move || filter_tab.get() == tab_id;
        nsh_base::tab()
            .selected(on)
            .accessibility_label(name)
            .on_press(move |_: &()| filter_tab.set(tab_id))
            .h_full()
            .px(px(10.0))
            .flex_row()
            .items_center()
            .justify_center()
            .rounded(px(RADIUS_MD))
            .transition_colors()
            .duration(150.0)
            .child(
                text(move || format!("{name} ({})", pick(counts.get())))
                    .text_size(px(13.0))
                    .line_height(px(14.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || if on() { fg() } else { muted_fg() }),
            )
    };

    let selectable_state = computed(move || {
        preview_rows.with(|list| {
            let mut total_cands = 0usize;
            let mut checked_cands = 0usize;
            for r in list {
                if !r.result.candidates.is_empty() {
                    total_cands += 1;
                    if r.checked {
                        checked_cands += 1;
                    }
                }
            }
            (total_cands, checked_cands)
        })
    });

    let toggle_all = move || {
        let (total_cands, checked_cands) = selectable_state.get_untracked();
        if total_cands == 0 {
            return;
        }
        let next = checked_cands < total_cands;
        preview_rows.update(|list| {
            for r in list.iter_mut() {
                if !r.result.candidates.is_empty() {
                    r.checked = next;
                    if next && r.result.selected.is_none() {
                        r.result.selected = Some(0);
                    }
                }
            }
        });
    };

    let tab_bar_right = dynamic(move || {
        let st = stage.get();
        if matches!(st, Stage::Matching | Stage::Applying) {
            return text(move || progress_text.get())
                .number_of_lines(1)
                .text_size(px(12.0))
                .line_height(px(14.0))
                .text_color(muted_fg())
                .into_element();
        }
        let (total_cands, checked_cands) = selectable_state.get();
        if total_cands > 0 {
            let label = if checked_cands < total_cands {
                "全选可写项"
            } else {
                "取消全选"
            };
            return text_button(Variant::Ghost, Size::Xs, None, label, toggle_all).into_element();
        }
        view().into_element()
    });

    view()
        .gap(px(10.0))
        .child(match_config_card(
            stage,
            total,
            prefs,
            start_matching,
            stop_matching,
        ))
        .child(when(
            move || matches!(stage.get(), Stage::Matching | Stage::Applying),
            move || {
                view()
                    .h(px(4.0))
                    .w_full()
                    .rounded_full()
                    .bg(muted())
                    .overflow_hidden()
                    .child(
                        view()
                            .h_full()
                            .w(move || relative(progress_ratio.get().clamp(0.0, 1.0)))
                            .bg(primary()),
                    )
            },
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    tab_list()
                        .child(tab_item(FilterTab::All, "全部", |c| c.0))
                        .child(tab_item(FilterTab::Matched, "已命中", |c| c.1))
                        .child(tab_item(FilterTab::NeedsReview, "待确认", |c| c.2))
                        .child(tab_item(FilterTab::Missed, "未命中", |c| c.3)),
                )
                .child(tab_bar_right),
        )
        .child(
            scroll()
                .h(px(248.0))
                .min_h(px(120.0))
                .flex_shrink(1.0)
                .rounded(px(8.0))
                .border_1()
                .border_color(border())
                .child(dynamic(move || {
                    let tab = filter_tab.get();
                    let list = preview_rows.get();
                    let st = stage.get();
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
                            let active_idx = row
                                .result
                                .selected
                                .or_else(|| (!row.result.candidates.is_empty()).then_some(0));
                            let summary = active_idx
                                .and_then(|i| row.result.candidates.get(i))
                                .map(candidate_summary)
                                .unwrap_or_else(|| {
                                    row.result
                                        .error
                                        .clone()
                                        .unwrap_or_else(|| "无匹配结果".to_owned())
                                });
                            let has_cands = !row.result.candidates.is_empty();
                            let st_text = status_label(row.result.status);
                            let checked_now = row.checked;
                            let cands_owned = row.result.candidates.clone();

                            let toggle_expand = {
                                let cands = cands_owned.clone();
                                let requested = requested_thumbs.clone();
                                let temp_dir = temp_dir.clone();
                                let alive = alive.clone();
                                move || {
                                    if !has_cands {
                                        return;
                                    }
                                    let mut opened = false;
                                    expanded_rows.update(|s| {
                                        if s.contains(&idx) {
                                            s.remove(&idx);
                                        } else {
                                            s.insert(idx);
                                            opened = true;
                                        }
                                    });
                                    if opened {
                                        lazy_load_thumbs(
                                            &cands, thumbs, &requested, &temp_dir, &alive,
                                        );
                                    }
                                }
                            };

                            let toggle_summary = toggle_expand.clone();
                            let toggle_btn = toggle_expand.clone();

                            let header = view()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .px(px(10.0))
                                .py(px(7.0))
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
                                        .on_press(toggle_summary)
                                        .child(
                                            text(file_name)
                                                .number_of_lines(1)
                                                .text_size(px(13.0))
                                                .line_height(px(15.0))
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
                                .child(has_cands.then(|| {
                                    tipped(
                                        icon_button(
                                            move || {
                                                if expanded_rows.with(|s| s.contains(&idx)) {
                                                    Icon::ChevronUp
                                                } else {
                                                    Icon::ChevronDown
                                                }
                                            },
                                            12.0,
                                            "展开或收起候选",
                                            muted_fg,
                                            toggle_btn,
                                        ),
                                        move || {
                                            SharedString::from(
                                                if expanded_rows.with(|s| s.contains(&idx)) {
                                                    "收起候选"
                                                } else {
                                                    "展开候选"
                                                },
                                            )
                                        },
                                    )
                                }));

                            let content = view()
                                .pl(px(34.0))
                                .pr(px(10.0))
                                .pb(px(8.0))
                                .pt(px(2.0))
                                .child(
                                    view()
                                        .p(px(4.0))
                                        .gap(px(2.0))
                                        .rounded(px(8.0))
                                        .bg(muted().opacity(0.35))
                                        .children(
                                            cands_owned
                                                .into_iter()
                                                .enumerate()
                                                .map(|(ci, item)| {
                                                    candidate_item_row(
                                                        item,
                                                        ci,
                                                        active_idx == Some(ci),
                                                        idx,
                                                        thumbs,
                                                        preview_rows,
                                                        expanded_rows,
                                                    )
                                                })
                                                .collect::<Vec<_>>(),
                                        ),
                                );

                            view()
                                .border_b_1()
                                .border_color(border().opacity(0.5))
                                .child(
                                    nsh_base::collapsible()
                                        .open(move || {
                                            has_cands && expanded_rows.with(|s| s.contains(&idx))
                                        })
                                        .reveal(true)
                                        .child(header)
                                        .content(content),
                                )
                                .into_element()
                        })
                        .collect();
                    if items.is_empty() {
                        let (headline, subline) = match st {
                            Stage::Idle => (
                                format!("已选 {total} 首曲目"),
                                "配置上方数据源后点击「开始匹配」开始在线检索",
                            ),
                            Stage::Matching => (
                                "正在搜索在线音乐库…".to_owned(),
                                "匹配结果将随进度逐条显示在此处",
                            ),
                            Stage::Preview | Stage::Applying => (
                                "当前分类下暂无曲目".to_owned(),
                                "可切换其他筛选标签或调整数据源重新匹配",
                            ),
                        };
                        return view()
                            .h(px(244.0))
                            .w_full()
                            .items_center()
                            .justify_center()
                            .gap(px(6.0))
                            .child(icon(Icon::Smart, 22.0, faint))
                            .child(
                                text(headline)
                                    .text_size(px(13.0))
                                    .line_height(px(15.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(fg()),
                            )
                            .child(
                                text(subline)
                                    .text_size(px(12.0))
                                    .line_height(px(14.0))
                                    .text_color(muted_fg()),
                            )
                            .into_element();
                    }
                    view().children(items).into_element()
                })),
        )
        .child(write_fields_section(prefs))
}
