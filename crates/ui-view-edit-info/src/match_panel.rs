//! 「在线匹配」卡片（嵌入「编辑元数据」弹窗内，交互参考 SPlayer-Next `TagEditorDialog.vue`）：
//! 关键词输入 + 平台下拉 + 「在线匹配」按钮 + 同卡片内展开的候选列表；
//! 点击候选立即回填文本字段，并异步下载封面与歌词，由用户确认微调后再点「保存」。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use zeo_matcher::{MatchCandidate, RankedCandidate, build_query};
use zeo_types::config::MatchSource;

use super::*;

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

/// 卡片内的小徽章（来源、最佳、时长一致）。
fn badge(label: impl Into<SharedString>, accent: bool) -> View {
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

/// 单曲「在线匹配」卡片。
pub(crate) fn match_card(
    draft: Signal<TagDraft>,
    form_gen: Signal<u32>,
    lyrics_gen: Signal<u32>,
) -> View {
    let initial_prefs = untracked(store::match_prefs);
    let source = signal(initial_prefs.single_source);

    let (file_name, duration_ms, init_keyword) = untracked(|| {
        draft.with(|d| {
            let p = Path::new(&d.path);
            let fname = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let dur = zeo_metadata::read_meta(p)
                .ok()
                .map(|m| (m.duration * 1000.0).round() as u64)
                .filter(|&ms| ms > 0);
            let title = d.values.get(&TagField::Title).cloned().unwrap_or_default();
            let artist = d.values.get(&TagField::Artist).cloned().unwrap_or_default();
            let album = d.values.get(&TagField::Album).cloned().unwrap_or_default();
            let q = build_query(&fname, &title, &artist, &album, dur);
            (fname, dur, q.keyword)
        })
    });

    let keyword = signal(init_keyword);
    let searching = signal(false);
    let candidates = signal(Vec::<RankedCandidate>::new());
    let expanded = signal(false);
    let status = signal(Option::<String>::None);
    let thumbs = signal(HashMap::<String, String>::new());

    // 会话级临时目录：放候选缩略图与待写盘高清封面，弹窗卸载即清理。
    let temp_dir = Arc::new(zeo_matcher::new_session_temp_dir());
    let alive = Arc::new(AtomicBool::new(true));
    let search_token = Arc::new(AtomicU64::new(0));
    let lyric_token = Arc::new(AtomicU64::new(0));

    cleanup({
        let alive = alive.clone();
        let temp_dir = temp_dir.clone();
        move || {
            alive.store(false, Ordering::Relaxed);
            zeo_matcher::cleanup_temp_dir(&temp_dir);
        }
    });

    let file_name_shared = Arc::new(file_name);

    let run_search = {
        let alive = alive.clone();
        let temp_dir = temp_dir.clone();
        let search_token = search_token.clone();
        let file_name_shared = file_name_shared.clone();
        move || {
            let kw = keyword.get_untracked().trim().to_owned();
            if kw.is_empty() || searching.get_untracked() {
                return;
            }
            let src = source.get_untracked();
            let tok = search_token.fetch_add(1, Ordering::Relaxed) + 1;
            searching.set(true);
            status.set(None);

            let (title, artist, album) = draft.with_untracked(|d| {
                (
                    d.values.get(&TagField::Title).cloned().unwrap_or_default(),
                    d.values.get(&TagField::Artist).cloned().unwrap_or_default(),
                    d.values.get(&TagField::Album).cloned().unwrap_or_default(),
                )
            });
            let mut query = build_query(&file_name_shared, &title, &artist, &album, duration_ms);
            query.keyword = kw;

            let alive_cb = alive.clone();
            let tok_ref = search_token.clone();
            let dir_cb = temp_dir.clone();
            store::search_metadata(src, query, 10, move |res| {
                if !alive_cb.load(Ordering::Relaxed) || tok_ref.load(Ordering::Relaxed) != tok {
                    return;
                }
                searching.set(false);
                match res {
                    Ok(list) => {
                        if list.is_empty() {
                            candidates.set(Vec::new());
                            expanded.set(false);
                            status.set(Some(
                                "未找到匹配的在线曲目，可尝试调整关键词或切换数据源".to_owned(),
                            ));
                            return;
                        }
                        // 按需下载缩略图（尊重全局 show_covers 开关）。
                        if store::covers_on() {
                            for item in list.iter().take(10) {
                                if let Some(url) = item.candidate.cover_thumb_url.clone() {
                                    let key = format!(
                                        "{:?}-{}",
                                        item.candidate.source, item.candidate.source_id
                                    );
                                    if thumbs.with_untracked(|m| m.contains_key(&key)) {
                                        continue;
                                    }
                                    let alive_t = alive_cb.clone();
                                    let tok_t = tok_ref.clone();
                                    let key_cb = key.clone();
                                    store::download_cover_temp(
                                        url,
                                        dir_cb.as_ref().clone(),
                                        format!("thumb-{key}"),
                                        move |dl| {
                                            if !alive_t.load(Ordering::Relaxed)
                                                || tok_t.load(Ordering::Relaxed) != tok
                                            {
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
                        }
                        candidates.set(list);
                        expanded.set(true);
                    }
                    Err(err) => {
                        status.set(Some(err));
                    }
                }
            });
        }
    };

    let apply_candidate = {
        let alive = alive.clone();
        let temp_dir = temp_dir.clone();
        let lyric_token = lyric_token.clone();
        move |cand: MatchCandidate| {
            let sep = store().artist_seps.with_untracked(|seps| {
                seps.first()
                    .map(|s| if s == ";" { "; ".to_owned() } else { s.clone() })
                    .unwrap_or_else(|| "; ".to_owned())
            });
            let artist_str = cand.joined_artists(&sep);
            let src_label = cand.source.label();

            draft.update(|d| {
                if !cand.title.trim().is_empty() {
                    d.values
                        .insert(TagField::Title, cand.title.trim().to_owned());
                }
                if !artist_str.is_empty() {
                    d.values.insert(TagField::Artist, artist_str);
                }
                if let Some(album) = &cand.album
                    && !album.trim().is_empty()
                {
                    d.values.insert(TagField::Album, album.trim().to_owned());
                }
                if let Some(aa) = &cand.album_artist
                    && !aa.trim().is_empty()
                {
                    d.values.insert(TagField::AlbumArtist, aa.trim().to_owned());
                }
                if let Some(year) = &cand.year
                    && !year.trim().is_empty()
                {
                    d.values.insert(TagField::Year, year.trim().to_owned());
                }
                if let Some(track) = cand.track_number {
                    d.values.insert(TagField::Track, track.to_string());
                }
                if let Some(disc) = cand.disc_number {
                    d.values.insert(TagField::Disc, disc.to_string());
                }
                d.error = None;
            });
            form_gen.update(|g| *g += 1);
            expanded.set(false);
            status.set(Some(format!(
                "已回填来自 {src_label} 的元数据，正在拉取封面与歌词…"
            )));

            let tok = lyric_token.fetch_add(1, Ordering::Relaxed) + 1;

            // 异步下载高清封面写入草稿（按当前 cover_target 放内嵌或侧车）。
            if let Some(url) = cand.best_cover_url().map(str::to_owned) {
                let alive_c = alive.clone();
                let tok_ref = lyric_token.clone();
                let dir = temp_dir.as_ref().clone();
                let stem = format!("cover-{}", cand.source_id);
                store::download_cover_temp(url, dir, stem, move |dl| {
                    if !alive_c.load(Ordering::Relaxed) || tok_ref.load(Ordering::Relaxed) != tok {
                        return;
                    }
                    if let Ok((path, data, mime)) = dl {
                        let path_str = path.to_string_lossy().into_owned();
                        draft.update(|d| {
                            let (slot, src) = if d.cover_target {
                                (&mut d.cover_file, &mut d.cover_file_src)
                            } else {
                                (&mut d.cover, &mut d.cover_src)
                            };
                            *slot = Some(CoverEdit::Replace { data, mime });
                            *src = Some(path_str);
                        });
                    }
                });
            }

            // 异步拉取歌词写入草稿（失败时保留原歌词，不打断编辑流程）。
            let alive_l = alive.clone();
            let tok_ref = lyric_token.clone();
            store::fetch_candidate_lyric(cand, move |res| {
                if !alive_l.load(Ordering::Relaxed) || tok_ref.load(Ordering::Relaxed) != tok {
                    return;
                }
                match res {
                    Ok(Some(lrc)) if !lrc.trim().is_empty() => {
                        draft.update(|d| {
                            if d.lrc_target {
                                d.lrc_text = lrc;
                            } else {
                                d.values.insert(TagField::Lyrics, lrc);
                            }
                        });
                        lyrics_gen.update(|g| *g += 1);
                        status.set(Some(format!(
                            "已填充来自 {src_label} 的元数据与歌词，请确认后点击「保存」"
                        )));
                    }
                    _ => {
                        status.set(Some(format!(
                            "已填充来自 {src_label} 的元数据（未获取到在线歌词），请确认后点击「保存」"
                        )));
                    }
                }
            });
        }
    };

    let options: Vec<SelectOption> = MatchSource::ALL
        .into_iter()
        .map(|src| {
            SelectOption::new(src.label(), move || {
                source.set(src);
                let mut prefs = store::match_prefs();
                prefs.single_source = src;
                store::set_match_prefs(prefs);
            })
        })
        .collect();

    let run_on_enter = run_search.clone();
    let run_on_click = run_search.clone();

    view()
        .gap(px(10.0))
        .p(px(12.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(border())
        .bg(muted().opacity(0.4))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    text_input(true)
                        .flex_1()
                        .min_w(px(0.0))
                        .label("在线匹配关键词")
                        .placeholder("输入歌名 / 歌手关键词")
                        .default_value(untracked(|| keyword.get()))
                        .readonly(move || searching.get() || draft.with(|d| d.saving))
                        .on_change(move |t: &str| keyword.set(t.to_owned()))
                        .on_submit(move |_: &str| run_on_enter()),
                )
                .child(select(
                    move || source.get().label().to_owned(),
                    "匹配数据源",
                    options,
                    112.0,
                ))
                .child(
                    text_button(
                        Variant::Default,
                        Size::Sm,
                        Some(Icon::Smart),
                        "在线匹配",
                        run_on_click,
                    )
                    .disabled(move || {
                        searching.get()
                            || draft.with(|d| d.saving)
                            || keyword.with(|k| k.trim().is_empty())
                    }),
                ),
        )
        .child(when(
            move || {
                searching.get() || status.with(Option::is_some) || !candidates.with(Vec::is_empty)
            },
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .child(
                        text(move || {
                            if searching.get() {
                                "正在搜索在线元数据…".to_owned()
                            } else if let Some(s) = status.get() {
                                s
                            } else {
                                format!(
                                    "找到 {} 条候选，点击任一项回填表单",
                                    candidates.with(Vec::len)
                                )
                            }
                        })
                        .flex_1()
                        .min_w(px(0.0))
                        .number_of_lines(1)
                        .text_size(px(12.0))
                        .line_height(px(16.0))
                        .text_color(muted_fg()),
                    )
                    .child(when(
                        move || !candidates.with(Vec::is_empty),
                        move || {
                            text_button(
                                Variant::Ghost,
                                Size::Xs,
                                None,
                                if expanded.get() {
                                    "收起候选"
                                } else {
                                    "展开候选"
                                },
                                move || expanded.update(|e| *e = !*e),
                            )
                        },
                    ))
            },
        ))
        .child(when(
            move || expanded.get() && !candidates.with(Vec::is_empty),
            {
                let apply_candidate = apply_candidate.clone();
                move || {
                    let list = candidates.get();
                    let apply_inner = apply_candidate.clone();
                    scroll().max_h(px(220.0)).child(
                        view().gap(px(2.0)).children(
                            list.into_iter()
                                .map(move |item| {
                                    let cand = item.candidate.clone();
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
                                        if let Some(d) =
                                            item.duration_diff_ms.and_then(format_diff_ms)
                                        {
                                            format!("{base} ({d})")
                                        } else {
                                            base
                                        }
                                    });
                                    let apply = apply_inner.clone();
                                    let pick_cand = cand.clone();
                                    let row = view()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(10.0))
                                        .p(px(6.0))
                                        .rounded(px(RADIUS_MD))
                                        .hover(|st| st.bg(hover()))
                                        .role(AccessibilityRole::Button)
                                        .label(format!("选择候选 {}", cand.title))
                                        .on_press(move || apply(pick_cand.clone()))
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
                                                        .child(badge(
                                                            cand.source.short_label(),
                                                            false,
                                                        ))
                                                        .child(
                                                            item.tags
                                                                .best
                                                                .then(|| badge("最佳", true)),
                                                        )
                                                        .child(
                                                            (!item.tags.best
                                                                && item.tags.duration_close)
                                                                .then(|| badge("时长一致", false)),
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
                                        }));
                                    if item.duration_far {
                                        row.opacity(0.45).into_element()
                                    } else {
                                        row.into_element()
                                    }
                                })
                                .collect::<Vec<_>>(),
                        ),
                    )
                }
            },
        ))
}
