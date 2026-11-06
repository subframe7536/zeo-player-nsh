//! 「编辑元数据」弹窗：单曲目的标签编辑，分「基础元数据 / 封面 / 歌词」
//! 三栏；保存走 `store::save_tags`（原子写回 + 索引刷新 + 当前曲重载）。

use std::collections::HashMap;
use std::path::Path;

use nsh_core::prelude::*;

use super::super::components::button::{RADIUS_MD, Size, Variant, text_button};
use super::super::components::cover::cover;
use super::super::components::form::{
    dialog_content, dialog_footer, dialog_header, input_field, modal, select, tab_list,
    tab_trigger, trigger_chevron,
};
use super::super::components::icons::{Icon, icon};
use super::super::store::{self, store};
use super::super::theme::*;
use crate::metadata::write::{self, CoverEdit, Edit, TagEdits, TagField};

/// 弹窗里的草稿：打开时从文件读出初值，保存时与原值比较生成 `TagEdits`
/// （没动的字段 Keep，不触发无谓的重写 / 多值键合并）。
#[derive(Debug, Clone, PartialEq)]
struct TagDraft {
    path: String,
    original: HashMap<TagField, String>,
    values: HashMap<TagField, String>,
    /// `None`：封面不动。
    cover: Option<CoverEdit>,
    /// `Replace` 时新图的本地路径（预览用文件路径，不用 data URI）。
    cover_src: Option<String>,
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
    let lrc_path = crate::engine::Meta::external_lrc(path.clone());
    let lrc_original = lrc_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let draft = signal(TagDraft {
        path,
        original: values.clone(),
        values,
        cover: None,
        cover_src: None,
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
                    tab_trigger(
                        move || tab.get() == 0,
                        None,
                        "基础",
                        move || tab.set(0),
                    )
                    .flex_1(),
                )
                .child(
                    tab_trigger(
                        move || tab.get() == 1,
                        None,
                        "封面",
                        move || tab.set(1),
                    )
                    .flex_1(),
                )
                .child(
                    tab_trigger(
                        move || tab.get() == 2,
                        None,
                        "歌词",
                        move || tab.set(2),
                    )
                    .flex_1(),
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
                .child(view().p(px(4.0)).child(dynamic(move || {
                    match tab.get() {
                        1 => cover_pane(draft).into_element(),
                        2 => lyrics_pane(draft).into_element(),
                        _ => basic_pane(draft).into_element(),
                    }
                }))),
        )
        .child(when(
            move || draft.with(|d| d.error.is_some()),
            move || {
                text(move || {
                    draft.with(|d| d.error.clone().unwrap_or_default())
                })
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

/// 草稿 → 写回改动：与原值相同的字段 Keep；改空的 Clear。歌词按
/// `lrc_target` 只走一条道——写 `.lrc` 时内嵌标签原样留着（另一栏的
/// 草稿不生效）。
fn build_edits(d: &TagDraft) -> TagEdits {
    let fields = TagField::ALL
        .iter()
        .filter_map(|f| {
            if d.lrc_target && *f == TagField::Lyrics {
                return None;
            }
            let old = d.original.get(f).map(String::as_str).unwrap_or_default();
            let new = d.values.get(f).map(|s| s.trim()).unwrap_or_default();
            if new == old {
                None
            } else if new.is_empty() {
                Some((*f, Edit::Clear))
            } else {
                Some((*f, Edit::Set(new.to_owned())))
            }
        })
        .collect();
    let lrc = if !d.lrc_target {
        None
    } else {
        let new = d.lrc_text.trim();
        if new == d.lrc_original.trim() {
            None
        } else if new.is_empty() {
            Some(Edit::Clear)
        } else {
            Some(Edit::Set(new.to_owned()))
        }
    };
    TagEdits {
        fields,
        cover: d.cover.clone(),
        lrc,
    }
}

/// 数字字段本地校验（与 `write::validate` 同规）：音轨 / 碟号必须正整数，
/// 年份 `u16` 整数；为空合法（Clear）。
fn valid(d: &TagDraft) -> bool {
    let numeric = [
        TagField::Year,
        TagField::Track,
        TagField::TrackTotal,
        TagField::Disc,
        TagField::DiscTotal,
    ];
    numeric.iter().all(|f| {
        let Some(v) = d.values.get(f) else { return true };
        let v = v.trim();
        if v.is_empty() {
            return true;
        }
        if *f == TagField::Year {
            v.parse::<u16>().is_ok()
        } else {
            v.parse::<u32>().is_ok_and(|n| n > 0)
        }
    })
}

fn save(draft: Signal<TagDraft>) {
    let s = store();
    let d = draft.get_untracked();
    draft.update(|d| {
        d.saving = true;
        d.error = None;
    });
    store::save_tags(d.path.clone(), build_edits(&d), move |result| match result {
        Ok(()) => s.edit_info.set(None),
        Err(error) => draft.update(|d| {
            d.saving = false;
            d.error = Some(error);
        }),
    });
}

// --- 基础元数据 ----------------------------------------------------------------

/// 「基础元数据」栏：文本字段 + 流派标签输入 + 数字行。
fn basic_pane(draft: Signal<TagDraft>) -> View {
    view()
        .gap(px(10.0))
        .child(field_row("标题", TagField::Title, draft, true))
        .child(field_row("歌手", TagField::Artist, draft, false))
        .child(field_row("专辑", TagField::Album, draft, false))
        .child(field_row("专辑歌手", TagField::AlbumArtist, draft, false))
        .child(genre_row(draft))
        .child(number_row(draft))
        .child(disc_row(draft))
        .child(field_row("作曲", TagField::Composer, draft, false))
        .child(field_row("注释", TagField::Comment, draft, false))
}

/// 一行 `标签 + 输入框`。
fn field_row(
    label: &'static str,
    field: TagField,
    draft: Signal<TagDraft>,
    auto_focus: bool,
) -> View {
    let initial = untracked(|| {
        draft
            .with(|d| d.values.get(&field).cloned())
            .unwrap_or_default()
    });
    let input = input_field(false)
        .flex_1()
        .label(label)
        .default_value(initial)
        .auto_focus(auto_focus)
        .readonly(move || draft.with(|d| d.saving))
        .on_change(move |t: &str| {
            draft.update(|d| {
                d.values.insert(field, t.to_owned());
            })
        });
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            text(label)
                .w(px(56.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(input)
}

/// 窄数字输入框（行内并排用）。
fn number_input(
    label: &'static str,
    field: TagField,
    draft: Signal<TagDraft>,
    width: f32,
) -> Input {
    let initial = untracked(|| {
        draft
            .with(|d| d.values.get(&field).cloned())
            .unwrap_or_default()
    });
    input_field(false)
        .w(px(width))
        .label(label)
        .default_value(initial)
        .readonly(move || draft.with(|d| d.saving))
        .on_change(move |t: &str| {
            draft.update(|d| {
                d.values.insert(field, t.to_owned());
            })
        })
}

/// 年份、音轨号 / 总数（`n/m` 拆成两格）。
fn number_row(draft: Signal<TagDraft>) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            text("年份")
                .w(px(56.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(number_input("年份", TagField::Year, draft, 72.0))
        .child(
            text("音轨")
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(number_input("音轨号", TagField::Track, draft, 56.0))
        .child(
            text("/")
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(faint()),
        )
        .child(number_input("音轨总数", TagField::TrackTotal, draft, 56.0))
}

/// 碟号 / 碟总数。
fn disc_row(draft: Signal<TagDraft>) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            text("碟号")
                .w(px(56.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(number_input("碟号", TagField::Disc, draft, 56.0))
        .child(
            text("/")
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(faint()),
        )
        .child(number_input("碟总数", TagField::DiscTotal, draft, 56.0))
}

// --- 流派标签输入器 --------------------------------------------------------------

/// `;` 分隔串 → 标签列表（草稿里流派仍是 "a; b" 形式，写回格式不变）。
fn split_genres(raw: &str) -> Vec<String> {
    raw.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 流派：标签输入器 + 下拉。chips 在 combobox 的 `prefix`；输入即过滤
/// 曲库已有流派（`s.tracks` 去重），点选 / 回车选中，回车对第一项生效
/// ——查询不是已有流派时第一项是「添加“…”」自由新增。选过的项再选会
/// 取消勾选（等同删 chip）。去重忽略大小写，草稿存 `;` 合并串。
fn genre_row(draft: Signal<TagDraft>) -> View {
    let s = store();
    let t = nsh_base::theme();
    let (input_c, ring, popover_c) = (t.input, t.ring, t.popover);
    let gate = store::menu_control();
    let query = signal(String::new());
    let genres = move || {
        draft.with(|d| {
            split_genres(
                d.values
                    .get(&TagField::Genre)
                    .map(String::as_str)
                    .unwrap_or_default(),
            )
        })
    };
    let set_genres = move |list: Vec<String>| {
        draft.update(|d| {
            d.values.insert(TagField::Genre, list.join("; "));
        })
    };
    // 下拉项：曲库已有流派（排序去重）；查询串不是已有流派时最上面
    // 塞一项「添加“{查询}”」，回车 / 点选即自由新增。
    let items = move || {
        let q = query.get();
        let q = q.trim();
        let mut known: Vec<String> = Vec::new();
        s.tracks.with(|tracks| {
            for g in tracks.iter().flat_map(|t| &t.genres) {
                if !known.iter().any(|k| k == g) {
                    known.push(g.clone());
                }
            }
        });
        known.sort();
        let mut items = Vec::with_capacity(known.len() + 1);
        if !q.is_empty() && !known.iter().any(|g| g.eq_ignore_ascii_case(q)) {
            items.push(nsh_base::SelectItem::new(
                q.to_owned(),
                format!("添加“{q}”"),
            ));
        }
        items.extend(
            known
                .into_iter()
                .map(|g| nsh_base::SelectItem::new(g.clone(), g)),
        );
        items
    };
    let chips = move || {
        // chips 占满输入框剩下的宽度并在框内折行；最多两行高
        // （2×24+4），再多的在框内滚——字段高度封顶，弹窗才不会
        // 被流派撑出滚动条 / 横向裁掉。
        scroll()
            .flex_1()
            .min_w(px(0.0))
            .max_h(px(52.0))
            .child(
                view()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap(px(4.0))
                    .child(dynamic(move || {
                        genres()
                            .into_iter()
                            .map(|g| genre_chip(draft, g).into_element())
                            .collect::<Vec<_>>()
                    })),
            )
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            text("流派")
                .w(px(56.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
        .child(
            nsh_base::combobox()
                .flex_1()
                .min_w(px(0.0))
                .min_h(px(36.0))
                // 根行默认顶对齐：chips 折行撑高后输入框 / chevron 会贴顶，
                // 居中才对。
                .items_center()
                .px(px(8.0))
                .py(px(5.0))
                .gap(px(6.0))
                .rounded(px(RADIUS_MD))
                .border_1()
                .border_color(input_c)
                .shadow_xs()
                .dark(move |st| st.bg(input_c.opacity(0.3)))
                .focus_within(move |st| {
                    st.border_color(ring)
                        .ring(px(3.0))
                        .ring_color(ring.opacity(0.5))
                })
                .items(items)
                .multiple(true)
                .value(move || {
                    Some(
                        genres()
                            .into_iter()
                            .map(SharedString::from)
                            .collect::<Vec<_>>(),
                    )
                })
                .query(move || Some(SharedString::from(query.get())))
                .on_query_change(move |q: &SharedString| query.set(q.to_string()))
                .open(gate.open())
                .on_open_change(gate.combobox_change())
                .on_change(move |v: &Vec<SharedString>| {
                    let mut list: Vec<String> = Vec::new();
                    for x in v {
                        let g = x.trim().to_owned();
                        if !g.is_empty() && !list.iter().any(|k| k.eq_ignore_ascii_case(&g)) {
                            list.push(g);
                        }
                    }
                    set_genres(list);
                    query.set(String::new());
                })
                .placeholder(Some(SharedString::from("回车添加")))
                .accessibility_label(Some(SharedString::from("流派")))
                .disabled(move || draft.with(|d| d.saving))
                .placement(Placement::Bottom)
                .align(Align::Start)
                .offset(px(4.0))
                .margin(px(8.0))
                .popup_style(move |st| {
                    st.p(px(4.0))
                        .rounded(px(10.0))
                        .overflow_hidden()
                        .border_1()
                        .border_color(border())
                        .bg(popover_c)
                        .shadow_lg()
                })
                // 行高 = 复选框 16 + py(6)×2 = 28；224 恰好 8 行，
                // 不会在弹层底边把最后一行切成半行。
                .list_style(|st| st.max_h(px(224.0)))
                .item_class("focus:bg-hover")
                .item_style(|st| {
                    st.px(px(8.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                })
                .render_item(move |entry: &nsh_base::SelectEntry| genre_item(draft, entry))
                .input_style(move |st| {
                    let st = st
                        .h(px(24.0))
                        .text_size(px(13.0))
                        .text_color(fg());
                    // 空字段时撑满（点哪里都能聚焦）；有 chips 时只占一
                    // 小段，把行留给 chips 折行。
                    if genres().is_empty() {
                        st.flex_1().min_w(px(72.0))
                    } else {
                        st.w(px(96.0))
                    }
                })
                .prefix(when(move || !genres().is_empty(), move || chips()))
                .trigger_button(
                    view()
                        .px(px(2.0))
                        .items_center()
                        .justify_center()
                        .child(trigger_chevron()),
                )
                .empty(
                    text("没有可添加的流派")
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(faint()),
                ),
        )
}

/// 下拉行：左侧复选框（选中打勾）+ 流派名。「添加“…”」项不在已选集
/// 里，自然显示空框。
fn genre_item(draft: Signal<TagDraft>, entry: &nsh_base::SelectEntry) -> View {
    let value = entry.value.to_string();
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            view()
                .w(px(16.0))
                .h(px(16.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(border())
                .items_center()
                .justify_center()
                .child(when(
                    move || {
                        draft.with(|d| {
                            split_genres(
                                d.values
                                    .get(&TagField::Genre)
                                    .map(String::as_str)
                                    .unwrap_or_default(),
                            )
                            .iter()
                            .any(|g| *g == value)
                        })
                    },
                    || icon(Icon::Check, 10.0, fg),
                )),
        )
        .child(
            text(entry.title.to_string())
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(fg()),
        )
}

/// 一个流派 chip：文字 + × 按钮。
fn genre_chip(draft: Signal<TagDraft>, name: String) -> View {
    let remove = {
        let name = name.clone();
        move || {
            if draft.with(|d| d.saving) {
                return;
            }
            draft.update(|d| {
                let mut list = split_genres(
                    d.values
                        .get(&TagField::Genre)
                        .map(String::as_str)
                        .unwrap_or_default(),
                );
                list.retain(|x| *x != name);
                d.values.insert(TagField::Genre, list.join("; "));
            });
        }
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .h(px(24.0))
        .pl(px(8.0))
        .pr(px(4.0))
        .rounded(px(6.0))
        .bg(muted())
        .child(
            text(name)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(fg()),
        )
        .child(
            view()
                .h(px(16.0))
                .w(px(16.0))
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .hover(|st| st.bg(hover()))
                .role(AccessibilityRole::Button)
                .label("移除流派")
                .on_press(move || remove())
                .child(icon(Icon::Close, 10.0, muted_fg)),
        )
}

// --- 封面 ----------------------------------------------------------------------

/// 「封面」栏：96px 预览（新图 / 现有封面 / 占位符）+ 更换 / 移除按钮。
fn cover_pane(draft: Signal<TagDraft>) -> View {
    let has_art = untracked(|| {
        draft.with(|d| {
            crate::metadata::read_meta(Path::new(&d.path))
                .map(|m| m.has_art)
                .unwrap_or(false)
        })
    });
    let src = move || {
        let d = draft.get();
        match &d.cover {
            // 换封面：预览本地图片文件。
            Some(CoverEdit::Replace { .. }) => d.cover_src.clone().map(SharedString::from),
            // 移除封面：显示占位符。
            Some(CoverEdit::Remove) => None,
            None => {
                if has_art {
                    store::cover_for_always(&d.path, 96.0)
                } else {
                    None
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
                let mime = write::cover_mime(&file)
                    .ok_or_else(|| "不支持的图片格式（PNG / JPEG / WebP / BMP / GIF）".to_owned())?;
                std::fs::read(&file)
                    .map_err(|e| format!("读取图片失败: {e}"))
                    .map(|data| (data, mime))
            })();
            match outcome {
                Ok((data, mime)) => draft.update(|d| {
                    d.cover = Some(CoverEdit::Replace { data, mime });
                    d.cover_src = Some(file.to_string_lossy().into_owned());
                    d.error = None;
                }),
                Err(error) => draft.update(|d| d.error = Some(error)),
            }
        });
    };
    let remove = move || {
        draft.update(|d| {
            d.cover = Some(CoverEdit::Remove);
            d.cover_src = None;
            d.error = None;
        })
    };
    view()
        .gap(px(10.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(16.0))
                .child(cover(src, Some(96.0), 8.0))
                .child(
                    view()
                        .gap(px(8.0))
                        .child(text_button(
                            Variant::Outline,
                            Size::Sm,
                            None,
                            "更换图片…",
                            pick,
                        ))
                        .child(
                            text_button(Variant::Outline, Size::Sm, None, "移除封面", remove)
                                .disabled(move || {
                                    draft.with(|d| d.saving || (!has_art && d.cover.is_none()))
                                }),
                        ),
                ),
        )
        .child(
            text("保存后替换文件里的全部内嵌图片。")
                .text_size(px(12.0))
                .text_color(faint()),
        )
}

// --- 歌词 ----------------------------------------------------------------------

/// 「歌词」栏：「保存到」选 `.lrc` 侧车文件或内嵌标签（默认跟已有文件
/// 走），多行输入；两种目标各自的草稿互不影响（切回去内容还在），
/// 保存时只写选中的那处。
fn lyrics_pane(draft: Signal<TagDraft>) -> View {
    let lrc_exists = untracked(|| draft.with(|d| d.lrc_exists));
    let file_mode = move || draft.with(|d| d.lrc_target);
    let target = |file: bool| -> Box<dyn Fn()> {
        Box::new(move || draft.update(|d| d.lrc_target = file))
    };
    view()
        .gap(px(8.0))
        .child(
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
                            ".lrc 文件"
                        } else {
                            "内嵌标签"
                        }
                        .to_owned()
                    },
                    "歌词保存位置",
                    vec![
                        (None, ".lrc 文件", target(true)),
                        (None, "内嵌标签", target(false)),
                    ],
                    130.0,
                )),
        )
        .child(when(move || file_mode(), move || {
            text(if lrc_exists {
                "保存到同名 .lrc 文件；清空会删除它。"
            } else {
                "保存时新建同名 .lrc 文件。"
            })
            .text_size(px(12.0))
            .text_color(faint())
        }))
        .child(when(move || !file_mode() && lrc_exists, move || {
            text("检测到同名 .lrc——「歌词来源」本地优先时，播放页显示它而不是内嵌歌词。")
                .text_size(px(12.0))
                .text_color(faint())
        }))
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
fn lyrics_area(draft: Signal<TagDraft>, file: bool) -> Input {
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
