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

/// 重命名行的控件：一颗按钮，显示当前模板（空时是占位文案），点开
/// [`rename_dialog`]。
pub(crate) fn rename_button(draft: Signal<BatchDraft>, open: Signal<bool>) -> AnyElement {
    let empty = move || draft.with(|d| d.rename_tpl.trim().is_empty());
    shell(Variant::Outline, Size::Sm)
        .flex_1()
        .min_w(px(96.0))
        .justify_start()
        .accessibility_label("重命名模板")
        .on_press(move |_: &()| open.set(true))
        .child(
            text(move || {
                if empty() {
                    "设置重命名模板…".to_owned()
                } else {
                    draft.with(|d| d.rename_tpl.clone())
                }
            })
            .flex_1()
            .min_w(px(0.0))
            .number_of_lines(1)
            .text_size(px(13.0))
            .line_height(px(13.0))
            .text_color(move || if empty() { muted_fg() } else { fg() }),
        )
        .child(icon(Icon::Edit, 14.0, muted_fg))
        .into_element()
}

/// 重命名模板弹窗：模板输入（实时写进草稿、面板里同步预览）、预设、
/// 占位符速查。不执行任何改名——改名随批量面板的「应用」一并做。
pub(crate) fn rename_dialog(
    draft: Signal<BatchDraft>,
    preview: Memo<Rc<Preview>>,
    error: Memo<Option<String>>,
    open: Signal<bool>,
) -> AnyElement {
    let close = move || open.set(false);
    let append = move |token: &'static str| draft.update(|d| d.rename_tpl.push_str(token));
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
            .map(|tpl| {
                let tpl = *tpl;
                text_button(Variant::Outline, Size::Xs, None, tpl, move || {
                    draft.update(|d| d.rename_tpl = tpl.to_owned())
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
    let content = dialog_content(520.0)
        .child(dialog_header(
            "重命名模板",
            Some("只修改文件名，同名 .lrc 与封面图片一起改名。这里只设模板，点「应用到 N 首」时才执行。"),
        ))
        .child(
            view()
                .gap(px(6.0))
                .child(
                    input_field(false)
                        // 与行里那颗「重命名模板」按钮分开命名，选择器不撞。
                        .label("模板")
                        .placeholder("{track:02d} - {title}")
                        .value(move || draft.with(|d| d.rename_tpl.clone()))
                        .on_change(move |t: &str| draft.update(|d| t.clone_into(&mut d.rename_tpl))),
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
                .child(preview_list(move || preview.with(|p| p.rename.clone()))),
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
        .child(
            dialog_footer()
                .child(text_button(
                    Variant::Ghost,
                    Size::Default,
                    None,
                    "清除模板",
                    move || draft.update(|d| d.rename_tpl.clear()),
                ))
                .child(text_button(
                    Variant::Default,
                    Size::Default,
                    None,
                    "完成",
                    close,
                )),
        );
    modal(content, close)
}
