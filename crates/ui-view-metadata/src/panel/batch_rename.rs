use super::*;

use zeo_ui_components::form::{dialog_content, dialog_footer, dialog_header, modal};

/// 常用模板预设：点一下整个替换当前模板。
const RENAME_PRESETS: [&str; 5] = [
    "{track:02d} - {title}",
    "{artist} - {title}",
    "{title}",
    "{album} - {track:02d} - {title}",
    "{artist} - {album} - {title}",
];

/// 占位符速查（与 `rename::Template` 认的名字一一对应）：点一下追加到模板末尾。
const PLACEHOLDERS: [(&str, &str); 15] = [
    ("{title}", "标题"),
    ("{artist}", "艺术家"),
    ("{album}", "专辑"),
    ("{album_artist}", "专辑艺术家"),
    ("{genre}", "流派"),
    ("{year}", "年份"),
    ("{track}", "音轨号"),
    ("{track_total}", "音轨总数"),
    ("{disc}", "碟号"),
    ("{disc_total}", "碟总数"),
    ("{composer}", "作曲"),
    ("{conductor}", "指挥"),
    ("{lyricist}", "作词"),
    ("{publisher}", "发行商"),
    ("{filename}", "原文件名（不含扩展名）"),
];

/// 批量重命名弹窗（工具栏入口，作用于当前所选）：模板输入 + 预设 +
/// 占位符速查 + 前几首的改名预览；「重命名 N 首」直接执行——全部文件名
/// 先整批校验，有一条不可用就整个不改，先于任何写盘。失败留在弹窗状态行，
/// 全部成功自动收弹窗（表格里的行随 `apply_moves` 换到新路径）。
pub(crate) fn rename_dialog(
    order: Rc<Vec<String>>,
    entries: Rc<HashMap<String, TrackEntry>>,
    sample: Rc<Vec<TrackEntry>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    open: Signal<bool>,
) -> AnyElement {
    let m = order.len();
    let tpl = signal(String::new());
    let error = computed(move || template_error(&tpl.get()));
    let applying = signal(false);
    let note = signal(Option::<String>::None);
    let close = move || open.set(false);
    let append = move |token: &'static str| tpl.update(|t| t.push_str(token));
    let caption = |label: &'static str| {
        text(label)
            .text_size(px(12.0))
            .line_height(px(12.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(muted_fg())
    };
    let presets = view().flex_row().flex_wrap().gap(px(6.0)).children(
        RENAME_PRESETS
            .iter()
            .map(|preset| {
                let preset = *preset;
                text_button(Variant::Outline, Size::Xs, None, preset, move || {
                    tpl.set(preset.to_owned())
                })
                .into_element()
            })
            .collect::<Vec<_>>(),
    );
    let cheatsheet = scroll().max_h(px(188.0)).child(
        view().children(
            PLACEHOLDERS
                .iter()
                .map(|(token, name)| {
                    let token = *token;
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .h(px(28.0))
                        .px(px(8.0))
                        .rounded(px(RADIUS_MD))
                        .role(AccessibilityRole::Button)
                        .label(format!("插入 {token}"))
                        .cursor(Cursor::Pointer)
                        .hover(|st| st.bg(hover()))
                        .on_press(move || append(token))
                        .child(
                            text(token)
                                .w(px(120.0))
                                .text_size(px(12.0))
                                .line_height(px(12.0))
                                .text_color(fg()),
                        )
                        .child(
                            text(*name)
                                .flex_1()
                                .min_w(px(0.0))
                                .number_of_lines(1)
                                .text_size(px(12.0))
                                .line_height(px(12.0))
                                .text_color(muted_fg()),
                        )
                        .into_element()
                })
                .collect::<Vec<_>>(),
        ),
    );
    let apply = move || {
        let map = rows.get_untracked();
        let planned = match plan_renames(&tpl.get_untracked(), &order, &entries, &map) {
            Ok(planned) => planned,
            Err(message) => {
                note.set(Some(message));
                return;
            }
        };
        if planned.is_empty() {
            note.set(Some("没有文件需要改名".to_owned()));
            return;
        }
        applying.set(true);
        note.set(None);
        rename_files(planned, rows, selected, move |failures| {
            applying.set(false);
            if failures.is_empty() {
                open.set(false);
            } else {
                note.set(Some(format!(
                    "{} 个重命名失败：{}",
                    failures.len(),
                    brief(&failures)
                )));
            }
        });
    };
    let content = dialog_content(520.0)
        // 右上角关闭钮：脱离标题行贴角放（内容盒自带 24pt 内边距，
        // 再叠 32pt 的命中框就显得悬空了），24pt 小命中框压在角上。
        .child(
            view()
                .absolute()
                .top(px(8.0))
                .right(px(8.0))
                .child(icon_button(
                    || Icon::Close,
                    12.0,
                    "关闭重命名模板",
                    muted_fg,
                    close,
                )),
        )
        .child(dialog_header(
            "重命名模板",
            Some("只修改文件名，同名 .lrc 与封面图片一起改名。模板按当前标签值渲染，点「重命名 N 首」立即执行。"),
        ))
        .child(
            view()
                .gap(px(6.0))
                .child(
                    input_field(false)
                        .label("模板")
                        .placeholder("{track:02d} - {title}")
                        .value(move || tpl.get())
                        .on_change(move |t: &str| tpl.set(t.to_owned())),
                )
                .child(when(
                    move || error.get().is_some(),
                    move || {
                        text(move || error.get().unwrap_or_default())
                            .text_size(px(12.0))
                            .line_height(px(16.0))
                            .text_color(nsh_base::theme().destructive)
                    },
                ))
                .child(preview_list(move || {
                    more_note(
                        rename_preview(&tpl.get(), &sample, &rows.get()),
                        m,
                        sample.len(),
                    )
                })),
        )
        .child(view().gap(px(8.0)).child(caption("预设")).child(presets))
        .child(
            view()
                .gap(px(6.0))
                .child(caption("占位符（点一下追加到模板末尾）"))
                .child(cheatsheet)
                .child(
                    text("补零写 {track:02d}；要写字面大括号用 {{ 和 }}。")
                        .text_size(px(11.0))
                        .text_color(faint()),
                ),
        )
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
                .child(text_button(
                    Variant::Ghost,
                    Size::Default,
                    None,
                    "清除模板",
                    move || tpl.set(String::new()),
                ))
                .child(
                    text_button(Variant::Default, Size::Default, None, format!("重命名 {m} 首"), apply)
                        // 空模板 / 模板报错 / 正在改都不让按（报错已在输入框下标红）。
                        .disabled(move || {
                            tpl.with(|t| t.trim().is_empty())
                                || error.get().is_some()
                                || applying.get()
                        }),
                ),
        );
    modal(content, close)
}
