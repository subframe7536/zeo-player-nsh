use super::*;

/// 「歌词」块：标题行右侧是「从文件导入」按钮与「保存到」目标（`.lrc`
/// 侧车文件或内嵌标签，默认跟已有文件走），下面一行提示 + 多行输入；
/// 两种目标各自的草稿互不影响（切回去内容还在），保存时只写选中的那处。
pub(crate) fn lyrics_block(draft: Signal<TagDraft>, remount: Signal<u32>) -> View {
    let lrc_exists = untracked(|| draft.with(|d| d.lrc_exists));
    let file_mode = move || draft.with(|d| d.lrc_target);
    view()
        .gap(px(8.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text("歌词")
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(muted_fg()),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(import_lyrics_button(draft, remount))
                        .child(target_row(
                            draft,
                            "歌词保存位置",
                            |d, v| d.lrc_target = v,
                            file_mode,
                            ".lrc 文件",
                        )),
                ),
        )
        .child(dynamic(move || {
            let message = if file_mode() {
                Some(if lrc_exists {
                    "保存到同名 .lrc 文件；清空会删除它。"
                } else {
                    "保存时新建同名 .lrc 文件。"
                })
            } else if lrc_exists {
                Some("检测到同名 .lrc——「歌词来源」本地优先时，播放页显示它而不是内嵌歌词。")
            } else {
                None
            };
            message.map(|m| {
                text(m)
                    .text_size(px(12.0))
                    .text_color(faint())
                    .into_element()
            })
        }))
        .child(dynamic(move || {
            lyrics_area(draft, file_mode(), remount.get()).into_element()
        }))
}

/// 「从文件导入歌词…」：选一个 `.lrc` / `.txt`，自动识别编码（UTF-8 /
/// BOM / GBK 等）后整段替换当前目标的歌词草稿（`.lrc` 写 `lrc_text`，
/// 内嵌写 `values[Lyrics]`），输入框按新文本重新挂载；不立即写盘，照常
/// 由「保存」写回（与元数据面板 `import_lyrics_row` 同逻辑）。
fn import_lyrics_button(draft: Signal<TagDraft>, remount: Signal<u32>) -> AnyElement {
    let pick = move || {
        spawn(async move {
            let picked = blocking(|| {
                let file = rfd::FileDialog::new()
                    .set_title("选择歌词文件")
                    .add_filter("歌词", &["lrc", "txt"])
                    .pick_file()?;
                Some(std::fs::read(&file).map_err(|e| format!("读取歌词文件失败: {e}")))
            })
            .await
            .ok()
            .flatten();
            let Some(read) = picked else { return };
            match read {
                Ok(bytes) => {
                    let text = zeo_metadata::export::decode_text_file(&bytes).replace("\r\n", "\n");
                    if text.trim().is_empty() {
                        draft.update(|d| d.error = Some("歌词文件是空的".to_owned()));
                        return;
                    }
                    draft.update(|d| {
                        if d.lrc_target {
                            d.lrc_text = text;
                        } else {
                            d.values.insert(TagField::Lyrics, text);
                        }
                        d.error = None;
                    });
                    remount.update(|r| *r += 1);
                }
                Err(error) => draft.update(|d| d.error = Some(error)),
            }
        });
    };
    text_button(Variant::Outline, Size::Sm, None, "从文件导入歌词…", pick)
        .disabled(move || draft.with(|d| d.saving))
        .into_element()
}

/// 歌词多行输入：`.lrc` 目标写 `lrc_text` 草稿，内嵌写 `values[Lyrics]`；
/// 目标切换或导入后按 `generation` 换 key 重建（各自草稿互不串）。
pub(crate) fn lyrics_area(draft: Signal<TagDraft>, file: bool, generation: u32) -> Input {
    let initial = untracked(|| {
        draft.with(|d| {
            if file {
                d.lrc_text.clone()
            } else {
                d.values.get(&TagField::Lyrics).cloned().unwrap_or_default()
            }
        })
    });
    text_area(TextAreaHeight::Fixed(200.0))
        .key(format!("{file}-{generation}"))
        .label("歌词")
        .default_value(initial)
        .placeholder("粘贴歌词文本（支持 LRC 时间轴）")
        .readonly(move || draft.with(|d| d.saving))
        .on_change(move |t: &str| {
            draft.update(|d| {
                if file {
                    t.clone_into(&mut d.lrc_text);
                } else {
                    d.values.insert(TagField::Lyrics, t.to_owned());
                }
            })
        })
}
