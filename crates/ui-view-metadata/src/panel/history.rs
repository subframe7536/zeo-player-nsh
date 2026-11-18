use super::*;

use std::collections::HashSet;

use zeo_ui_components::form::{dialog_content, dialog_footer, dialog_header, modal};

/// 标签历史弹窗（入口在「显示列」下拉末尾，只在选中恰好一首时能打开）：列出写过这首曲目的历史批次——每次保存 /
/// 批量应用写回前自动记下被改字段的旧值（含封面）——每批一键还原。还原本身
/// 也记成新批次，刷新后出现在最上面（可以再还原回去）。
///
/// 还原写盘之后单选面板的输入框是非受控的、还停在旧值：成功后 `reload` 加一，
/// 面板按新读出的磁盘值整个重建（弹窗是页面级的，不受重建影响）。
pub(crate) fn history_dialog(
    path: String,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    open: Signal<bool>,
    reload: Signal<u32>,
) -> AnyElement {
    // 列表在还原后重读；`restoring` 防止连点。
    let rev = signal(0u32);
    let restoring = signal(false);
    let note = signal(Option::<String>::None);
    let close = move || open.set(false);
    let own_path = path.clone();

    let list = dynamic(move || {
        rev.get();
        let batches: Vec<AnyElement> = store::tag_history()
            .into_iter()
            .filter(|batch| batch.paths.contains(&own_path))
            .map(|batch| history_row(&batch, &own_path, rows, rev, restoring, note, reload))
            .collect();
        if batches.is_empty() {
            return text("这首曲目还没有标签历史。保存写回前会自动记下旧值。")
                .text_size(px(13.0))
                .text_color(muted_fg())
                .into_element();
        }
        view().gap(px(2.0)).children(batches).into_element()
    });

    let content = dialog_content(520.0)
        .child(dialog_header(
            "标签历史",
            Some("保留最近 30 次写回的旧值（含封面）。还原只作用于这首曲目，并同样记入历史。"),
        ))
        .child(scroll().max_h(px(320.0)).child(list))
        .child(when(
            move || note.with(|n| n.is_some()),
            move || {
                text(move || note.with(|n| n.clone().unwrap_or_default()))
                    .text_size(px(12.0))
                    .text_color(muted_fg())
            },
        ))
        .child(dialog_footer().child(text_button(
            Variant::Default,
            Size::Default,
            None,
            "完成",
            close,
        )));
    modal(content, close)
}

/// 一个批次一行：摘要 + 时间 / 同批文件数 + 还原钮。
fn history_row(
    batch: &store::TagBatch,
    path: &str,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    rev: Signal<u32>,
    restoring: Signal<bool>,
    note: Signal<Option<String>>,
    reload: Signal<u32>,
) -> AnyElement {
    let id = batch.id;
    let only: HashSet<String> = HashSet::from([path.to_owned()]);
    let restore = move || {
        if restoring.get_untracked() {
            return;
        }
        restoring.set(true);
        note.set(None);
        store::restore_tag_batch(id, &only, move |saved| {
            let errors = saved.errors;
            forget_rows(saved.written.iter(), rows);
            let wrote = !saved.written.is_empty();
            note.set(Some(if errors.is_empty() {
                format!("已还原 {} 首", saved.written.len())
            } else {
                let lines: Vec<String> = errors
                    .iter()
                    .map(|(p, e)| format!("{}：{e}", zeo_ui_views_base::last_segment(p)))
                    .collect();
                format!("还原失败：{}", brief(&lines))
            }));
            restoring.set(false);
            rev.update(|r| *r += 1);
            if wrote {
                // 最后一步：面板重建会销毁弹窗与它的信号。
                reload.update(|r| *r += 1);
            }
        });
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .py(px(8.0))
        .border_b_1()
        .border_b_color(border())
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(4.0))
                .child(
                    text(batch.summary.clone())
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(fg()),
                )
                .child(
                    text(format!(
                        "{} · 同批 {} 首",
                        zeo_types::config::format_local_datetime(batch.created_at),
                        batch.paths.len()
                    ))
                    .number_of_lines(1)
                    .text_size(px(11.0))
                    .line_height(px(11.0))
                    .text_color(muted_fg()),
                ),
        )
        .child(
            text_button(Variant::Outline, Size::Xs, None, "还原", restore)
                .disabled(move || restoring.get()),
        )
        .into_element()
}
