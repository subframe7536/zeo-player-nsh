//! 歌单界面：侧栏的歌单区、歌单页、曲目的右键菜单（添加到歌单 / 从歌单
//! 移除）、新建与编辑弹窗（智能歌单的条件编辑器）。

use std::collections::BTreeMap;
use std::rc::Rc;

use nsh_base::{context_menu, menu, submenu};
use nsh_core::prelude::*;

use super::super::Nav;
use super::super::components::button::{
    Size, Variant, icon_button, outline_button, pill_button, text_button,
};
use super::super::components::form::{
    SelectOption, cover_realign, cover_realign_flex, dialog_content, dialog_footer, dialog_header,
    input_field, modal, select, select_trigger, tab_list, tab_trigger,
};
use super::super::components::icons::{Icon, icon};
use super::super::components::page::{empty_state, page_header};
use super::super::components::tracks::{Columns, track_columns};
use super::super::store::{self, Draft, format_total, play_list, store};
use super::super::theme::*;
use super::{item, popup, separator};
use crate::config::{TrackEntry, unix_now};
use crate::playlists::{Condition, Field, FieldType, Op, Playlist, Rule, Sort};

// --- 菜单部件 -----------------------------------------------------------------

/// 「添加到歌单」子菜单：新建歌单…，再列出全部手动歌单。`paths` 在选中时取。
pub fn add_to_submenu(paths: impl Fn() -> Vec<String> + Clone + 'static) -> AnyElement {
    let s = store();
    let create = paths.clone();
    submenu()
        .trigger(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(6.0))
                .role(AccessibilityRole::Menuitem)
                .label("添加到歌单")
                .hover(|st| st.bg(hover()))
                .child(icon(Icon::Plus, 15.0, muted_fg))
                .child(
                    text("添加到歌单")
                        .flex_1()
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(fg()),
                )
                .child(icon(Icon::ChevronRight, 12.0, faint)),
        )
        .child(item("新建歌单…", Some(Icon::Plus), move || {
            store::new_playlist(false, create())
        }))
        .child(dynamic(move || {
            let manual: Vec<Playlist> = s
                .playlists
                .with(|l| l.iter().filter(|p| !p.is_smart()).cloned().collect());
            let mut out = Vec::new();
            if !manual.is_empty() {
                out.push(separator());
            }
            for p in manual {
                let paths = paths.clone();
                let id = p.id;
                let glyph = if p.favorite {
                    Icon::Heart
                } else {
                    Icon::Playlist
                };
                out.push(item(p.name, Some(glyph), move || {
                    store::add_to_playlist(id, paths())
                }));
            }
            out
        }))
        .into_element()
}

/// 曲目的右键菜单（包着 `row`）：播放、添加到歌单、从歌单移除（`from`：
/// 当前所在的手动歌单）、在访达中显示、重命名文件（`rename`，文件夹页
/// 给）、删除记录 / 删除文件。
pub fn track_menu(
    row: impl IntoElement,
    track: Memo<Option<TrackEntry>>,
    from: Option<u64>,
    play: impl Fn() + 'static,
    rename: Option<Rc<dyn Fn(&str)>>,
) -> AnyElement {
    let gate = store::menu_control();
    let path = move || track.with_untracked(|t| t.as_ref().map(|t| t.path.clone()));
    let paths = move || path().into_iter().collect::<Vec<_>>();
    let menu = context_menu()
        .trigger(row)
        .open(gate.open())
        .on_open_change(gate.menu_change());
    popup!(menu, 220.0)
        .child(item("播放", Some(Icon::Play), play))
        .child(separator())
        .child(add_to_submenu(paths))
        .child(from.map(|id| {
            item("从歌单中移除", Some(Icon::Close), move || {
                // 等菜单关掉再移除：移除会拆掉这一行（连同它的菜单）。
                if let Some(p) = path() {
                    store::later(move || store::remove_from_playlist(id, &p));
                }
            })
        }))
        .child(separator())
        .child(item("编辑元数据…", Some(Icon::Edit), move || {
            if let Some(p) = path() {
                store::open_edit_info(&p);
            }
        }))
        .child(item(
            super::super::now_playing::REVEAL_LABEL,
            Some(Icon::Reveal),
            move || {
                if let Some(p) = path() {
                    super::super::now_playing::reveal(&p);
                }
            },
        ))
        .child(rename.map(|rename| {
            item("重命名文件…", Some(Icon::Edit), move || {
                if let Some(p) = path() {
                    rename(&p);
                }
            })
        }))
        .child(separator())
        .child(item("从曲库中删除", Some(Icon::Close), move || {
            // 等菜单关掉再删：删掉会拆掉这一行（连同它的菜单）。
            if let Some(p) = path() {
                store::later(move || store::remove_record(&p));
            }
        }))
        .child(item("删除文件…", Some(Icon::Trash), move || {
            if let Some(p) = path() {
                store::ask_delete_file(&p);
            }
        }))
        .into_element()
}

