use super::*;

/// 批量封面行：保持 / 替换…（文件选择器暂存）/ 移除；已暂存显示
/// 说明与「撤销」。
pub(crate) fn cover_mode_row(draft: Signal<BatchDraft>) -> View {
    let pick = move || {
        spawn(async move {
            let picked = blocking(|| {
                rfd::FileDialog::new()
                    .set_title("选择封面图片")
                    .add_filter("图片", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
                    .pick_file()
            })
            .await
            .ok()
            .flatten();
            let Some(file) = picked else { return };
            if let Ok((data, mime)) = write::cover_mime(&file)
                .ok_or_else(|| "不支持的图片格式".to_owned())
                .and_then(|mime| {
                    std::fs::read(&file)
                        .map(|data| (data, mime))
                        .map_err(|e| format!("读取图片失败: {e}"))
                })
            {
                draft.update(|d| {
                    d.cover = CoverMode::Replace {
                        data,
                        mime,
                        src: file.to_string_lossy().into_owned(),
                    };
                });
            }
        });
    };
    wrap_row(
        "封面",
        vec![
            dynamic(move || match draft.get().cover {
                CoverMode::Keep => view()
                    .flex_row()
                    .gap(px(8.0))
                    .child(text_button(Variant::Outline, Size::Xs, None, "替换…", pick))
                    .child(text_button(
                        Variant::Destructive,
                        Size::Xs,
                        None,
                        "移除",
                        move || draft.update(|d| d.cover = CoverMode::Remove),
                    ))
                    .into_element(),
                ref mode => {
                    let note = match mode {
                        CoverMode::Replace { src, .. } => {
                            format!("替换为 {}", zeo_ui_views_base::last_segment(src))
                        }
                        _ => "移除内嵌封面".to_owned(),
                    };
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text(note)
                                .flex_1()
                                .min_w(px(0.0))
                                .number_of_lines(1)
                                .text_size(px(12.0))
                                .line_height(px(12.0))
                                .text_color(fg()),
                        )
                        .child(text_button(
                            Variant::Ghost,
                            Size::Xs,
                            None,
                            "撤销",
                            move || draft.update(|d| d.cover = CoverMode::Keep),
                        ))
                        .into_element()
                }
            })
            .into_element(),
        ],
    )
}

/// 「应用」：按当前显示序生成每首的 `TagEdits`（编号 / 大小写覆盖同名
/// 字段），后台顺序写回；成功行失效重读。设了重命名模板就在标签写完后
/// 接着改名（模板按应用后的标签渲染）：全部文件名先整批校验，有一条
/// 不可用就整个不应用，先于任何写盘。
pub(crate) fn apply_batch(
    paths: Rc<Vec<String>>,
    selected_entries: Rc<HashMap<String, TrackEntry>>,
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    draft: Signal<BatchDraft>,
) {
    let d = draft.get_untracked();
    // 编号按当前显示序（不是选择集的字典序）。
    let order: Vec<String> = displayed
        .get_untracked()
        .iter()
        .filter(|t| paths.contains(&t.path))
        .map(|t| t.path.clone())
        .collect();
    let map = rows.get_untracked();
    let items = batch_edits(
        &order,
        &d.modes,
        &d.values,
        d.numbering(),
        d.case,
        &d.cover,
        order.len(),
        |p, f| {
            selected_entries
                .get(p)
                .map(|t| current_value(t, &map, f))
                .unwrap_or_default()
        },
    );
    let renames = match plan_renames(&d.rename_tpl, &order, &items, &selected_entries, &map) {
        Ok(planned) => planned,
        Err(message) => {
            draft.update(|d| d.message = Some(message));
            return;
        }
    };
    if items.is_empty() && renames.is_empty() {
        draft.update(|d| d.message = Some("没有要应用的修改".to_owned()));
        return;
    }
    draft.update(|d| {
        d.applying = true;
        d.message = None;
    });
    let tag_total = items.len();
    let item_paths: Vec<String> = items.iter().map(|(p, _)| p.clone()).collect();
    let after_tags = move |errors: Vec<(String, String)>| {
        // 失败的路径仍保留在选中集里；成功行失效，加载器按新指纹重读。
        let ok: Vec<&String> = item_paths
            .iter()
            .filter(|p| !errors.iter().any(|(e, _)| e == *p))
            .collect();
        {
            let mut cache = full_cache().lock().unwrap();
            for p in &ok {
                cache.retain(|(cp, _)| cp != *p);
            }
        }
        rows.update(|r| {
            for p in &ok {
                r.remove(*p);
            }
        });
        let tag_note = (tag_total > 0).then(|| {
            if errors.is_empty() {
                format!("已更新 {tag_total} 首")
            } else {
                let lines: Vec<String> = errors
                    .iter()
                    .map(|(p, e)| format!("{}：{e}", zeo_ui_views_base::last_segment(p)))
                    .collect();
                format!("{} 首失败：{}", errors.len(), brief(&lines))
            }
        });
        // 标签没写成的不改名：它们的内容还是旧的，改了名也对不上预览。
        let todo: Vec<rename::Planned> = renames
            .into_iter()
            .filter(|p| !errors.iter().any(|(e, _)| *e == p.src))
            .collect();
        let rename_total = todo.len();
        let finish = move |rename_note: Option<String>| {
            draft.update(|d| {
                d.applying = false;
                let notes: Vec<String> = tag_note.into_iter().chain(rename_note).collect();
                d.message = Some(notes.join("；"));
            });
        };
        if todo.is_empty() {
            finish(None);
            return;
        }
        rename_files(todo, rows, selected, move |failures| {
            finish(Some(if failures.is_empty() {
                format!("已重命名 {rename_total} 个文件")
            } else {
                format!("{} 个重命名失败：{}", failures.len(), brief(&failures))
            }))
        });
    };
    if items.is_empty() {
        after_tags(Vec::new());
    } else {
        store::save_tags_batch(items, after_tags);
    }
}

