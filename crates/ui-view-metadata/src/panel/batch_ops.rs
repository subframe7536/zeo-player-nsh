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
                    .child(text_button(Variant::Outline, Size::Sm, None, "替换…", pick))
                    .child(text_button(
                        Variant::Outline,
                        Size::Sm,
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
                            Size::Sm,
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
        d.case,
        d.fix_encoding,
        &d.cover,
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
    let has_tag_items = !items.is_empty();
    let after_tags = move |saved: store::BatchSaved| {
        let errors = saved.errors;
        // 失败的路径仍保留在选中集里；写成功的行失效，加载器按新指纹重读。
        forget_rows(saved.written.iter(), rows);
        let tag_note = has_tag_items.then(|| {
            if errors.is_empty() {
                match saved.written.len() {
                    0 => "标签没有需要修改的内容".to_owned(),
                    n => format!("已更新 {n} 首"),
                }
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
        after_tags(store::BatchSaved::default());
    } else {
        store::save_tags_batch(items, "批量修改", after_tags);
    }
}

/// 标签写成功的文件：丢掉它们的行缓存与单选面板缓存，加载器按新指纹重读。
pub(crate) fn forget_rows<'a>(
    paths: impl IntoIterator<Item = &'a String>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
) {
    let paths: Vec<&String> = paths.into_iter().collect();
    {
        let mut cache = full_cache().lock().unwrap();
        cache.retain(|(cp, _)| !paths.contains(&cp));
    }
    rows.update(|r| {
        for p in &paths {
            r.remove(*p);
        }
    });
}

/// 失败清单缩成一句（最多 3 条）。
pub(crate) fn brief(failures: &[String]) -> String {
    failures
        .iter()
        .take(3)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("；")
}

/// 乱码修复覆盖的文本字段（值取自曲库索引）。歌词与注释不在这里：视图行里它们
/// 只是截断的预览，拿预览去修复再写回会把原文截掉——它们走
/// [`ENCODING_FILE_FIELDS`]。
pub(crate) const ENCODING_FIELDS: [TagField; 10] = [
    TagField::Title,
    TagField::Artist,
    TagField::Album,
    TagField::AlbumArtist,
    TagField::Genre,
    TagField::Composer,
    TagField::Conductor,
    TagField::Lyricist,
    TagField::Publisher,
    TagField::Copyright,
];

/// 乱码修复里要读文件才有完整文本的字段（`Edit::Redecode`）。
pub(crate) const ENCODING_FILE_FIELDS: [TagField; 2] = [TagField::Lyrics, TagField::Comment];

/// 写进 `fields` 的某字段改动（已有就换，没有就加）。
fn put_edit(fields: &mut Vec<(TagField, Edit)>, field: TagField, edit: Edit) {
    match fields.iter_mut().find(|(k, _)| *k == field) {
        Some(slot) => slot.1 = edit,
        None => fields.push((field, edit)),
    }
}

/// 该字段此刻的值：前面的改动（三态 / 乱码修复）已经定了新值就用它，
/// 否则是 `old`——后面的步骤（大小写）作用在前面步骤的结果上。
fn effective_value(fields: &[(TagField, Edit)], field: TagField, old: String) -> String {
    match fields.iter().find(|(k, _)| *k == field).map(|(_, e)| e) {
        Some(Edit::Set(v)) => v.clone(),
        Some(Edit::Clear) => String::new(),
        _ => old,
    }
}

/// 批量草稿 → 每首一份 `TagEdits`（纯函数）：三态按字段展开（「保持原
/// 值」= `Edit::Keep`），乱码修复、大小写依次在其后覆盖同名字段（三态已定值的
/// 字段不做乱码修复；大小写作用在修复后的值上）；全空的跳过。
pub(crate) fn batch_edits(
    paths: &[String],
    modes: &HashMap<TagField, Tri>,
    values: &HashMap<TagField, String>,
    case: Option<CaseOp>,
    fix_encoding: bool,
    cover: &CoverMode,
    field_of: impl Fn(&str, TagField) -> String,
) -> Vec<(String, TagEdits)> {
    paths
        .iter()
        .filter_map(|path| {
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
            // 乱码修复 / 大小写覆盖同名字段。
            if fix_encoding {
                for f in ENCODING_FIELDS {
                    let decided = fields.iter().any(|(k, e)| *k == f && *e != Edit::Keep);
                    if decided {
                        continue;
                    }
                    if let Some(fixed) = fix_mojibake(&field_of(path, f)) {
                        put_edit(&mut fields, f, Edit::Set(fixed));
                    }
                }
                // 歌词与注释的完整文本只在文件里（视图里是截断预览）：留给写回时
                // 读文件解析（`resolve_redecode`），没乱码的文件自然成空改动。
                for f in ENCODING_FILE_FIELDS {
                    let decided = fields.iter().any(|(k, e)| *k == f && *e != Edit::Keep);
                    if !decided {
                        put_edit(&mut fields, f, Edit::Redecode);
                    }
                }
            }
            if let Some(op) = case {
                for f in [TagField::Title, TagField::Artist, TagField::Album] {
                    let old = effective_value(&fields, f, field_of(path, f));
                    let new = case_convert(&old, op);
                    if new != old {
                        put_edit(&mut fields, f, Edit::Set(new));
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