// --- 侧栏 ---------------------------------------------------------------------

/// 「新建歌单」菜单（表头的「+」与图标栏的加号行共用弹层内容）。
fn add_playlist_menu(trigger: View) -> AnyElement {
    let gate = store::menu_control();
    let add = menu()
        .offset(px(4.0))
        .trigger(trigger)
        .open(gate.open())
        .on_open_change(gate.menu_change());
    popup!(add, 200.0)
        .child(item("新建歌单", Some(Icon::Playlist), || {
            store::new_playlist(false, Vec::new())
        }))
        .child(item("新建智能歌单", Some(Icon::Smart), || {
            store::new_playlist(true, Vec::new())
        }))
        .into_element()
}

/// 侧栏的歌单区：小标题 + 新建按钮，下面是歌单列表（可滚动）。
/// 添加歌单的按钮（带下拉菜单）。
pub(crate) fn add_playlist_button() -> AnyElement {
    add_playlist_menu(
        view()
            .size(px(22.0))
            .rounded(px(6.0))
            .items_center()
            .justify_center()
            .role(AccessibilityRole::Button)
            .label("添加歌单")
            .window_drag(false)
            .hover(|st| st.bg(move || primary().opacity(0.08)))
            .active(|st| st.opacity(0.6))
            .child(icon(Icon::Plus, 14.0, faint)),
    )
}

/// 侧栏歌单列表（滚动区）。
pub(crate) fn sidebar_list(rail: impl Fn() -> bool + Copy + 'static) -> impl IntoElement {
    let s = store();
    let fade = super::super::frame::label_fade(rail);
    // 图标栏时顶替表头「+」的一行：只在 rail 出现（不占高度），
    // 加号与其他行的图标同列。
    let add_row = add_playlist_menu(
        view()
            .w_full()
            .h(move || px(if rail() { 34.0 } else { 0.0 }))
            .opacity(move || if rail() { 1.0_f32 } else { 0.0 })
            .overflow_hidden()
            .flex_row()
            .items_center()
            .pl(px(10.0))
            .rounded(px(8.0))
            .role(AccessibilityRole::Button)
            .label("新建歌单")
            .hover(|st| st.bg(move || primary().opacity(0.08)))
            .active(|st| st.opacity(0.7))
            .child(
                view()
                    .size(px(20.0))
                    .items_center()
                    .justify_center()
                    .child(icon(Icon::Plus, 17.0, sidebar_fg)),
            ),
    );
    scroll().flex_1().child(
        view()
            .px(px(10.0))
            .pb(px(8.0))
            .gap(px(2.0))
            .child(add_row)
            .child(dynamic(move || {
                let lists = s.playlists.get();
                // 「我喜欢」固定在最前（`ensure_favorites`），其余按创建顺序。
                let mut rows: Vec<AnyElement> =
                    lists.iter().map(|p| sidebar_row(p, rail)).collect();
                if lists.iter().all(|p| p.favorite) {
                    rows.push(
                        view()
                            .opacity(fade)
                            .child(
                                text("点 + 新建歌单，或在曲目上点右键添加。")
                                    .text_size(px(12.0))
                                    .line_height(px(18.0))
                                    .text_color(faint())
                                    .px(px(10.0))
                                    .py(px(4.0)),
                            )
                            .into_element(),
                    );
                }
                rows
            })),
    )
}

