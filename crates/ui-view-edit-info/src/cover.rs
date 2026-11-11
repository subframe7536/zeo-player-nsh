use super::*;

/// 「封面」栏：「保存到」选同名图片文件或内嵌标签（默认跟已有文件
/// 走），两目标各自的改动草稿互不影响——`.lrc` / 内嵌歌词同款。
/// 192px 预览（新图 / 目标的现有封面 / 占位符）+ 更换 / 移除按钮
/// 在剩余高度里整体居中（栏高固定给最高的「基础」栏）。
pub(crate) fn cover_pane(draft: Signal<TagDraft>) -> View {
    let has_art = untracked(|| {
        draft.with(|d| {
            zeo_metadata::read_meta(Path::new(&d.path))
                .map(|m| m.has_art)
                .unwrap_or(false)
        })
    });
    let art_exists = untracked(|| draft.with(|d| d.art_exists));
    let file_mode = move || draft.with(|d| d.cover_target);
    let src = move || {
        let d = draft.get();
        let (edit, picked) = if d.cover_target {
            (&d.cover_file, &d.cover_file_src)
        } else {
            (&d.cover, &d.cover_src)
        };
        match edit {
            // 换封面：预览本地图片文件。
            Some(CoverEdit::Replace { .. }) => picked.clone().map(SharedString::from),
            // 移除封面：显示占位符。
            Some(CoverEdit::Remove) => None,
            // 目标的现有封面：图片文件 = 同名侧车文件本身；内嵌 = 内嵌
            // 封面的缩略图（不能走 cover_for_always——它按「本地优先」
            // 选源，内嵌目标会错显侧车图）。
            None => {
                if d.cover_target {
                    zeo_metadata::covers::find_sidecar(Path::new(&d.path))
                        .map(|p| SharedString::from(p.to_string_lossy().into_owned()))
                } else {
                    has_art
                        .then(|| {
                            zeo_metadata::covers::embedded_cover_file(
                                Path::new(&d.path),
                                zeo_metadata::covers::THUMB,
                            )
                            .map(|p| SharedString::from(p.to_string_lossy().into_owned()))
                        })
                        .flatten()
                }
            }
        }
    };
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
            let outcome = (|| {
                let mime = write::cover_mime(&file).ok_or_else(|| {
                    "不支持的图片格式（PNG / JPEG / WebP / BMP / GIF）".to_owned()
                })?;
                std::fs::read(&file)
                    .map_err(|e| format!("读取图片失败: {e}"))
                    .map(|data| (data, mime))
            })();
            match outcome {
                Ok((data, mime)) => draft.update(|d| {
                    let (slot, src) = if d.cover_target {
                        (&mut d.cover_file, &mut d.cover_file_src)
                    } else {
                        (&mut d.cover, &mut d.cover_src)
                    };
                    *slot = Some(CoverEdit::Replace { data, mime });
                    *src = Some(file.to_string_lossy().into_owned());
                    d.error = None;
                }),
                Err(error) => draft.update(|d| d.error = Some(error)),
            }
        });
    };
    let remove = move || {
        draft.update(|d| {
            let (slot, src) = if d.cover_target {
                (&mut d.cover_file, &mut d.cover_file_src)
            } else {
                (&mut d.cover, &mut d.cover_src)
            };
            *slot = Some(CoverEdit::Remove);
            *src = None;
            d.error = None;
        })
    };
    view()
        .min_h(px(440.0))
        .gap(px(8.0))
        .child(target_row(
            draft,
            "封面保存位置",
            |d, v| d.cover_target = v,
            file_mode,
            "同名图片",
        ))
        .child(when(
            move || file_mode(),
            move || {
                text(if art_exists {
                    "保存到同名图片文件；「移除封面」会删除它。"
                } else {
                    "保存时新建同名图片文件。"
                })
                .text_size(px(12.0))
                .text_color(faint())
            },
        ))
        .child(when(
            move || file_mode(),
            move || {
                text("同名图片在「封面来源」本地优先时先于内嵌封面显示。")
                    .text_size(px(12.0))
                    .text_color(faint())
            },
        ))
        .child(when(
            move || !file_mode(),
            move || {
                text("保存后替换文件里的全部内嵌图片。")
                    .text_size(px(12.0))
                    .text_color(faint())
            },
        ))
        .child(when(
            move || !file_mode() && art_exists,
            move || {
                text("检测到同名图片——「封面来源」本地优先时，显示它而不是内嵌封面。")
                    .text_size(px(12.0))
                    .text_color(faint())
            },
        ))
        .child(
            view().flex_1().items_center().justify_center().child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(20.0))
                    .child(cover(src, Some(192.0), 10.0))
                    .child(
                        view()
                            .gap(px(8.0))
                            .child(
                                text_button(
                                    Variant::Outline,
                                    Size::Default,
                                    None,
                                    "更换图片…",
                                    pick,
                                )
                                .w(px(112.0)),
                            )
                            .child(
                                text_button(
                                    Variant::Outline,
                                    Size::Default,
                                    None,
                                    "移除封面",
                                    remove,
                                )
                                .w(px(112.0))
                                .disabled(move || {
                                    draft.with(|d| {
                                        if d.saving {
                                            return true;
                                        }
                                        if d.cover_target {
                                            !d.art_exists && d.cover_file.is_none()
                                        } else {
                                            !has_art && d.cover.is_none()
                                        }
                                    })
                                }),
                            ),
                    ),
            ),
        )
}

/// 「保存到」行：封面 / 歌词栏共用——标签 + 目标下拉（trigger 110，
/// 弹层与 trigger 同宽）。`set` 写哪个目标字段（`cover_target` /
/// `lrc_target`），`file_mode` 读当前目标；`file_name` 是文件目标
/// 的显示名（「同名图片」/「.lrc 文件」），另一项固定「内嵌标签」。
pub(crate) fn target_row(
    draft: Signal<TagDraft>,
    label: &'static str,
    set: impl Fn(&mut TagDraft, bool) + Copy + 'static,
    file_mode: impl Fn() -> bool + Copy + 'static,
    file_name: &'static str,
) -> View {
    let target =
        move |file: bool| -> Box<dyn Fn()> { Box::new(move || draft.update(|d| set(d, file))) };
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text("保存到")
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(select(
            move || {
                if file_mode() {
                    file_name
                } else {
                    "内嵌标签"
                }
                .to_owned()
            },
            label,
            vec![
                SelectOption::new(file_name, target(true)),
                SelectOption::new("内嵌标签", target(false)),
            ],
            110.0,
        ))
}

// --- 歌词 ----------------------------------------------------------------------