/// 失败清单缩成一句（最多 3 条）。
fn brief(failures: &[String]) -> String {
    failures
        .iter()
        .take(3)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("；")
}

/// 批量草稿 → 每首一份 `TagEdits`（纯函数）：三态按字段展开（「保持原
/// 值」= `Edit::Keep`），编号与大小写在其后覆盖同名字段；全空的跳过。
/// `total` 是所选总数（写音轨总数用），`paths` 的下标就是编号偏移。
pub(crate) fn batch_edits(
    paths: &[String],
    modes: &HashMap<TagField, Tri>,
    values: &HashMap<TagField, String>,
    numbering: Option<(u32, bool)>,
    case: Option<CaseOp>,
    cover: &CoverMode,
    total: usize,
    field_of: impl Fn(&str, TagField) -> String,
) -> Vec<(String, TagEdits)> {
    // `paths` 可以只是所选的前几首（预览），总数照所选全部。
    let total = total as u32;
    paths
        .iter()
        .enumerate()
        .filter_map(|(i, path)| {
            let mut fields: Vec<(TagField, Edit)> = TRI_FIELDS
                .iter()
                .map(|f| {
                    (
                        *f,
                        match modes.get(f).copied().unwrap_or(Tri::Keep) {
                            Tri::Keep => Edit::Keep,
                            Tri::Set => Edit::Set(values.get(f).cloned().unwrap_or_default()),
                            Tri::Clear => Edit::Clear,
                        },
                    )
                })
                .collect();
            let mut put = |f: TagField, e: Edit| {
                if let Some(slot) = fields.iter_mut().find(|(k, _)| *k == f) {
                    slot.1 = e;
                } else {
                    fields.push((f, e));
                }
            };
            // 编号 / 大小写覆盖同名字段（tri-state 里 TrackTotal 也在）。
            if let Some((start, with_total)) = numbering {
                put(TagField::Track, Edit::Set((start + i as u32).to_string()));
                if with_total {
                    put(TagField::TrackTotal, Edit::Set(total.to_string()));
                }
            }
            if let Some(op) = case {
                for f in [TagField::Title, TagField::Artist, TagField::Album] {
                    let old = field_of(path, f);
                    let new = case_convert(&old, op);
                    if new != old {
                        put(f, Edit::Set(new));
                    }
                }
            }
            let cover = match cover {
                CoverMode::Keep => None,
                CoverMode::Replace { data, mime, .. } => Some(CoverEdit::Replace {
                    data: data.clone(),
                    mime: mime.clone(),
                }),
                CoverMode::Remove => Some(CoverEdit::Remove),
            };
            let edits = TagEdits {
                fields,
                cover,
                cover_file: None,
                lrc: None,
            };
            (!edits.is_empty()).then(|| (path.clone(), edits))
        })
        .collect()
}

/// 英文大小写：Title = 每个 ASCII 字母词首字母大写其余小写（数字与
/// 非 ASCII 原样保留）；Lower / Upper 整串转。
pub(crate) fn case_convert(s: &str, op: CaseOp) -> String {
    match op {
        CaseOp::Lower => s.to_lowercase(),
        CaseOp::Upper => s.to_uppercase(),
        CaseOp::Title => {
            let mut out = String::with_capacity(s.len());
            let mut word_start = true;
            for c in s.chars() {
                if c.is_ascii_alphabetic() {
                    out.push(if word_start {
                        c.to_ascii_uppercase()
                    } else {
                        c.to_ascii_lowercase()
                    });
                } else {
                    out.push(c);
                }
                word_start = !c.is_ascii_alphanumeric();
            }
            out
        }
    }
}
