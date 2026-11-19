use super::*;

use zeo_ui_components::form::{dialog_content, dialog_footer, dialog_header, modal};

/// 工具栏「乱码修复」入口：外框钮（悬停出提示），选中至少一首才可点，
/// 点开 [`encoding_dialog`]。
pub(crate) fn encoding_button(open: Signal<bool>, can: Memo<bool>) -> AnyElement {
    tipped(
        shell(Variant::Outline, Size::IconSm)
            .accessibility_label("乱码修复")
            .disabled(move || !can.get())
            .on_press(move |_: &()| open.set(true))
            .child(icon(Icon::SpellCheck, 14.0, muted_fg)),
        "乱码修复",
    )
    .into_element()
}

/// 工具栏「批量重命名」入口：同 `encoding_button`，点开 [`rename_dialog`]。
pub(crate) fn rename_button(open: Signal<bool>, can: Memo<bool>) -> AnyElement {
    tipped(
        shell(Variant::Outline, Size::IconSm)
            .accessibility_label("批量重命名")
            .disabled(move || !can.get())
            .on_press(move |_: &()| open.set(true))
            .child(icon(Icon::FileRename, 14.0, muted_fg)),
        "批量重命名",
    )
    .into_element()
}

/// 打开弹窗那一刻的所选快照：显示序的路径、TrackEntry 查找表、预览样本
/// （前 [`PREVIEW_N`] 首）。弹窗开着的期间选中集 / 筛选可以变，作用目标
/// 按打开时这一份算（与批量面板对「应用」的口径一致：显示序 ∩ 所选）。
pub(crate) fn tool_context(
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    selected: Signal<BTreeSet<String>>,
) -> (
    Rc<Vec<String>>,
    Rc<HashMap<String, TrackEntry>>,
    Rc<Vec<TrackEntry>>,
) {
    let set = selected.get_untracked();
    let (mut order, mut entries, mut sample) = (Vec::new(), HashMap::new(), Vec::new());
    displayed.with_untracked(|v| {
        for t in v.iter().filter(|t| set.contains(&t.path)) {
            if sample.len() < PREVIEW_N {
                sample.push(t.clone());
            }
            order.push(t.path.clone());
            entries.insert(t.path.clone(), t.clone());
        }
    });
    (Rc::new(order), Rc::new(entries), Rc::new(sample))
}

/// 预览样本里歌词 / 注释的完整文本（视图行里是截断预览）：从文件读。
fn read_sample_texts(sample: &[TrackEntry]) -> SampleTexts {
    let mut texts = SampleTexts::new();
    for t in sample {
        if let Ok(fields) = zeo_metadata::write::read_fields(Path::new(&t.path)) {
            texts.insert(t.path.clone(), fields);
        }
    }
    texts
}

/// 乱码修复弹窗（工具栏入口，作用于当前所选）：先预览前几首会修的字段
/// （含文件里的完整歌词 / 注释），「修复 N 首」后台顺序写回——没乱码的
/// 文件解析后是空改动自然跳过（`resolve_redecode`）。失败留在状态行，全部
/// 成功自动收弹窗。
pub(crate) fn encoding_dialog(
    order: Rc<Vec<String>>,
    entries: Rc<HashMap<String, TrackEntry>>,
    sample: Rc<Vec<TrackEntry>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    open: Signal<bool>,
) -> AnyElement {
    let m = order.len();
    let applying = signal(false);
    let note = signal(Option::<String>::None);
    let close = move || open.set(false);
    let texts = Rc::new(read_sample_texts(&sample));
    let n = sample.len();
    let apply = move || {
        let map = rows.get_untracked();
        let items = batch_edits(
            &order,
            &HashMap::new(),
            &HashMap::new(),
            true,
            &CoverMode::Keep,
            |p, f| {
                entries
                    .get(p)
                    .map(|t| current_value(t, &map, f))
                    .unwrap_or_default()
            },
        );
        if items.is_empty() {
            note.set(Some("没有检测到乱码".to_owned()));
            return;
        }
        applying.set(true);
        note.set(None);
        store::save_tags_batch(items, "乱码修复", move |saved| {
            forget_rows(saved.written.iter(), rows);
            applying.set(false);
            if saved.errors.is_empty() {
                open.set(false);
            } else {
                let lines: Vec<String> = saved
                    .errors
                    .iter()
                    .map(|(p, e)| format!("{}：{e}", zeo_ui_views_base::last_segment(p)))
                    .collect();
                note.set(Some(format!(
                    "{} 首失败：{}",
                    saved.errors.len(),
                    brief(&lines)
                )));
            }
        });
    };
    let content = dialog_content(520.0)
        .child(
            view()
                .absolute()
                .top(px(8.0))
                .right(px(8.0))
                .child(icon_button(
                    || Icon::Close,
                    12.0,
                    "关闭乱码修复",
                    muted_fg,
                    close,
                )),
        )
        .child(dialog_header(
            "乱码修复",
            Some("尽力识别 GBK / Big5 / Shift-JIS 等被误读的标签文本（含歌词、注释），认不准的保持原样。"),
        ))
        .child(preview_list(move || {
            more_note(encoding_preview(&sample, &rows.get(), &texts), m, n)
        }))
        .child(when(
            move || note.with(|n| n.is_some()),
            move || {
                text(move || note.with(|n| n.clone().unwrap_or_default()))
                    .number_of_lines(2)
                    .text_size(px(12.0))
                    .text_color(muted_fg())
            },
        ))
        .child(
            dialog_footer()
                .child(text_button(Variant::Ghost, Size::Default, None, "取消", close))
                .child(
                    text_button(Variant::Default, Size::Default, None, format!("修复 {m} 首"), apply)
                        .disabled(move || applying.get()),
                ),
        );
    modal(content, close)
}
