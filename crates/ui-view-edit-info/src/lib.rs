//! 「编辑元数据」弹窗：单曲目的标签编辑，分「基础元数据 / 封面 / 歌词」
//! 三栏；保存走 `store::save_tags`（原子写回 + 索引刷新 + 当前曲重载）。

pub(crate) use std::collections::HashMap;

pub(crate) use std::path::Path;

pub(crate) use nsh_core::prelude::*;

pub(crate) use zeo_metadata::write::{self, CoverEdit, Edit, TagEdits, TagField};

pub(crate) use zeo_ui_components::button::{RADIUS_MD, Size, Variant, text_button};

pub(crate) use zeo_ui_components::cover::cover;

pub(crate) use zeo_ui_components::form::{
    SelectOption, dialog_content, dialog_footer, dialog_header, input_field, modal, select,
    tab_list, tab_trigger, trigger_chevron,
};

pub(crate) use zeo_ui_components::icons::{Icon, icon};

pub(crate) use zeo_ui_store::theme::*;

pub(crate) use zeo_ui_store::{self as store, store};

mod cover;
mod genre;
mod logic;
mod lyrics;
mod widgets;

pub(crate) use cover::*;
pub(crate) use genre::*;
pub(crate) use logic::*;
pub(crate) use lyrics::*;
pub(crate) use widgets::*;

/// 弹窗里的草稿：打开时从文件读出初值，保存时与原值比较生成 `TagEdits`
/// （没动的字段 Keep，不触发无谓的重写 / 多值键合并）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TagDraft {
    path: String,
    original: HashMap<TagField, String>,
    values: HashMap<TagField, String>,
    /// `None`：内嵌封面不动。
    cover: Option<CoverEdit>,
    /// `Replace` 时新图的本地路径（预览用文件路径，不用 data URI）。
    cover_src: Option<String>,
    /// 同名图片文件已存在（「保存到」默认值与提示文案用）。
    art_exists: bool,
    /// 封面写到同名图片文件（true）还是内嵌标签（false）；默认跟已有文件走。
    cover_target: bool,
    /// 图片文件目标的封面改动 / `Replace` 预览路径（与内嵌目标的
    /// 草稿互不影响——`.lrc` / 内嵌歌词同款）。
    cover_file: Option<CoverEdit>,
    cover_file_src: Option<String>,
    /// 同名 `.lrc` 侧车歌词文件已存在（提示文案与「清空会删除它」用）。
    lrc_exists: bool,
    /// 歌词写到 `.lrc`（true）还是内嵌标签（false）；默认跟已有文件走。
    lrc_target: bool,
    /// `.lrc` 目标打开时的文件内容（对比改动用）。
    lrc_original: String,
    /// `.lrc` 目标的草稿（内嵌目标仍走 `values[Lyrics]`）。
    lrc_text: String,
    /// 保存进行中：输入与按钮禁用。
    saving: bool,
    /// 上次保存失败的报错。
    error: Option<String>,
}

/// 「编辑元数据」弹窗（`store().edit_info` 有路径时挂载）。
pub fn edit_info_dialog() -> AnyElement {
    let s = store();
    let path = s.edit_info.get_untracked().unwrap_or_default();
    // 读当前标签值（弹窗打开一次；读不出就当全空，用户仍可填）。
    let values = write::read_fields(Path::new(&path)).unwrap_or_default();
    // 同名 `.lrc` 已有时歌词默认写它（编辑的就是显示的那份）。
    let lrc_path = zeo_engine::Meta::external_lrc(path.clone());
    let lrc_original = lrc_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    // 同名图片已有时封面默认写它（编辑的就是显示的那份）。
    let art_exists = zeo_metadata::covers::find_sidecar(Path::new(&path)).is_some();
    let draft = signal(TagDraft {
        path,
        original: values.clone(),
        values,
        cover: None,
        cover_src: None,
        art_exists,
        cover_target: art_exists,
        cover_file: None,
        cover_file_src: None,
        lrc_exists: lrc_path.is_some(),
        lrc_target: lrc_path.is_some(),
        lrc_text: lrc_original.clone(),
        lrc_original,
        saving: false,
        error: None,
    });
    let tab = signal(0u8);
    let close = move || s.edit_info.set(None);
    let content = dialog_content(480.0)
        .child(dialog_header("编辑元数据", None))
        .child(
            tab_list()
                .child(
                    tab_trigger(move || tab.get() == 0, None, "基础", move || tab.set(0)).flex_1(),
                )
                .child(
                    tab_trigger(move || tab.get() == 1, None, "封面", move || tab.set(1)).flex_1(),
                )
                .child(
                    tab_trigger(move || tab.get() == 2, None, "歌词", move || tab.set(2)).flex_1(),
                ),
        )
        .child(
            // 固定高度：取最高的「基础」栏（9 行 ×36 + 间距 + 内边距 ≈ 440），
            // 切栏时弹窗不伸缩；窗口太矮时收缩、滚动兜底。内容内缩 4pt：
            // focus ring 画在控件外侧 3pt，贴视口边会被裁掉。
            scroll()
                .h(px(448.0))
                .min_h(px(160.0))
                .flex_shrink(1.0)
                .child(view().p(px(4.0)).child(dynamic(move || match tab.get() {
                    1 => cover_pane(draft).into_element(),
                    2 => lyrics_pane(draft).into_element(),
                    _ => basic_pane(draft).into_element(),
                }))),
        )
        .child(when(
            move || draft.with(|d| d.error.is_some()),
            move || {
                text(move || draft.with(|d| d.error.clone().unwrap_or_default()))
                    .text_size(px(12.0))
                    .line_height(px(16.0))
                    .text_color(nsh_base::theme().destructive)
            },
        ))
        .child(
            dialog_footer()
                .child(text_button(
                    Variant::Outline,
                    Size::Default,
                    None,
                    "取消",
                    close,
                ))
                .child(
                    text_button(Variant::Default, Size::Default, None, "保存", move || {
                        save(draft)
                    })
                    .disabled(move || draft.with(|d| d.saving || !valid(d))),
                ),
        );
    modal(content, close)
}