fn sidebar_row(p: &Playlist, rail: impl Fn() -> bool + Copy + 'static) -> AnyElement {
    let (id, name, smart, favorite) = (p.id, p.name.clone(), p.is_smart(), p.favorite);
    let nav = global::<Nav>();
    let active = move || nav.get() == Nav::Playlist(id);
    let glyph = if favorite {
        Icon::Heart
    } else if smart {
        Icon::Smart
    } else {
        Icon::Playlist
    };
    // 按钮不能挂 `window_drag(false)`（`Interactive` 不在组件元素上），
    // 同 `page_header` 的返回键：裸 view 包一层。
    let row = view()
        .window_drag(false)
        .child(super::super::frame::adaptive_row(
            rail,
            glyph,
            name.clone().into(),
            active,
            move || {
                store().search.set(String::new());
                store().search_open.set(false);
                nav.set(Nav::Playlist(id));
            },
        ));
    // 「我喜欢」不能改名或删除：没有右键菜单。
    if favorite {
        return row.into_element();
    }
    let gate = store::menu_control();
    let menu = context_menu()
        .trigger(row)
        .open(gate.open())
        .on_open_change(gate.menu_change());
    popup!(menu, 180.0)
        .child(item(
            if smart {
                "编辑条件…"
            } else {
                "重命名…"
            },
            Some(Icon::Edit),
            move || store::edit_playlist(id),
        ))
        .child(separator())
        .child(item("删除歌单", Some(Icon::Trash), move || {
            confirm_delete(id)
        }))
        .into_element()
}

/// 删除前确认（系统对话框）。
pub fn confirm_delete(id: u64) {
    let Some(p) = store::playlist(id) else { return };
    let body = if p.is_smart() {
        "只删除歌单本身，曲库里的文件不受影响。".to_owned()
    } else {
        "歌单里的曲目只会从歌单中移除，文件不受影响。".to_owned()
    };
    let answer = nsh_base::alert_dialog()
        .title(format!("删除「{}」？", p.name))
        .description(body)
        .cancel("cancel", "取消")
        .destructive("delete", "删除")
        .show();
    spawn(async move {
        if answer.await.map(|r| r.action) == Ok(Some("delete".into())) {
            store::delete_playlist(id);
        }
    });
}

// --- 歌单页 -------------------------------------------------------------------

/// 歌单页：头部（名称、种类与曲目数、播放 / 编辑 / 删除）+ 曲目列表。
pub fn page(id: u64) -> View {
    let s = store();
    let list = computed(move || s.playlists.with(|l| l.iter().find(|p| p.id == id).cloned()));
    let items = computed(move || {
        let tracks = s.tracks.get();
        Rc::new(list.with(|p| p.as_ref().map(|p| p.tracks(&tracks)).unwrap_or_default()))
    });
    let smart = move || list.with(|p| p.as_ref().is_some_and(Playlist::is_smart));
    let favorite = move || list.with(|p| p.as_ref().is_some_and(|p| p.favorite));
    let title = move || list.with(|p| p.as_ref().map(|p| p.name.clone()).unwrap_or_default());
    let subtitle = move || {
        let kind = if smart() { "智能歌单" } else { "歌单" };
        items.with(|v| {
            let total: f64 = v.iter().map(|t| t.duration).sum();
            if v.is_empty() {
                kind.to_owned()
            } else {
                format!("{kind} · {} 首 · {}", v.len(), format_total(total))
            }
        })
    };
    // 「我喜欢」只有「播放」，不能改名或删除。
    let actions = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(when(
            move || items.with(|v| !v.is_empty()),
            move || {
                pill_button(Icon::Play, "播放", move || {
                    items.with_untracked(|v| play_list(v, 0))
                })
            },
        ))
        .child(when(
            move || !favorite(),
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        when(smart, move || {
                            outline_button(Icon::Edit, "编辑条件", move || {
                                store::edit_playlist(id)
                            })
                        })
                        .otherwise(move || {
                            outline_button(Icon::Edit, "重命名", move || {
                                store::edit_playlist(id)
                            })
                        }),
                    )
                    .child(icon_button(
                        || Icon::Trash,
                        16.0,
                        "删除歌单",
                        muted_fg,
                        move || confirm_delete(id),
                    ))
            },
        ));
    let header = view()
        .child(page_header(
            title,
            subtitle,
            None,
            Some(actions.into_element()),
        ))
        .child(track_columns(Columns::ALL));
    let empty = dynamic(move || {
        if smart() {
            empty_state(
                Icon::Smart,
                "没有符合条件的曲目",
                "调整条件后，符合的曲目会自动出现在这里。",
            )
            .child(view().mt(px(12.0)).child(outline_button(
                Icon::Edit,
                "编辑条件",
                move || store::edit_playlist(id),
            )))
        } else if favorite() {
            empty_state(
                Icon::Heart,
                "还没有喜欢的曲目",
                "点播放条上的心形按钮，喜欢的曲目会出现在这里。",
            )
        } else {
            empty_state(
                Icon::Playlist,
                "歌单是空的",
                "在任意曲目上点右键，选「添加到歌单」。",
            )
        }
    });
    let from = untracked(|| list.with(|p| p.as_ref().filter(|p| !p.is_smart()).map(|p| p.id)));
    view()
        .flex_1()
        .child(super::track_list_in(items, Columns::ALL, header.into_element(), from).empty(empty))
}

