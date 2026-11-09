use super::*;

/// 「歌词」栏：「保存到」选 `.lrc` 侧车文件或内嵌标签（默认跟已有文件
/// 走），多行输入；两种目标各自的草稿互不影响（切回去内容还在），
/// 保存时只写选中的那处。
pub(crate) fn lyrics_pane(draft: Signal<TagDraft>) -> View {
    let lrc_exists = untracked(|| draft.with(|d| d.lrc_exists));
    let file_mode = move || draft.with(|d| d.lrc_target);
    view()
        .gap(px(8.0))
        .child(target_row(
            draft,
            "歌词保存位置",
            |d, v| d.lrc_target = v,
            file_mode,
            ".lrc 文件",
        ))
        .child(when(
            move || file_mode(),
            move || {
                text(if lrc_exists {
                    "保存到同名 .lrc 文件；清空会删除它。"
                } else {
                    "保存时新建同名 .lrc 文件。"
                })
                .text_size(px(12.0))
                .text_color(faint())
            },
        ))
        .child(when(
            move || !file_mode() && lrc_exists,
            move || {
                text("检测到同名 .lrc——「歌词来源」本地优先时，播放页显示它而不是内嵌歌词。")
                    .text_size(px(12.0))
                    .text_color(faint())
            },
        ))
        .child(
            text("歌词")
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(dynamic(move || {
            lyrics_area(draft, file_mode()).into_element()
        }))
}

/// 歌词多行输入：`.lrc` 目标写 `lrc_text` 草稿，内嵌写 `values[Lyrics]`；
/// 目标切换时重建（各自草稿互不串）。
pub(crate) fn lyrics_area(draft: Signal<TagDraft>, file: bool) -> Input {
    let initial = untracked(|| {
        draft.with(|d| {
            if file {
                d.lrc_text.clone()
            } else {
                d.values.get(&TagField::Lyrics).cloned().unwrap_or_default()
            }
        })
    });
    input_field(false)
        .label("歌词")
        .multiline(true)
        .h(px(220.0))
        .py(px(10.0))
        .default_value(initial)
        .placeholder("粘贴歌词文本（支持 LRC 时间轴）")
        .readonly(move || draft.with(|d| d.saving))
        .on_change(move |t: &str| {
            draft.update(|d| {
                if file {
                    d.lrc_text = t.to_owned();
                } else {
                    d.values.insert(TagField::Lyrics, t.to_owned());
                }
            })
        })
}
