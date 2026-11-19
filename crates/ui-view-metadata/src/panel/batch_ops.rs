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

/// 「应用」：按当前显示序生成每首的 `TagEdits`，后台顺序写回；成功行失效
/// 重读。改名不在这里：模板重命名是工具栏入口的独立弹窗（`batch_rename`）。
pub(crate) fn apply_batch(
    paths: Rc<Vec<String>>,
    selected_entries: Rc<HashMap<String, TrackEntry>>,
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    draft: Signal<BatchDraft>,
    applied: Signal<u64>,
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
    let items = batch_edits(&order, &d.modes, &d.values, false, &d.cover, |p, f| {
        selected_entries
            .get(p)
            .map(|t| current_value(t, &map, f))
            .unwrap_or_default()
    });
    if items.is_empty() {
        draft.update(|d| d.message = Some("没有要应用的修改".to_owned()));
        return;
    }
    draft.update(|d| {
        d.applying = true;
        d.message = None;
    });
    let after_tags = move |saved: store::BatchSaved| {
        let errors = saved.errors;
        // 失败的路径仍保留在选中集里；写成功的行失效，加载器按新指纹重读。
        forget_rows(saved.written.iter(), rows);
        // 全成功不复述「已更新」——顶栏按钮换 ✓ 三秒；有失败仍走状态行说清。
        let ok = errors.is_empty();
        draft.update(|d| {
            d.applying = false;
            d.message = (!ok).then(|| {
                let lines: Vec<String> = errors
                    .iter()
                    .map(|(p, e)| format!("{}：{e}", zeo_ui_views_base::last_segment(p)))
                    .collect();
                format!("{} 首失败：{}", errors.len(), brief(&lines))
            });
        });
        if ok && let Some(prev) = applied.try_get() {
            // try_*：面板在后台写盘期间被关掉时信号已随 owner 析构。
            let seq = prev + 1;
            applied.try_set(seq);
            spawn(async move {
                timer(std::time::Duration::from_secs(3)).await;
                // 三秒内又应用成功（seq 已前进）就不复位，由新的计时器管。
                if applied.try_get() == Some(seq) {
                    applied.try_set(0);
                }
            });
        }
    };
    store::save_tags_batch(items, "批量修改", after_tags);
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

/// 批量改动 → 每首一份 `TagEdits`（纯函数）：三态按字段展开（「保持原
/// 值」= `Edit::Keep`），乱码修复在其后覆盖同名字段（三态已定值的字段不做
/// 乱码修复）；全空的跳过。批量面板传 `fix_encoding = false`——独立的
/// 乱码修复弹窗传 `true` 且三态为空。
pub(crate) fn batch_edits(
    paths: &[String],
    modes: &HashMap<TagField, Tri>,
    values: &HashMap<TagField, String>,
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