// --- 编辑弹窗 -----------------------------------------------------------------

/// 新建 / 编辑歌单的弹窗（`store().editor` 有草稿时显示）。
pub fn editor_dialog() -> AnyElement {
    let s = store();
    let draft = signal(s.editor.get_untracked().unwrap_or(Draft {
        id: None,
        name: String::new(),
        rule: None,
        add: Vec::new(),
    }));
    let close = move || s.editor.set(None);
    let save = move || {
        store::save_draft(draft.get_untracked());
    };
    let smart = untracked(|| draft.with(|d| d.rule.is_some()));
    let heading = untracked(|| match (draft.with(|d| d.id.is_some()), smart) {
        (false, false) => "新建歌单",
        (false, true) => "新建智能歌单",
        (true, false) => "重命名歌单",
        (true, true) => "编辑智能歌单",
    });
    let name_input = input_field(false)
        .w_full()
        .label("歌单名称")
        .auto_focus(true)
        .default_value(untracked(|| draft.with(|d| d.name.clone())))
        .placeholder("歌单名称")
        .on_change(move |t: &str| draft.update(|d| d.name = t.to_owned()))
        .on_submit(move |_: &str| save());
    let description = if smart {
        Some("按条件从曲库自动聚合，曲库变化时跟着更新。")
    } else {
        None
    };
    let content = dialog_content(if smart { 640.0 } else { 425.0 })
        .child(dialog_header(heading, description))
        .child(name_input)
        .child(smart.then(|| rule_editor(draft)))
        .child(
            dialog_footer()
                .child(text_button(
                    Variant::Outline,
                    Size::Default,
                    None,
                    "取消",
                    close,
                ))
                .child(text_button(
                    Variant::Default,
                    Size::Default,
                    None,
                    if heading.starts_with("新建") {
                        "创建"
                    } else {
                        "保存"
                    },
                    save,
                )),
        );
    modal(content, close)
}

/// 智能规则编辑：全部 / 任一、条件行、添加条件、排序、数量上限、实时匹配数。
fn rule_editor(draft: Signal<Draft>) -> View {
    let s = store();
    let rule = move |f: &dyn Fn(&Rule) -> String| {
        draft.with(|d| d.rule.as_ref().map(f).unwrap_or_default())
    };
    let change = move |f: Box<dyn FnOnce(&mut Rule)>| {
        draft.update(|d| {
            if let Some(r) = d.rule.as_mut() {
                f(r);
            }
        })
    };
    let count =
        computed(move || draft.with(|d| d.rule.as_ref().map(|r| r.conditions.len()).unwrap_or(0)));
    let matched = computed(move || {
        let tracks = s.tracks.get();
        draft.with(|d| {
            d.rule
                .as_ref()
                .map(|r| r.select(&tracks, unix_now()).len())
                .unwrap_or(0)
        })
    });
    let match_all = move || draft.with(|d| d.rule.as_ref().is_none_or(|r| r.match_all));
    let chip = move |label: &'static str, all: bool| {
        tab_trigger(
            move || match_all() == all,
            None,
            label,
            move || change(Box::new(move |r| r.match_all = all)),
        )
    };
    let mode = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text("符合以下")
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(muted_fg()),
        )
        .child(
            tab_list()
                .child(chip("全部", true))
                .child(chip("任一", false)),
        )
        .child(
            text("条件")
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(muted_fg()),
        )
        .child(view().flex_1())
        .child(
            text(move || format!("匹配 {} 首", matched.get()))
                .text_size(px(14.0))
                .line_height(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .tabular_nums()
                .text_color(move || primary())
                .transition_colors()
                .duration(300.0),
        );
    // 行数变化（添加 / 删除）时整组重建；改字段、条件、值只更新所在行。
    let rows = dynamic(move || {
        (0..count.get())
            .map(|i| condition_row(draft, i).into_element())
            .collect::<Vec<_>>()
    });
    let add = view().self_start().child(text_button(
        Variant::Ghost,
        Size::Sm,
        Some(Icon::Plus),
        "添加条件",
        move || change(Box::new(|r| r.conditions.push(Condition::default()))),
    ));
    let sort = select(
        move || rule(&|r| r.sort.label().to_owned()),
        "排序",
        Sort::ALL
            .iter()
            .map(|&o| SelectOption::new(o.label(), move || change(Box::new(move |r| r.sort = o))))
            .collect(),
        170.0,
    );
    let limit = input_field(true)
        .w(px(80.0))
        .label("数量上限")
        .keyboard_type(KeyboardType::Number)
        .default_value(untracked(|| {
            rule(&|r| r.limit.map(|n| n.to_string()).unwrap_or_default())
        }))
        .placeholder("不限")
        .on_change(move |t: &str| {
            let limit = t.trim().parse::<usize>().ok().filter(|n| *n > 0);
            change(Box::new(move |r| r.limit = limit));
        });
    let footer = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text("排序")
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(muted_fg()),
        )
        .child(sort)
        .child(view().w(px(12.0)))
        .child(
            text("最多")
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(muted_fg()),
        )
        .child(limit)
        .child(
            text("首")
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(muted_fg()),
        );
    view()
        .gap(px(12.0))
        .flex_shrink(1.0)
        .min_h(px(0.0))
        .child(mode)
        .child(
            scroll()
                .flex_shrink(1.0)
                .max_h(px(300.0))
                .child(view().gap(px(8.0)).child(rows)),
        )
        .child(add)
        .child(view().h(px(1.0)).bg(border()))
        .child(footer)
}

/// 一行条件：字段、比较方式、值、单位、删除。
fn condition_row(draft: Signal<Draft>, index: usize) -> View {
    let get = move || {
        draft.with(|d| {
            d.rule
                .as_ref()
                .and_then(|r| r.conditions.get(index).cloned())
                .unwrap_or_default()
        })
    };
    let change = move |f: Box<dyn FnOnce(&mut Condition)>| {
        draft.update(|d| {
            if let Some(c) = d.rule.as_mut().and_then(|r| r.conditions.get_mut(index)) {
                f(c);
            }
        })
    };
    let field = computed(move || get().field);
    let field_options = Field::ALL
        .iter()
        .enumerate()
        .map(|(i, &f)| {
            let heading = (i == 0 || Field::ALL[i - 1].group() != f.group()).then(|| f.group());
            let mut opt = SelectOption::new(f.label(), move || {
                change(Box::new(move |c| {
                    // 标签只能从列表里选，换过去时把自由文本的值也清掉。
                    if c.field.kind() != f.kind() || f == Field::Tag {
                        c.value.clear();
                    }
                    c.field = f;
                    if !f.ops().contains(&c.op) {
                        c.op = f.ops()[0];
                    }
                }))
            });
            if let Some(h) = heading {
                opt = opt.with_heading(h);
            }
            opt
        })
        .collect();
    let field_select = select(
        move || get().field.label().to_owned(),
        "字段",
        field_options,
        120.0,
    );
    // 比较方式随字段变：字段换了整个下拉重建。
    let op_select = dynamic(move || {
        let options = field
            .get()
            .ops()
            .iter()
            .map(|&o: &Op| {
                SelectOption::new(o.label(), move || change(Box::new(move |c| c.op = o)))
            })
            .collect();
        select(move || get().op.label().to_owned(), "条件", options, 110.0)
    });
    // 值：标签是从曲库现有标签里选，其余是输入框；换了字段（可能清空了值）
    // 时重建，打字时不重建。
    let value_input = dynamic(move || {
        let f = field.get();
        if f == Field::Tag {
            return tag_select(
                move || get().value,
                move |v| change(Box::new(move |c| c.value = v)),
            );
        }
        let placeholder = match f.kind() {
            FieldType::Text if f == Field::Format => "mp3 / flac / wav",
            FieldType::Text => "",
            FieldType::Number if f == Field::Duration => "3:30",
            FieldType::Number => "数值",
            FieldType::Date => "天数",
        };
        input_field(true)
            .flex_1()
            .min_w(px(80.0))
            .label("值")
            .default_value(untracked(|| get().value))
            .placeholder(placeholder)
            .on_change(move |t: &str| {
                let t = t.to_owned();
                change(Box::new(move |c| c.value = t));
            })
            .into_element()
    });
    let unit = move || {
        let c = get();
        match (c.field.kind(), c.field.unit()) {
            (FieldType::Date, _) => "天内".to_owned(),
            (_, unit) => unit.to_owned(),
        }
    };
    let invalid = move || {
        let c = get();
        !c.value.trim().is_empty() && !c.is_complete()
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        // 行比输入框高 8 pt：聚焦环（3 pt）才不会被滚动区裁掉。
        .p(px(4.0))
        .child(field_select)
        .child(op_select)
        .child(view().flex_1().flex_row().items_center().child(value_input))
        .child(
            text(move || {
                if invalid() {
                    "无效".to_owned()
                } else {
                    unit()
                }
            })
            .w(px(70.0))
            .number_of_lines(1)
            .text_size(px(12.0))
            .line_height(px(12.0))
            .text_color(move || {
                if invalid() {
                    nsh_base::theme().destructive
                } else {
                    faint()
                }
            }),
        )
        .child(icon_button(
            || Icon::Close,
            16.0,
            "删除条件",
            muted_fg,
            move || {
                draft.update(|d| {
                    if let Some(r) = d.rule.as_mut()
                        && index < r.conditions.len()
                    {
                        r.conditions.remove(index);
                    }
                })
            },
        ))
}

/// 标签条件的值：从曲库现有标签里选（按小写名排序，忽略大小写去重）。
/// nsh-base `select`：可搜索（标签多了以后），列表虚拟化。
fn tag_select(
    current: impl Fn() -> String + Copy + 'static,
    pick: impl Fn(String) + Copy + 'static,
) -> AnyElement {
    let s = store();
    let pulse = signal(0.0f32);
    // 弹性宽加在 select 元素上（根是列容器；触发器里的 flex_1 会在纵轴
    // 把它压没，只保 min_w 由列拉伸铺满）。宽度脉冲走根的 min_w，基准
    // 宽由触发器记（cover_realign_flex）。
    let (min_w, layout) = cover_realign_flex(pulse);
    let trigger = select_trigger(current, || None, "值", 0.0, "选择标签").on_layout(layout);
    let gate = store::menu_control();
    let no_tags = move || !s.tracks.get().iter().any(|t| !t.genres.is_empty());
    nsh_base::select()
        .flex_1()
        .min_w(min_w)
        .items(move || {
            let tracks = s.tracks.get();
            // 键用小写：匹配不分大小写，只差大小写的标签算同一个。
            let mut tags: BTreeMap<String, String> = BTreeMap::new();
            for g in tracks.iter().flat_map(|t| &t.genres) {
                tags.entry(g.to_lowercase()).or_insert_with(|| g.clone());
            }
            tags.into_values()
                .map(|tag| nsh_base::SelectItem::new(tag.clone(), tag))
                .collect::<Vec<_>>()
        })
        .value(move || {
            let cur = current();
            nsh_base::SelectValue::Controlled((!cur.is_empty()).then(|| cur.into()))
        })
        .searchable(true)
        .search_placeholder("搜索标签")
        .search_accessibility_label("搜索标签")
        .accessibility_label("值")
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(4.0))
        .margin(px(8.0))
        .open(gate.open())
        .on_open_change(gate.select_change())
        .on_open(move || cover_realign(pulse))
        .on_change(move |v: &Option<SharedString>| {
            if let Some(v) = v {
                pick(v.to_string())
            }
        })
        .popup_style(|st| {
            st.min_w(px(200.0))
                .p(px(4.0))
                .rounded(px(10.0))
                .overflow_hidden()
                .border_1()
                .border_color(border())
                .bg(nsh_base::theme().popover)
                .shadow_lg()
        })
        .item_class("focus:bg-hover")
        .item_style(|st| {
            st.px(px(10.0))
                .py(px(6.0))
                .rounded(px(6.0))
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(fg())
        })
        .search_style(|st| {
            st.h(px(32.0))
                .px(px(10.0))
                .text_size(px(13.0))
                .text_color(fg())
        })
        .empty(
            view().h(px(48.0)).items_center().justify_center().child(
                text(move || {
                    if no_tags() {
                        "曲库里还没有标签"
                    } else {
                        "没有匹配的标签"
                    }
                })
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
            ),
        )
        .trigger(trigger)
        .into_element()
}
