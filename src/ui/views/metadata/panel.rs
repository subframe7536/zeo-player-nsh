//! 右侧面板：单选（m==1）全字段编辑 + 顶栏内联改名 + 「音频参数」
//! 节（常开、值可复制）；多选（m≥2）批量修改（字段三态 + 音轨编号 /
//! 大小写 / 模板重命名工具 + 统一封面），写回走 `store::save_tags` /
//! `save_tags_batch`。操作都收在固定顶栏（单选「保存」/ 批量「应用」），
//! 状态行是滚动区第一条；滚动区末位不再放操作行。

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use nsh_core::prelude::*;

use super::super::super::components::button::{
    RADIUS_MD, Size, Variant, icon_button, shell, text_button,
};
use super::super::super::components::checkbox::checkbox;
use super::super::super::components::cover::cover;
use super::super::super::components::form::{SelectOption, input_field, select, trigger_chevron};
use super::super::super::components::icons::{Icon, icon};
use super::super::super::store::{self, poster};
use super::super::super::theme::*;
use super::super::folders::stop_if_playing_under;
use super::MetaRow;
use crate::config::{TrackEntry, library_store};
use crate::metadata::rename::{self, Status};
use crate::metadata::write::{self, CoverEdit, Edit, TagEdits, TagField};

/// 单选面板异步读的完整行：全部标签字段 + 读取来源的标签类型 + 入库
/// 时间（`db::added_at`，随同一后台读捎回——SQLite 读别上界面线程）。
type FullRow = (HashMap<TagField, String>, Option<&'static str>, Option<u64>);

const FULL_CACHE_CAP: usize = 32;
type FullCache = Mutex<Vec<(String, FullRow)>>;

fn full_cache() -> &'static FullCache {
    static CACHE: OnceLock<FullCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

fn get_cached_full(path: &str) -> Option<FullRow> {
    let mut cache = full_cache().lock().unwrap();
    let pos = cache.iter().position(|(p, _)| p == path)?;
    let entry = cache.remove(pos);
    let row = entry.1.clone();
    cache.push(entry);
    Some(row)
}

fn put_cached_full(path: &str, row: FullRow) {
    let mut cache = full_cache().lock().unwrap();
    if let Some(pos) = cache.iter().position(|(p, _)| p == path) {
        cache.remove(pos);
    } else if cache.len() >= FULL_CACHE_CAP {
        cache.remove(0);
    }
    cache.push((path.to_owned(), row));
}

/// 面板（自适应宽 280-380，左边框 + 浮动阴影）：单选表单 / 批量修改，按选中数切换——两个
/// 分支各按「单选路径 / 批量集合」memo，集合没变不重建（草稿留得住）。
pub(super) fn panel(
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    close: Rc<dyn Fn()>,
    open_rename: impl Fn() + Copy + 'static,
) -> View {
    let open_rename = Rc::new(open_rename);
    let single = computed(move || {
        selected.with(|set| (set.len() == 1).then(|| set.iter().next().unwrap().clone()))
    });
    let batch = computed(move || {
        let set = selected.get();
        (set.len() >= 2).then(|| Rc::new(set.iter().cloned().collect::<Vec<String>>()))
    });
    view()
        .w(move || {
            let cw = crate::ui::components::tracks::content_width();
            if cw < 760.0 {
                px(280.0)
            } else if cw < 960.0 {
                px(320.0)
            } else {
                px(380.0)
            }
        })
        .flex_shrink_0()
        .rounded(px(12.0))
        .border_1()
        .border_color(border())
        .shadow_xl()
        .bg(nsh_base::theme().card)
        .overflow_hidden()
        .child(dynamic(move || {
            if let Some(path) = single.get() {
                return single_panel(path, displayed, rows, selected, close.clone()).into_element();
            }
            if let Some(paths) = batch.get() {
                return batch_panel(paths, displayed, rows, close.clone(), open_rename.clone())
                    .into_element();
            }
            view().into_element()
        }))
}

/// 固定顶栏（滚动区外）：高 36pt 与表格表头等高对齐 + 底部分隔线，
/// 标题件横排后接一颗关闭钮（清空选中集，面板随之收掉）。
fn panel_header(parts: Vec<AnyElement>, close: Rc<dyn Fn()>) -> View {
    view()
        .h(px(36.0))
        .px(px(16.0))
        .border_b_1()
        .border_b_color(border())
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .children(parts)
        .child(icon_button(
            || Icon::Close,
            12.0,
            "关闭",
            muted_fg,
            move || {
                close();
            },
        ))
}

// --- 单选编辑 ------------------------------------------------------------------

/// 顶栏改名模式的状态：文件名主干输入与标签草稿分家——改名是文件
/// 操作（`rename::apply`，含同名侧车），不走 `TagEdits` / 保存钮。
#[derive(Clone, Copy)]
struct RenameState {
    /// 改名模式开着（顶栏整行换成输入 + ✓ / ✕）。
    renaming: Signal<bool>,
    /// 文件名主干（不含扩展名）输入。
    name_input: Signal<String>,
    /// `plan_name` 预检 / `apply` 执行报错（状态行显示，✓ 禁用）。
    err: Signal<Option<String>>,
    /// 后台改名进行中（输入只读、✓ ✕ 禁用）。
    busy: Signal<bool>,
}

#[derive(Debug, Clone, PartialEq)]
struct SingleDraft {
    path: String,
    original: HashMap<TagField, String>,
    values: HashMap<TagField, String>,
    /// 内嵌封面改动（`None` 不动）。
    cover: Option<CoverEdit>,
    /// `Replace` 时新图的本地路径（预览用）。
    cover_src: Option<String>,
    saving: bool,
    saved: bool,
    error: Option<String>,
}

fn track_seed(t: &TrackEntry) -> HashMap<TagField, String> {
    TagField::ALL
        .iter()
        .map(|&f| (f, entry_field_of(t, f)))
        .collect()
}

fn single_panel(
    path: String,
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    close: Rc<dyn Fn()>,
) -> View {
    // 完整字段值优先读 LRU 缓存与曲库索引条目（第 0 帧秒开，歌词与注释零延迟呈现）；
    // 未命中走后台异步读取（Lofty 跳过大封面与音频属性解析，亚毫秒完成）。
    let entry = displayed
        .with_untracked(|list| list.iter().find(|t| t.path == path).cloned())
        .or_else(|| {
            store::store()
                .tracks
                .with_untracked(|list| list.iter().find(|t| t.path == path).cloned())
        });
    let cached = get_cached_full(&path);
    let initial_values = cached
        .as_ref()
        .map(|(fields, _, _)| fields.clone())
        .or_else(|| entry.as_ref().map(track_seed))
        .unwrap_or_default();
    let full = signal(cached.clone());
    let alive = Arc::new(AtomicBool::new(true));
    cleanup({
        let alive = alive.clone();
        move || alive.store(false, Ordering::Relaxed)
    });
    let draft = signal(SingleDraft {
        path: path.clone(),
        original: initial_values.clone(),
        values: initial_values,
        cover: None,
        cover_src: None,
        saving: false,
        saved: false,
        error: None,
    });
    let rename_state = RenameState {
        renaming: signal(false),
        name_input: signal(
            Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        err: signal(None),
        busy: signal(false),
    };

    if full.get_untracked().is_none() {
        let read = path.clone();
        let alive_t = alive.clone();
        spawn(async move {
            let read_p = PathBuf::from(&read);
            let read_target = read.clone();
            let Ok(res) = blocking(move || {
                let (fields, tag_type) = write::read_row(&read_p).unwrap_or_default();
                let added = crate::db::added_at(&read_target);
                (fields, tag_type, added)
            })
            .await
            else {
                return;
            };
            if !alive_t.load(Ordering::Relaxed) {
                return;
            }
            put_cached_full(&read, res.clone());
            let (fields, tag_type, added) = res;
            draft.update(|d| {
                for (f, v) in &fields {
                    if d.values.get(f) == d.original.get(f) {
                        d.values.insert(*f, v.clone());
                    }
                }
                d.original = fields.clone();
            });
            full.set(Some((fields, tag_type, added)));
        });
    }

    let file_label = Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| super::super::last_segment(&path).to_owned());
    view()
        .flex_1()
        .min_h(px(0.0))
        .child(single_header(
            &path,
            file_label,
            rename_state,
            draft,
            full,
            rows,
            selected,
            alive.clone(),
            close,
        ))
        .child(
            scroll().key(path.clone()).flex_1().min_h(px(0.0)).child(
                view()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(16.0))
                    .gap(px(12.0))
                    .overflow_hidden()
                    // 状态行是滚动区第一条：写回 / 改名报错（红）与
                    // 「已保存」（淡色），空着不占位。
                    .child(when(
                        move || {
                            draft.with(|d| d.error.is_some() || d.saved)
                                || rename_state.err.with(|e| e.is_some())
                        },
                        move || {
                            text(move || {
                                let d = draft.get();
                                if let Some(e) = &d.error {
                                    e.clone()
                                } else if let Some(e) = rename_state.err.get() {
                                    e
                                } else if d.saved {
                                    "已保存".to_owned()
                                } else {
                                    String::new()
                                }
                            })
                            .number_of_lines(1)
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .text_color(move || {
                                if draft.with(|d| d.error.is_some())
                                    || rename_state.err.with(|e| e.is_some())
                                {
                                    nsh_base::theme().destructive
                                } else {
                                    muted_fg()
                                }
                            })
                        },
                    ))
                    .child(single_form(path.clone(), draft, rows, full)),
            ),
        )
}

/// 单选面板顶栏（与 [`panel_header`] 同款 `px-4 pt-3 pb-2` + 底部分
/// 隔线）：常态「文件名 + 铅笔 + 保存 + ✕」；改名模式整行换成「主干
/// 输入 + `.扩展名` + ✓ + ✕」（`Input` 没有取消事件，Esc 由 ✕ 承担）。
#[allow(clippy::too_many_arguments)]
fn single_header(
    path: &str,
    file_label: String,
    rename_state: RenameState,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    alive: Arc<AtomicBool>,
    close: Rc<dyn Fn()>,
) -> View {
    let stem: Rc<String> = Rc::new(
        Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    let ext = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().into_owned());
    let path = path.to_owned();
    let commit: Rc<dyn Fn()> = Rc::new({
        let path = path.clone();
        let alive = alive.clone();
        move || commit_rename(&path, rename_state, rows, selected, alive.clone())
    });
    view()
        .h(px(36.0))
        .px(px(16.0))
        .border_b_1()
        .border_b_color(border())
        .child(dynamic(move || {
            let parts: Vec<AnyElement> = if rename_state.renaming.get() {
                // 改名模式：主干可编（扩展名独立显示、完成时改名），回车
                // 或 ✓ 提交；输入即时 `plan_name` 预检（分隔符硬规则 /
                // 撞名先报，✓ 随之禁用）。
                let mut parts: Vec<AnyElement> = vec![
                    input_field(true)
                        .flex_1()
                        .min_w(px(0.0))
                        .h(px(24.0))
                        .py(px(0.0))
                        .text_size(px(12.0))
                        .label("文件名")
                        .auto_focus(true)
                        .default_value(rename_state.name_input.get_untracked())
                        .readonly(move || rename_state.busy.get())
                        .on_change({
                            let path = path.clone();
                            move |t: &str| {
                                rename_state.name_input.set(t.to_owned());
                                rename_state
                                    .err
                                    .set(match &rename::plan_name(&path, t).status {
                                        Status::Ok | Status::Unchanged => None,
                                        Status::Invalid(m) | Status::Conflict(m) => Some(m.clone()),
                                    });
                            }
                        })
                        .on_submit({
                            let commit = commit.clone();
                            move |_: &str| commit()
                        })
                        .into_element(),
                ];
                if let Some(e) = &ext {
                    parts.push(
                        text(format!(".{e}"))
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .text_color(muted_fg())
                            .into_element(),
                    );
                }
                parts.push(
                    icon_button(|| Icon::Check, 12.0, "完成重命名", muted_fg, {
                        let commit = commit.clone();
                        move || commit()
                    })
                    .disabled(move || {
                        rename_state.busy.get() || rename_state.err.with(|e| e.is_some())
                    })
                    .into_element(),
                );
                parts.push(
                    icon_button(|| Icon::Close, 12.0, "取消重命名", muted_fg, {
                        let stem = stem.clone();
                        move || {
                            rename_state.err.set(None);
                            rename_state.name_input.set((*stem).clone());
                            rename_state.renaming.set(false);
                        }
                    })
                    .disabled(move || rename_state.busy.get())
                    .into_element(),
                );
                parts
            } else {
                vec![
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            text(file_label.clone())
                                .flex_1()
                                .min_w(px(0.0))
                                .number_of_lines(1)
                                .text_size(px(13.0))
                                .line_height(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(fg()),
                        )
                        .child(
                            icon_button(|| Icon::Edit, 12.0, "重命名", muted_fg, {
                                let stem = stem.clone();
                                move || {
                                    rename_state.name_input.set((*stem).clone());
                                    rename_state.err.set(None);
                                    rename_state.renaming.set(true);
                                }
                            })
                            .disabled(move || {
                                (draft.with(|d| d.values.is_empty()) && full.get().is_none())
                                    || rename_state.busy.get()
                            }),
                        )
                        .into_element(),
                    text_button(Variant::Default, Size::Xs, None, "保存", {
                        let alive = alive.clone();
                        move || save_single(draft, rows, alive.clone())
                    })
                    .accessibility_label("保存")
                    .disabled(move || {
                        (draft.with(|d| d.values.is_empty()) && full.get().is_none())
                            || draft.with(|d| {
                                d.saving || !valid(&d.values) || build_edits(d).is_empty()
                            })
                    })
                    .into_element(),
                    icon_button(|| Icon::Close, 12.0, "关闭", muted_fg, {
                        let close = close.clone();
                        move || close()
                    })
                    .into_element(),
                ]
            };
            view()
                .h_full()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .children(parts)
                .into_element()
        }))
}

/// 单选表单本体（数据就绪后挂载，输入框非受控——`default_value` 只在
/// 挂载时生效，所以全部种子必须先落定）。
fn single_form(
    path: String,
    draft: Signal<SingleDraft>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    full: Signal<Option<FullRow>>,
) -> View {
    view()
        .gap(px(18.0))
        .child(section(
            "基本信息",
            vec![
                field_row("标题", TagField::Title, draft, full),
                field_row("歌手", TagField::Artist, draft, full),
                field_row("专辑", TagField::Album, draft, full),
                field_row("专辑歌手", TagField::AlbumArtist, draft, full),
                genre_row(draft, full),
                narrow_row("年份", TagField::Year, draft, full, 64.0),
                pair_row(
                    "音轨",
                    "音轨号",
                    TagField::Track,
                    "音轨总数",
                    TagField::TrackTotal,
                    draft,
                    full,
                ),
                pair_row(
                    "碟号",
                    "碟号",
                    TagField::Disc,
                    "碟总数",
                    TagField::DiscTotal,
                    draft,
                    full,
                ),
            ],
        ))
        .child(section(
            "人员与版权",
            vec![
                field_row("作曲", TagField::Composer, draft, full),
                field_row("指挥", TagField::Conductor, draft, full),
                field_row("作词", TagField::Lyricist, draft, full),
                field_row("发行商", TagField::Publisher, draft, full),
                field_row("版权", TagField::Copyright, draft, full),
                rating_row(draft, full),
            ],
        ))
        .child(section(
            "封面与歌词",
            vec![
                cover_row(draft, full),
                multi_row("歌词", TagField::Lyrics, draft, full, 140.0, 260.0),
                multi_row("注释", TagField::Comment, draft, full, 64.0, 120.0),
            ],
        ))
        .child(section(
            "ReplayGain",
            vec![
                narrow_row("音轨增益", TagField::TrackGain, draft, full, 88.0),
                narrow_row("音轨峰值", TagField::TrackPeak, draft, full, 88.0),
                narrow_row("专辑增益", TagField::AlbumGain, draft, full, 88.0),
                narrow_row("专辑峰值", TagField::AlbumPeak, draft, full, 88.0),
            ],
        ))
        .child(tech_section(&path, rows, full))
}

/// 节标题：12pt semibold muted。
fn section_title(title: &'static str) -> Text {
    text(title)
        .text_size(px(12.0))
        .line_height(px(12.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(muted_fg())
}

/// 一节：标题 + 间距 10 的若干行（无内部卡片圆角边框）。
fn section(title: &'static str, rows: Vec<View>) -> View {
    view()
        .gap(px(10.0))
        .child(section_title(title))
        .children(rows)
}

/// 行首标签：72pt 列，13pt 淡色，与控件垂直居中。
fn label_cell(label: &'static str) -> Text {
    text(label)
        .w(px(72.0))
        .text_size(px(13.0))
        .line_height(px(13.0))
        .text_color(muted_fg())
}

/// 加载中的输入框占位：与 `input_field` 同款外观，无法获得焦点，文字显示「加载中...」。
fn loading_input(small: bool, width: Option<f32>) -> View {
    let t = nsh_base::theme();
    let pad = match width {
        Some(w) if w < 70.0 => 4.0,
        _ => 12.0,
    };
    let mut v = view()
        .h(px(if small { 32.0 } else { 36.0 }))
        .px(px(pad))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(t.input)
        .bg(t.card)
        .items_start()
        .justify_center()
        .overflow_hidden();
    if let Some(w) = width {
        v = v.w(px(w));
    } else {
        v = v.flex_1().min_w(px(0.0));
    }
    v.child(
        text("加载中...")
            .number_of_lines(1)
            .text_size(px(12.0))
            .line_height(px(12.0))
            .text_color(faint()),
    )
}

/// 一行 `标签 + 输入框`（标签列 72pt，控件占满余宽）。
fn field_row(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell(label))
        .child(dynamic(move || {
            let is_ready =
                untracked(|| draft.with(|d| d.values.contains_key(&field))) || full.get().is_some();
            if !is_ready {
                return loading_input(true, None).into_element();
            }
            let initial = untracked(|| {
                draft
                    .with(|d| d.values.get(&field).cloned())
                    .or_else(|| {
                        full.with(|f| f.as_ref().and_then(|(m, _, _)| m.get(&field).cloned()))
                    })
                    .unwrap_or_default()
            });
            input_field(true)
                .flex_1()
                .min_w(px(0.0))
                .label(label)
                .default_value(initial)
                .readonly(move || draft.with(|d| d.saving))
                .on_change(move |t: &str| {
                    draft.update(|d| {
                        d.values.insert(field, t.to_owned());
                        d.saved = false;
                    })
                })
                .into_element()
        }))
}

/// `;` 分隔串 → 标签列表（草稿里流派仍是 "a; b" 形式，写回格式不变）。
fn split_genres(raw: &str) -> Vec<String> {
    raw.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 流派标签输入行：combobox + 标签 chips，与单个修改（`edit_info`）完全一致。
fn genre_row(draft: Signal<SingleDraft>, full: Signal<Option<FullRow>>) -> View {
    let s = store::store();
    let t = nsh_base::theme();
    let (input_c, ring, popover_c, card_c) = (t.input, t.ring, t.popover, t.card);
    let gate = store::menu_control();
    let query = signal(String::new());
    let genres = move || {
        let raw = draft.with(|d| {
            d.values
                .get(&TagField::Genre)
                .cloned()
                .or_else(|| {
                    full.with(|f| {
                        f.as_ref()
                            .and_then(|(m, _, _)| m.get(&TagField::Genre).cloned())
                    })
                })
                .unwrap_or_default()
        });
        split_genres(&raw)
    };
    let set_genres = move |list: Vec<String>| {
        draft.update(|d| {
            d.values.insert(TagField::Genre, list.join("; "));
            d.saved = false;
        });
    };
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
            }))
    };
    view()
        .flex_row()
        .items_start()
        .gap(px(8.0))
        .child(view().w(px(72.0)).pt(px(8.0)).child(label_cell("标签")))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(4.0))
                .child(dynamic(move || {
                    let is_ready =
                        untracked(|| draft.with(|d| d.values.contains_key(&TagField::Genre)))
                            || full.get().is_some();
                    if !is_ready {
                        return loading_input(true, None).into_element();
                    }
                    nsh_base::combobox()
                        .w(percentage(1.0))
                        .min_h(px(32.0))
                        .flex_wrap()
                        .items_center()
                        .pl(px(6.0))
                        .pr(px(28.0))
                        .py(px(3.0))
                        .gap(px(4.0))
                        .rounded(px(RADIUS_MD))
                        .border_1()
                        .border_color(input_c)
                        .shadow_xs()
                        .bg(card_c)
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
                                if !g.is_empty() && !list.iter().any(|k| k.eq_ignore_ascii_case(&g))
                                {
                                    list.push(g);
                                }
                            }
                            set_genres(list);
                            query.set(String::new());
                        })
                        .placeholder(Some(SharedString::from("回车添加")))
                        .accessibility_label(Some(SharedString::from("标签")))
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
                        .list_style(|st| st.max_h(px(224.0)))
                        .item_class("focus:bg-hover")
                        .item_style(|st| st.px(px(8.0)).py(px(6.0)).rounded(px(6.0)))
                        .render_item(move |entry: &nsh_base::SelectEntry| genre_item(draft, entry))
                        .input_style(move |st| {
                            st.h(px(24.0))
                                .flex_1()
                                .min_w(px(72.0))
                                .text_size(px(13.0))
                                .text_color(fg())
                        })
                        .prefix(when(move || !genres().is_empty(), move || chips()))
                        .trigger_button(
                            view()
                                .absolute()
                                .top(px(4.0))
                                .right(px(6.0))
                                .h(px(24.0))
                                .items_center()
                                .justify_center()
                                .child(trigger_chevron()),
                        )
                        .empty(
                            text("没有可添加的标签")
                                .text_size(px(12.0))
                                .line_height(px(12.0))
                                .text_color(faint()),
                        )
                        .into_element()
                }))
                .child(dynamic(move || {
                    let is_ready =
                        untracked(|| draft.with(|d| d.values.contains_key(&TagField::Genre)))
                            || full.get().is_some();
                    if !is_ready {
                        return view().into_element();
                    }
                    let formatted = genres().join("; ");
                    text(format!("流派：{formatted}"))
                        .text_size(px(11.0))
                        .line_height(px(15.0))
                        .text_color(muted_fg())
                        .into_element()
                })),
        )
}

/// 下拉行：左侧复选框（选中打勾）+ 流派名。「添加“…”」项不在已选集
/// 里，自然显示空框。
fn genre_item(draft: Signal<SingleDraft>, entry: &nsh_base::SelectEntry) -> View {
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
                            .any(|g| g == &value)
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
fn genre_chip(draft: Signal<SingleDraft>, name: String) -> View {
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
                list.retain(|x| x != &name);
                d.values.insert(TagField::Genre, list.join("; "));
                d.saved = false;
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

/// 一行 `标签 + 定宽输入框`（年份 / ReplayGain 类，控件列从同一 x 起）。
fn narrow_row(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
    width: f32,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell(label))
        .child(dynamic(move || {
            let is_ready =
                untracked(|| draft.with(|d| d.values.contains_key(&field))) || full.get().is_some();
            if !is_ready {
                return loading_input(true, Some(width)).into_element();
            }
            number_input(field.label(), field, draft, full, width).into_element()
        }))
}

/// 一行 `标签 + 号 / 总数`：两个等宽输入，`/` 居中，从控件列起点排。
fn pair_row(
    label: &'static str,
    a_label: &'static str,
    a: TagField,
    b_label: &'static str,
    b: TagField,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell(label))
        .child(dynamic(move || {
            let is_ready = untracked(|| {
                draft.with(|d| d.values.contains_key(&a) || d.values.contains_key(&b))
            }) || full.get().is_some();
            if !is_ready {
                return view()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .child(loading_input(true, Some(56.0)))
                    .child(
                        text("/")
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(faint()),
                    )
                    .child(loading_input(true, Some(56.0)))
                    .into_element();
            }
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(number_input(a_label, a, draft, full, 56.0))
                .child(
                    text("/")
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(faint()),
                )
                .child(number_input(b_label, b, draft, full, 56.0))
                .into_element()
        }))
}

/// 窄数字输入（行内并排用）。
fn number_input(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
    width: f32,
) -> impl IntoElement {
    let initial = untracked(|| {
        draft
            .with(|d| d.values.get(&field).cloned())
            .or_else(|| full.with(|f| f.as_ref().and_then(|(m, _, _)| m.get(&field).cloned())))
            .unwrap_or_default()
    });
    input_field(true)
        .w(px(width))
        .label(label)
        .default_value(initial)
        .readonly(move || draft.with(|d| d.saving))
        .on_change(move |t: &str| {
            draft.update(|d| {
                d.values.insert(field, t.to_owned());
                d.saved = false;
            })
        })
}

/// 评分：5 颗星；点当前星清空。加载中显示灰星且不可交互。
fn rating_row(draft: Signal<SingleDraft>, full: Signal<Option<FullRow>>) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell("评分"))
        .child(dynamic(move || {
            let is_ready = untracked(|| draft.with(|d| d.values.contains_key(&TagField::Rating)))
                || full.get().is_some();
            if !is_ready {
                return view()
                    .flex_row()
                    .gap(px(2.0))
                    .children((1..=5).map(|_| {
                        view()
                            .size(px(22.0))
                            .items_center()
                            .justify_center()
                            .child(
                                text("☆")
                                    .text_size(px(16.0))
                                    .line_height(px(16.0))
                                    .text_color(faint()),
                            )
                            .into_element()
                    }))
                    .into_element();
            }
            let star = |n: u32| {
                let filled = move || {
                    draft.with(|d| {
                        d.values
                            .get(&TagField::Rating)
                            .and_then(|v| v.parse::<u32>().ok())
                            .unwrap_or(0)
                            >= n
                    })
                };
                view()
                    .size(px(22.0))
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .role(AccessibilityRole::Button)
                    .label(format!("评分 {n} 星"))
                    .cursor(Cursor::Pointer)
                    .hover(|st| st.bg(hover()))
                    .on_press(move || {
                        draft.update(|d| {
                            let current =
                                d.values.get(&TagField::Rating).cloned().unwrap_or_default();
                            d.values.insert(
                                TagField::Rating,
                                if current == n.to_string() {
                                    String::new()
                                } else {
                                    n.to_string()
                                },
                            );
                            d.saved = false;
                        });
                    })
                    .child(
                        text(move || if filled() { "★" } else { "☆" })
                            .text_size(px(16.0))
                            .line_height(px(16.0))
                            .text_color(move || if filled() { primary() } else { faint() }),
                    )
            };
            view()
                .flex_row()
                .gap(px(2.0))
                .children((1..=5).map(|n| star(n).into_element()))
                .into_element()
        }))
}

/// 多行输入（注释 / 歌词）：限制最大高度（歌词 260pt，注释 120pt）；未就绪时画同款外观占位。
fn multi_row(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
    min_height: f32,
    max_height: f32,
) -> View {
    view()
        .flex_row()
        .items_start()
        .gap(px(8.0))
        .child(view().w(px(72.0)).pt(px(8.0)).child(label_cell(label)))
        .child(dynamic(move || {
            let is_ready =
                untracked(|| draft.with(|d| d.values.contains_key(&field))) || full.get().is_some();
            if !is_ready {
                return view()
                    .flex_1()
                    .min_w(px(0.0))
                    .min_h(px(min_height))
                    .max_h(px(max_height))
                    .px(px(12.0))
                    .py(px(10.0))
                    .rounded(px(RADIUS_MD))
                    .border_1()
                    .border_color(nsh_base::theme().input)
                    .bg(nsh_base::theme().card)
                    .overflow_hidden()
                    .child(
                        text("加载中...")
                            .number_of_lines(1)
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(faint()),
                    )
                    .into_element();
            }
            let initial = untracked(|| {
                draft
                    .with(|d| d.values.get(&field).cloned())
                    .or_else(|| {
                        full.with(|f| f.as_ref().and_then(|(m, _, _)| m.get(&field).cloned()))
                    })
                    .unwrap_or_default()
            });
            input_field(false)
                .flex_1()
                .min_w(px(0.0))
                .h(auto())
                .min_h(px(min_height))
                .max_h(px(max_height))
                .py(px(10.0))
                .multiline(true)
                .overflow_hidden()
                .label(label)
                .default_value(initial)
                .readonly(move || draft.with(|d| d.saving))
                .on_change(move |t: &str| {
                    draft.update(|d| {
                        d.values.insert(field, t.to_owned());
                        d.saved = false;
                    })
                })
                .into_element()
        }))
}

/// 封面行：缩略图 + 更换 / 移除（写内嵌图片）。
fn cover_row(draft: Signal<SingleDraft>, full: Signal<Option<FullRow>>) -> View {
    let src = move || {
        let d = draft.get();
        match &d.cover {
            Some(CoverEdit::Replace { .. }) => d.cover_src.clone().map(SharedString::from),
            Some(CoverEdit::Remove) => None,
            None => store::cover_for_always(&d.path, 80.0),
        }
    };
    let pick = move || {
        let is_ready = untracked(|| draft.with(|d| !d.values.is_empty())) || full.get().is_some();
        if !is_ready {
            return;
        }
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
            let outcome = write::cover_mime(&file)
                .ok_or_else(|| "不支持的图片格式（PNG / JPEG / WebP / BMP / GIF）".to_owned())
                .and_then(|mime| {
                    std::fs::read(&file)
                        .map(|data| (data, mime))
                        .map_err(|e| format!("读取图片失败: {e}"))
                });
            match outcome {
                Ok((data, mime)) => draft.update(|d| {
                    d.cover = Some(CoverEdit::Replace { data, mime });
                    d.cover_src = Some(file.to_string_lossy().into_owned());
                    d.saved = false;
                    d.error = None;
                }),
                Err(error) => draft.update(|d| d.error = Some(error)),
            }
        });
    };
    let busy_or_loading = move || {
        (draft.with(|d| d.values.is_empty()) && full.get().is_none()) || draft.with(|d| d.saving)
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .child(label_cell("封面"))
        .child(cover(src, Some(80.0), 8.0))
        .child(
            view()
                .gap(px(6.0))
                .child(
                    text_button(Variant::Outline, Size::Xs, None, "更换图片…", pick)
                        .disabled(busy_or_loading),
                )
                .child(
                    text_button(
                        Variant::Outline,
                        Size::Xs,
                        None,
                        "移除封面",
                        move || {
                            draft.update(|d| {
                                d.cover = Some(CoverEdit::Remove);
                                d.cover_src = None;
                                d.saved = false;
                            })
                        },
                    )
                    .disabled(busy_or_loading),
                ),
        )
}

/// 「音频参数」节：常开，每行尾部一颗复制钮；「文件夹」行收尾（文件
/// 名在顶栏改，这里不再放）。编码信息读 `MetaCache`（曲库条目都已在
/// 缓存里），标签类型与入库时间随 `full` 落地即准。
fn tech_section(
    path: &str,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    full: Signal<Option<FullRow>>,
) -> View {
    let entry = untracked(|| {
        crate::metadata::MetaCache::global()
            .entry(Path::new(path))
            .ok()
    });
    // 标签类型：`full` 的权威值优先，未落地回落行里的（MetaRow 由同
    // 一条 read_row 得来）。
    let tag_type = {
        let path = path.to_owned();
        move || {
            full.get()
                .and_then(|(_, t, _)| t)
                .or_else(|| rows.with(|r| r.get(&path).and_then(|row| row.tag_type)))
                .unwrap_or("无")
                .to_owned()
        }
    };
    let mut lines = Vec::new();
    if let Some(entry) = &entry {
        let m = &*entry.value;
        lines.push(tech_line("品质", quality_label(m)));
        lines.push(tech_line("编码", m.codec.clone().unwrap_or_default()));
        lines.push(tech_line_dyn("标签类型", tag_type));
        lines.push(tech_line(
            "采样率",
            m.sample_rate
                .map(|r| format!("{:.1} kHz", r as f64 / 1000.0))
                .unwrap_or_default(),
        ));
        lines.push(tech_line(
            "位深",
            m.bit_depth.map(|d| format!("{d} bit")).unwrap_or_default(),
        ));
        lines.push(tech_line(
            "比特率",
            m.bit_rate.map(|b| format!("{b} kbps")).unwrap_or_default(),
        ));
        lines.push(tech_line(
            "声道",
            m.channels.map(|c| c.to_string()).unwrap_or_default(),
        ));
        lines.push(tech_line("时长", store::format_time(m.duration)));
        lines.push(tech_line(
            "大小",
            format!("{:.1} MB", entry.size as f64 / 1_048_576.0),
        ));
        lines.push(tech_line(
            "修改时间",
            if entry.modified == 0 {
                String::new()
            } else {
                crate::config::format_local_datetime(entry.modified)
            },
        ));
    }
    // 入库时间随 `full` 落地（后台同趟读 `db::added_at`）；没记过显示 —。
    lines.push(tech_line_dyn("入库时间", move || {
        full.get()
            .and_then(|(_, _, added)| added)
            .map(crate::config::format_local_datetime)
            .unwrap_or_else(|| "—".to_owned())
    }));
    lines.push(tech_line(
        "文件夹",
        Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    ));
    section("音频参数", lines)
}

/// 技术参数行：72pt 标签 + 值 + 复制钮（`set_clipboard`，标签拼进
/// 无障碍名便于测试断言）。
fn tech_line_dyn(label: &'static str, value: impl Fn() -> String + 'static) -> View {
    let value = Rc::new(value);
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text(label)
                .w(px(72.0))
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg()),
        )
        .child(
            text({
                let value = value.clone();
                move || value()
            })
            .flex_1()
            .min_w(px(0.0))
            .text_size(px(12.0))
            .line_height(px(16.0))
            .text_color(fg()),
        )
        .child(
            shell(Variant::Ghost, Size::IconXs)
                .accessibility_label(SharedString::from(format!("复制{label}")))
                .on_press(move |_: &()| set_clipboard(value()))
                .child(icon(Icon::Copy, 11.0, muted_fg)),
        )
}

/// 静态值版 [`tech_line_dyn`]。
fn tech_line(label: &'static str, value: String) -> View {
    tech_line_dyn(label, move || value.clone())
}

/// 品质徽章：Lossless + 高采样 → Hi-Res；Lossless → SQ；Lossy ≥320 → HQ；
/// 其余回落编码名。
fn quality_label(m: &crate::metadata::MetaInfo) -> String {
    match m.quality.as_deref() {
        Some("Lossless")
            if m.bit_depth.is_some_and(|d| d > 16) || m.sample_rate.is_some_and(|r| r > 48000) =>
        {
            "Hi-Res".to_owned()
        }
        Some("Lossless") => "SQ".to_owned(),
        Some("Lossy") if m.bit_rate.is_some_and(|b| b >= 320) => "HQ".to_owned(),
        _ => m
            .codec
            .clone()
            .or_else(|| m.quality.clone())
            .unwrap_or_default(),
    }
}

/// 草稿 → `TagEdits`（与 edit_info 同规：没动的字段省略，改空的 Clear）。
fn build_edits(d: &SingleDraft) -> TagEdits {
    TagEdits {
        fields: TagField::ALL
            .iter()
            .filter_map(|f| {
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
            .collect(),
        cover: d.cover.clone(),
        cover_file: None,
        lrc: None,
    }
}

/// 本地校验（与 `write::validate` 同规；为空合法 = Clear）。
fn valid(values: &HashMap<TagField, String>) -> bool {
    use crate::metadata::{parse_gain, parse_peak};
    values.iter().all(|(f, raw)| {
        let v = raw.trim();
        if v.is_empty() {
            return true;
        }
        match f {
            TagField::Year => v.parse::<u16>().is_ok(),
            TagField::Track | TagField::TrackTotal | TagField::Disc | TagField::DiscTotal => {
                v.parse::<u32>().is_ok_and(|n| n > 0)
            }
            TagField::Rating => matches!(v.parse::<u8>(), Ok(1..=5)),
            TagField::TrackGain | TagField::AlbumGain => parse_gain(v).is_some(),
            TagField::TrackPeak | TagField::AlbumPeak => parse_peak(v).is_some(),
            _ => true,
        }
    })
}

/// 「保存」：把草稿的 `TagEdits` 写回（后台线程，当前曲目先 release
/// 再 reload 由 `store::save_tags` 管）。成功后行失效，加载器按新指纹
/// 重读（`refresh_paths` 已换索引指纹）。改名与它分家，走顶栏。
fn save_single(
    draft: Signal<SingleDraft>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    alive: Arc<AtomicBool>,
) {
    let d = draft.get_untracked();
    let path = d.path.clone();
    let edits = build_edits(&d);
    if edits.is_empty() {
        return;
    }
    draft.update(|d| {
        d.saving = true;
        d.saved = false;
        d.error = None;
    });
    store::save_tags(path.clone(), edits, move |result| match result {
        Ok(()) => {
            full_cache().lock().unwrap().retain(|(p, _)| p != &path);
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            rows.update(|r| {
                r.remove(&path);
            });
            draft.update(|d| {
                d.saving = false;
                d.saved = true;
                d.original = d.values.clone();
            });
        }
        Err(error) => {
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            draft.update(|d| {
                d.saving = false;
                d.error = Some(error);
            });
        }
    });
}

/// 顶栏改名提交：`plan_name` 预检（Invalid / Conflict 留在 `err`，
/// Unchanged 直接退出改名态），Ok 后台执行（正播放的先停、含同名
/// `.lrc` / 封面侧车）；音频移走后索引 / 歌单引用与行图 / 选中集
/// 换到新路径（`single` memo 随即按新路径重建面板），执行报错留
/// `err` 与原名原位。
fn commit_rename(
    path: &str,
    state: RenameState,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    alive: Arc<AtomicBool>,
) {
    let planned = rename::plan_name(path, &state.name_input.get_untracked());
    match &planned.status {
        Status::Invalid(m) | Status::Conflict(m) => {
            state.err.set(Some(m.clone()));
            return;
        }
        Status::Unchanged => {
            state.err.set(None);
            state.renaming.set(false);
            return;
        }
        Status::Ok => {}
    }
    state.busy.set(true);
    state.err.set(None);
    let src = planned.src.clone();
    let dest = planned.dest.clone();
    let post = poster();
    std::thread::spawn(move || {
        // 正播放的源先停（Windows 文件句柄占用），再改。
        stop_if_playing_under(&src);
        let (moves, error) = rename::apply(&planned);
        post.post(move || {
            if moves.iter().any(|(o, n)| o == &src && n == &dest) {
                // 应用级引用先换：面板没了（换页 / 换选）也不能让索引 /
                // 歌单指向旧路径。
                library_store().move_path(&src, &dest);
                store::move_playlist_paths(&src, &dest);
                if alive.load(Ordering::Relaxed) {
                    apply_move(&src, &dest, rows, selected);
                }
            }
            if let Some(error) = error {
                if alive.load(Ordering::Relaxed) {
                    state.busy.set(false);
                    state.err.set(Some(error));
                }
                return;
            }
            // 成功：选中集换键已把面板重建成新路径（本面板 `alive` 翻
            // false，这里多半跳过）；仍活着（面板没重建的边角）就把改
            // 名态收掉。
            if alive.load(Ordering::Relaxed) {
                state.busy.set(false);
                state.renaming.set(false);
            }
        });
    });
}

/// 单路径改名后的界面侧换键：行图与选中集从 `src` 挪到 `dest`（`single`
/// memo 随之按新路径重建面板）。与重命名弹窗同一套挪法。
fn apply_move(
    src: &str,
    dest: &str,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
) {
    let mut cache = full_cache().lock().unwrap();
    if let Some(pos) = cache.iter().position(|(p, _)| p == src) {
        let mut entry = cache.remove(pos);
        entry.0 = dest.to_owned();
        cache.push(entry);
    }
    rows.update(|r| {
        if let Some(row) = r.remove(src) {
            r.insert(dest.to_owned(), row);
        }
    });
    selected.update(|set| {
        if set.remove(src) {
            set.insert(dest.to_owned());
        }
    });
}

// --- 批量修改 ------------------------------------------------------------------

/// 字段三态。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tri {
    /// 保持原值。
    Keep,
    /// 统一设为输入值。
    Set,
    /// 清空。
    Clear,
}

/// 英文大小写格式化。
#[derive(Debug, Clone, Copy, PartialEq)]
enum CaseOp {
    /// 首字母大写（每个 ASCII 字母词）。
    Title,
    /// 全部小写。
    Lower,
    /// 全部大写。
    Upper,
}

/// 批量封面改动。
#[derive(Debug, Clone, PartialEq)]
enum CoverMode {
    Keep,
    Replace {
        data: Vec<u8>,
        mime: String,
        src: String,
    },
    Remove,
}

/// 三态字段（spec §四：艺术家、专辑、流派、年份、碟号、作曲、发行商等）。
const TRI_FIELDS: &[TagField] = &[
    TagField::Artist,
    TagField::Album,
    TagField::AlbumArtist,
    TagField::Genre,
    TagField::Year,
    TagField::TrackTotal,
    TagField::Disc,
    TagField::DiscTotal,
    TagField::Composer,
    TagField::Conductor,
    TagField::Lyricist,
    TagField::Publisher,
    TagField::Copyright,
    TagField::Comment,
    TagField::Rating,
];

#[derive(Debug, Clone, PartialEq)]
struct BatchDraft {
    modes: HashMap<TagField, Tri>,
    values: HashMap<TagField, String>,
    /// 音轨编号：起始输入（默认 "1"）与「同时写入音轨总数」。
    num_start: String,
    num_total: bool,
    /// 已暂存的编号（「编号」按下后）；起始 + 是否写总数。
    staged_num: Option<(u32, bool)>,
    /// 已暂存的大小写格式化（作用于标题 / 歌手 / 专辑）。
    staged_case: Option<CaseOp>,
    cover: CoverMode,
    applying: bool,
    message: Option<String>,
}

fn batch_panel(
    paths: Rc<Vec<String>>,
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    close: Rc<dyn Fn()>,
    open_rename: Rc<dyn Fn()>,
) -> View {
    let draft = signal(BatchDraft {
        modes: HashMap::new(),
        values: HashMap::new(),
        num_start: "1".to_owned(),
        num_total: false,
        staged_num: None,
        staged_case: None,
        cover: CoverMode::Keep,
        applying: false,
        message: None,
    });
    let m = paths.len();
    let busy = move || draft.with(|d| d.applying);
    let mut tools = view().gap(px(8.0));

    // 工具行：音轨连续编号（按当前显示序）与英文大小写——按下只暂存，
    // 「应用」统一写回。
    let case_sel = signal(CaseOp::Title);
    tools = tools
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(label_cell("音轨编号"))
                .child(
                    input_field(true)
                        .w(px(56.0))
                        .label("编号起始")
                        .value(move || draft.with(|d| d.num_start.clone()))
                        .readonly(busy)
                        .on_change(move |t: &str| draft.update(|d| d.num_start = t.to_owned())),
                )
                .child(checkbox(
                    move || draft.with(|d| d.num_total),
                    || false,
                    "同时写入音轨总数",
                    move |next| draft.update(|d| d.num_total = next),
                ))
                .child(
                    text("写总数")
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(fg()),
                )
                .child(text_button(
                    Variant::Outline,
                    Size::Xs,
                    None,
                    "编号",
                    move || {
                        let start = draft
                            .with(|d| d.num_start.trim().parse::<u32>().ok())
                            .unwrap_or(1);
                        draft.update(|d| {
                            d.staged_num = Some((start, d.num_total));
                            d.message = None;
                        });
                    },
                )),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(label_cell("大小写"))
                .child(select(
                    move || {
                        match case_sel.get() {
                            CaseOp::Title => "首字母大写",
                            CaseOp::Lower => "全部小写",
                            CaseOp::Upper => "全部大写",
                        }
                        .to_owned()
                    },
                    "大小写",
                    vec![
                        SelectOption::new("首字母大写", move || case_sel.set(CaseOp::Title)),
                        SelectOption::new("全部小写", move || case_sel.set(CaseOp::Lower)),
                        SelectOption::new("全部大写", move || case_sel.set(CaseOp::Upper)),
                    ],
                    120.0,
                ))
                .child(text_button(
                    Variant::Outline,
                    Size::Xs,
                    None,
                    "格式化",
                    move || {
                        draft.update(|d| {
                            d.staged_case = Some(case_sel.get());
                            d.message = None;
                        });
                    },
                )),
        )
        // 提示跟控件列对齐（72 标签 + 8 间隔），行内不再挤。
        .child(
            text("作用于标题 / 歌手 / 专辑")
                .pl(px(80.0))
                .text_size(px(11.0))
                .line_height(px(11.0))
                .text_color(faint()),
        )
        .child(when(
            move || draft.with(|d| d.staged_num.is_some() || d.staged_case.is_some()),
            move || {
                let n = m;
                view().gap(px(2.0)).children(
                    [
                        draft.with(|d| d.staged_num).map(|_| {
                            text(format!("将修改音轨号 ×{n}"))
                                .text_size(px(11.0))
                                .line_height(px(11.0))
                                .text_color(muted_fg())
                                .into_element()
                        }),
                        draft.with(|d| d.staged_case).map(|_| {
                            text("将对标题 / 歌手 / 专辑做大小写格式化")
                                .text_size(px(11.0))
                                .line_height(px(11.0))
                                .text_color(muted_fg())
                                .into_element()
                        }),
                    ]
                    .into_iter()
                    .flatten(),
                )
            },
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(label_cell("重命名"))
                .child(text_button(
                    Variant::Outline,
                    Size::Xs,
                    None,
                    "重命名文件…",
                    move || open_rename(),
                )),
        )
        // 提示跟控件列对齐（同上行的大小写提示）。
        .child(
            text("按模板重命名所选文件（文件留在原文件夹）")
                .pl(px(80.0))
                .text_size(px(11.0))
                .line_height(px(11.0))
                .text_color(faint()),
        );
    // 建立所选路径的 TrackEntry 快速查找表（一次 pass），避免三态字段
    // 计算占位符与批量应用时对全表进行 O(M × N) 线性扫描。
    let selected_entries: Rc<HashMap<String, TrackEntry>> = Rc::new({
        let path_set: std::collections::HashSet<&str> = paths.iter().map(|s| s.as_str()).collect();
        displayed.with_untracked(|v| {
            v.iter()
                .filter(|t| path_set.contains(t.path.as_str()))
                .map(|t| (t.path.clone(), t.clone()))
                .collect()
        })
    });
    let mut fields = view().gap(px(8.0));
    for field in TRI_FIELDS {
        fields = fields.child(tri_row(
            *field,
            paths.clone(),
            selected_entries.clone(),
            rows,
            draft,
        ));
    }
    let form = view()
        .px(px(16.0))
        .pt(px(16.0))
        .pb(px(16.0))
        .gap(px(18.0))
        .overflow_hidden()
        // 消息行是滚动区第一条（同单选面板的状态行），空着不占位。
        .child(when(
            move || draft.with(|d| d.message.is_some()),
            move || {
                text(move || draft.with(|d| d.message.clone().unwrap_or_default()))
                    .number_of_lines(1)
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(muted_fg())
            },
        ))
        .child(section("工具", vec![tools]))
        .child(section("字段", vec![fields]))
        .child(section("封面", vec![cover_mode_row(draft)]));
    view()
        .flex_1()
        .min_h(px(0.0))
        .child(panel_header(
            vec![
                text("批量修改")
                    .text_size(px(15.0))
                    .line_height(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(fg())
                    .into_element(),
                text(format!("已选 {m} 首"))
                    .flex_1()
                    .min_w(px(0.0))
                    .number_of_lines(1)
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(muted_fg())
                    .into_element(),
                text_button(
                    Variant::Default,
                    Size::Xs,
                    None,
                    format!("应用到 {m} 首"),
                    {
                        let paths = paths.clone();
                        let selected_entries = selected_entries.clone();
                        move || {
                            apply_batch(
                                paths.clone(),
                                selected_entries.clone(),
                                displayed,
                                rows,
                                draft,
                            )
                        }
                    },
                )
                .disabled(move || draft.with(|d| d.applying))
                .into_element(),
            ],
            close,
        ))
        .child(scroll().key("batch").flex_1().min_h(px(0.0)).child(form))
}

/// 单首曲目的现有索引字段值（`TrackEntry` 兜底）。
fn entry_field_of(t: &TrackEntry, field: TagField) -> String {
    match field {
        TagField::Title => t.title.clone(),
        TagField::Artist => t.artist.clone(),
        TagField::Album => t.album.clone(),
        TagField::AlbumArtist => t.meta.album_artist.clone(),
        TagField::Genre => t.genres.join("; "),
        TagField::Year => t.year.map(|n| n.to_string()).unwrap_or_default(),
        TagField::Track => t.track.map(|n| n.to_string()).unwrap_or_default(),
        TagField::TrackTotal => t.track_total.map(|n| n.to_string()).unwrap_or_default(),
        TagField::Disc => t.disk.map(|n| n.to_string()).unwrap_or_default(),
        TagField::DiscTotal => t.disk_total.map(|n| n.to_string()).unwrap_or_default(),
        TagField::Composer => t.composers.join("; "),
        TagField::Conductor => t.conductor.clone().unwrap_or_default(),
        TagField::Lyricist => t.lyricist.clone().unwrap_or_default(),
        TagField::Publisher => t.publisher.clone().unwrap_or_default(),
        TagField::Copyright => t.copyright.clone().unwrap_or_default(),
        TagField::Comment => t.comment.clone().unwrap_or_default(),
        TagField::Rating => t.rating.map(|r| r.to_string()).unwrap_or_default(),
        TagField::Lyrics => t.lyrics.clone().unwrap_or_default(),
        TagField::TrackGain => t
            .replaygain
            .track_gain
            .map(|g| format!("{g:.2}"))
            .unwrap_or_default(),
        TagField::TrackPeak => t
            .replaygain
            .track_peak
            .map(|p| format!("{p:.6}"))
            .unwrap_or_default(),
        TagField::AlbumGain => t
            .replaygain
            .album_gain
            .map(|g| format!("{g:.2}"))
            .unwrap_or_default(),
        TagField::AlbumPeak => t
            .replaygain
            .album_peak
            .map(|p| format!("{p:.6}"))
            .unwrap_or_default(),
    }
}

/// 三态行：标签 + 模式下拉（保持原值 / 统一设为 / 清空）+ 输入框
/// （仅「统一设为」可写，占位显示共同值或「多个值」）。
fn tri_row(
    field: TagField,
    paths: Rc<Vec<String>>,
    selected_entries: Rc<HashMap<String, TrackEntry>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    draft: Signal<BatchDraft>,
) -> View {
    let label: &'static str = field.label();
    let mode = move || draft.with(|d| d.modes.get(&field).copied().unwrap_or(Tri::Keep));
    // 占位：全部选中行同值显示它，否则「多个值」（行加载完会修正）。
    let placeholder = {
        let map = rows.get_untracked();
        let mut it = paths.iter().map(|p| {
            if let Some(row) = map.get(p) {
                return row.fields.get(&field).cloned().unwrap_or_default();
            }
            if let Some(t) = selected_entries.get(p) {
                return entry_field_of(t, field);
            }
            String::new()
        });
        match it.next() {
            Some(first) if it.all(|v| v == first) && !first.is_empty() => first,
            _ => "多个值".to_owned(),
        }
    };
    let initial = untracked(|| {
        draft
            .with(|d| d.values.get(&field).cloned())
            .unwrap_or_default()
    });
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell(label))
        .child(select(
            move || {
                match mode() {
                    Tri::Keep => "保持原值",
                    Tri::Set => "统一设为",
                    Tri::Clear => "清空",
                }
                .to_owned()
            },
            label,
            vec![
                SelectOption::new("保持原值", move || {
                    draft.update(|d| {
                        d.modes.insert(field, Tri::Keep);
                    })
                }),
                SelectOption::new("统一设为", move || {
                    draft.update(|d| {
                        d.modes.insert(field, Tri::Set);
                    })
                }),
                SelectOption::new("清空", move || {
                    draft.update(|d| {
                        d.modes.insert(field, Tri::Clear);
                    })
                }),
            ],
            112.0,
        ))
        .child(
            input_field(true)
                .flex_1()
                .min_w(px(0.0))
                .label(label)
                .placeholder(placeholder)
                .default_value(initial)
                .readonly(move || mode() != Tri::Set || draft.with(|d| d.applying))
                .on_change(move |t: &str| {
                    draft.update(|d| {
                        d.values.insert(field, t.to_owned());
                    })
                }),
        )
}

/// 批量封面行：保持 / 替换…（文件选择器暂存）/ 移除；已暂存显示
/// 说明与「撤销」。
fn cover_mode_row(draft: Signal<BatchDraft>) -> View {
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
            if let Ok((data, mime)) = write::cover_mime(&file)
                .ok_or_else(|| "不支持的图片格式".to_owned())
                .and_then(|mime| {
                    std::fs::read(&file)
                        .map(|data| (data, mime))
                        .map_err(|e| format!("读取图片失败: {e}"))
                })
            {
                draft.update(|d| {
                    d.cover = CoverMode::Replace {
                        data,
                        mime,
                        src: file.to_string_lossy().into_owned(),
                    };
                });
            }
        });
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell("封面"))
        .child(dynamic(move || match draft.get().cover {
            CoverMode::Keep => view()
                .flex_row()
                .gap(px(8.0))
                .child(text_button(Variant::Outline, Size::Xs, None, "替换…", pick))
                .child(text_button(
                    Variant::Outline,
                    Size::Xs,
                    None,
                    "移除",
                    move || draft.update(|d| d.cover = CoverMode::Remove),
                ))
                .into_element(),
            ref mode => {
                let note = match mode {
                    CoverMode::Replace { src, .. } => {
                        format!("替换为 {}", super::super::last_segment(src))
                    }
                    _ => "移除内嵌封面".to_owned(),
                };
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        text(note)
                            .flex_1()
                            .number_of_lines(1)
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .text_color(fg()),
                    )
                    .child(text_button(
                        Variant::Ghost,
                        Size::Xs,
                        None,
                        "撤销",
                        move || draft.update(|d| d.cover = CoverMode::Keep),
                    ))
                    .into_element()
            }
        }))
}

/// 「应用」：按当前显示序生成每首的 `TagEdits`（暂存的编号 / 大小写
/// 覆盖同名字段），后台顺序写回；成功行失效重读。
fn apply_batch(
    paths: Rc<Vec<String>>,
    selected_entries: Rc<HashMap<String, TrackEntry>>,
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    draft: Signal<BatchDraft>,
) {
    let d = draft.get_untracked();
    // 编号按当前显示序（不是选择集的字典序）。
    let order: Vec<String> = displayed
        .get_untracked()
        .iter()
        .filter(|t| paths.contains(&t.path))
        .map(|t| t.path.clone())
        .collect();
    let map = rows.get_untracked();
    let items = batch_edits(
        &order,
        &d.modes,
        &d.values,
        d.staged_num,
        d.staged_case,
        &d.cover,
        |p, f| {
            if let Some(row) = map.get(p) {
                return row.fields.get(&f).cloned().unwrap_or_default();
            }
            if let Some(t) = selected_entries.get(p) {
                return entry_field_of(t, f);
            }
            String::new()
        },
    );
    let total = items.len();
    if total == 0 {
        draft.update(|d| d.message = Some("没有要应用的修改".to_owned()));
        return;
    }
    draft.update(|d| {
        d.applying = true;
        d.message = None;
    });
    let item_paths: Vec<String> = items.iter().map(|(p, _)| p.clone()).collect();
    store::save_tags_batch(items, move |errors| {
        // 失败的路径仍保留在选中集里；成功行失效，加载器按新指纹重读。
        let ok: Vec<&String> = item_paths
            .iter()
            .filter(|p| !errors.iter().any(|(e, _)| e == *p))
            .collect();
        {
            let mut cache = full_cache().lock().unwrap();
            for p in &ok {
                cache.retain(|(cp, _)| cp != *p);
            }
        }
        rows.update(|r| {
            for p in &ok {
                r.remove(*p);
            }
        });
        draft.update(|d| {
            d.applying = false;
            d.message = Some(if errors.is_empty() {
                format!("已更新 {total} 首")
            } else {
                let shown: Vec<String> = errors
                    .iter()
                    .take(3)
                    .map(|(p, e)| format!("{}：{e}", super::super::last_segment(p)))
                    .collect();
                format!("{} 首失败：{}", errors.len(), shown.join("；"))
            });
        });
    });
}

/// 批量草稿 → 每首一份 `TagEdits`（纯函数）：三态按字段展开（「保持原
/// 值」= `Edit::Keep`），编号与大小写在其后覆盖同名字段；全空的跳过。
fn batch_edits(
    paths: &[String],
    modes: &HashMap<TagField, Tri>,
    values: &HashMap<TagField, String>,
    numbering: Option<(u32, bool)>,
    case: Option<CaseOp>,
    cover: &CoverMode,
    field_of: impl Fn(&str, TagField) -> String,
) -> Vec<(String, TagEdits)> {
    let total = paths.len() as u32;
    paths
        .iter()
        .enumerate()
        .filter_map(|(i, path)| {
            let mut fields: Vec<(TagField, Edit)> = TRI_FIELDS
                .iter()
                .map(|f| {
                    (
                        *f,
                        match modes.get(f).copied().unwrap_or(Tri::Keep) {
                            Tri::Keep => Edit::Keep,
                            Tri::Set => Edit::Set(values.get(f).cloned().unwrap_or_default()),
                            Tri::Clear => Edit::Clear,
                        },
                    )
                })
                .collect();
            let mut put = |f: TagField, e: Edit| {
                if let Some(slot) = fields.iter_mut().find(|(k, _)| *k == f) {
                    slot.1 = e;
                } else {
                    fields.push((f, e));
                }
            };
            // 编号 / 大小写覆盖同名字段（tri-state 里 TrackTotal 也在）。
            if let Some((start, with_total)) = numbering {
                put(TagField::Track, Edit::Set((start + i as u32).to_string()));
                if with_total {
                    put(TagField::TrackTotal, Edit::Set(total.to_string()));
                }
            }
            if let Some(op) = case {
                for f in [TagField::Title, TagField::Artist, TagField::Album] {
                    let old = field_of(path, f);
                    let new = case_convert(&old, op);
                    if new != old {
                        put(f, Edit::Set(new));
                    }
                }
            }
            let cover = match cover {
                CoverMode::Keep => None,
                CoverMode::Replace { data, mime, .. } => Some(CoverEdit::Replace {
                    data: data.clone(),
                    mime: mime.clone(),
                }),
                CoverMode::Remove => Some(CoverEdit::Remove),
            };
            let edits = TagEdits {
                fields,
                cover,
                cover_file: None,
                lrc: None,
            };
            (!edits.is_empty()).then(|| (path.clone(), edits))
        })
        .collect()
}

/// 英文大小写：Title = 每个 ASCII 字母词首字母大写其余小写（数字与
/// 非 ASCII 原样保留）；Lower / Upper 整串转。
fn case_convert(s: &str, op: CaseOp) -> String {
    match op {
        CaseOp::Lower => s.to_lowercase(),
        CaseOp::Upper => s.to_uppercase(),
        CaseOp::Title => {
            let mut out = String::with_capacity(s.len());
            let mut word_start = true;
            for c in s.chars() {
                if c.is_ascii_alphabetic() {
                    out.push(if word_start {
                        c.to_ascii_uppercase()
                    } else {
                        c.to_ascii_lowercase()
                    });
                } else {
                    out.push(c);
                }
                word_start = !c.is_ascii_alphanumeric();
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 编号按传入顺序递增、带不带总数；大小写覆盖同名字段；三态展开；
    /// 全空跳过。
    #[test]
    fn batch_edits_builds_per_file() {
        let paths: Vec<String> = (1..=3).map(|i| format!("/m/{i}.mp3")).collect();
        let mut modes = HashMap::new();
        modes.insert(TagField::Artist, Tri::Set);
        modes.insert(TagField::Album, Tri::Clear);
        let mut values = HashMap::new();
        values.insert(TagField::Artist, "歌手".to_owned());
        // 编号起始 3 带总数 + 首字母大写（作用于现有标题）。
        let field_of = |_: &str, f: TagField| match f {
            TagField::Title => "my song".to_owned(),
            _ => String::new(),
        };
        let items = batch_edits(
            &paths,
            &modes,
            &values,
            Some((3, true)),
            Some(CaseOp::Title),
            &CoverMode::Keep,
            field_of,
        );
        assert_eq!(items.len(), 3);
        let get = |e: &TagEdits, f: TagField| {
            e.fields
                .iter()
                .find(|(k, _)| *k == f)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            get(&items[0].1, TagField::Track),
            Some(Edit::Set("3".to_owned()))
        );
        assert_eq!(
            get(&items[2].1, TagField::Track),
            Some(Edit::Set("5".to_owned()))
        );
        assert_eq!(
            get(&items[0].1, TagField::TrackTotal),
            Some(Edit::Set("3".to_owned()))
        );
        assert_eq!(
            get(&items[0].1, TagField::Artist),
            Some(Edit::Set("歌手".to_owned()))
        );
        assert_eq!(get(&items[0].1, TagField::Album), Some(Edit::Clear));
        // 大小写覆盖 Title（Artist 的 Set 不受影响——Artist 不在大小写范围）。
        assert_eq!(
            get(&items[0].1, TagField::Title),
            Some(Edit::Set("My Song".to_owned()))
        );
        // 全 Keep 的跳过。
        let empty = batch_edits(
            &paths,
            &HashMap::new(),
            &HashMap::new(),
            None,
            None,
            &CoverMode::Keep,
            |_, _| String::new(),
        );
        assert!(empty.is_empty());
    }

    /// 大小写：词首大写（ASCII 字母词），小写 / 大写整串转；非 ASCII
    /// 与数字原样。
    #[test]
    fn case_convert_rules() {
        assert_eq!(case_convert("my song", CaseOp::Title), "My Song");
        assert_eq!(case_convert("MY SONG", CaseOp::Title), "My Song");
        assert_eq!(case_convert("abc123def", CaseOp::Title), "Abc123def");
        assert_eq!(case_convert("中文abc", CaseOp::Title), "中文Abc");
        assert_eq!(case_convert("A/B", CaseOp::Lower), "a/b");
        assert_eq!(case_convert("a b", CaseOp::Upper), "A B");
    }

    #[component]
    fn MultiRowOverflowTest() -> impl IntoElement {
        crate::ui::store::install();
        let p = "mock.flac".to_owned();
        let rows = signal(HashMap::new());
        let full = signal(Some((
            HashMap::from([
                (TagField::Lyrics, "line\n".repeat(100)),
                (TagField::Comment, "comment\n".repeat(50)),
            ]),
            Some("FLAC"),
            None,
        )));
        let draft = signal(SingleDraft {
            path: p.clone(),
            original: HashMap::new(),
            values: HashMap::new(),
            cover: None,
            cover_src: None,
            saving: false,
            saved: false,
            error: None,
        });
        scroll().flex_1().min_h(px(0.0)).child(
            view()
                .overflow_hidden()
                .child(single_form(p, draft, rows, full)),
        )
    }

    #[test]
    fn multiline_overflow_does_not_expand_scroll_content() {
        use nsh_core::geometry::{px, size};
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;
        let theme =
            nsh_base::Theme::from_json(include_str!("../../../../theme.json")).expect("theme.json");
        let mut config =
            EngineConfig::new(Root::of::<MultiRowOverflowTest>().tokens(theme.into_tokens()));
        config.ui_sets = vec![nsh_base::registry(), crate::registry()];
        config.viewport = size(px(400.0), px(800.0));
        let mut app = Headless::start(config).expect("start");
        app.wait_for("基本信息").unwrap();
        // Since lyrics and comment have overflow_hidden, their 100 lines of text
        // are clipped within max_h (260pt and 120pt) and do not expand the scroll content.
        let err = app.scroll("text:基本信息", px(-1.0)).unwrap_err();
        assert!(err.contains("cannot scroll by -1"));
    }

    #[component]
    fn SinglePanelInstantSeedTest() -> impl IntoElement {
        crate::ui::store::install();
        let p = "mock.flac".to_owned();
        let meta = Arc::new(crate::metadata::MetaInfo {
            title: "Mock Title".to_owned(),
            artist: "Mock Artist".to_owned(),
            lyrics: Some("[00:01.00]测试歌词".to_owned()),
            comment: Some("测试注释".to_owned()),
            ..Default::default()
        });
        let entry = TrackEntry(Arc::new(crate::config::TrackShared {
            provider: crate::config::ProviderId::Local,
            path: p.clone(),
            file_name: "mock.flac".to_owned(),
            album_artist: "Mock Artist".to_owned(),
            genres: vec!["二次元".to_owned(), "JPOP".to_owned()],
            size_bytes: 1024,
            modified: 0,
            stamp: 1,
            meta,
        }));
        let displayed = computed(move || Rc::new(vec![entry.clone()]));
        let rows = signal(HashMap::new());
        let selected = signal(BTreeSet::from([p.clone()]));
        single_panel(p, displayed, rows, selected, Rc::new(|| {}))
    }

    #[test]
    fn single_panel_prefills_lyrics_instantly_from_seed() {
        use nsh_core::geometry::{px, size};
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;
        let theme =
            nsh_base::Theme::from_json(include_str!("../../../../theme.json")).expect("theme.json");
        let mut config =
            EngineConfig::new(Root::of::<SinglePanelInstantSeedTest>().tokens(theme.into_tokens()));
        config.ui_sets = vec![nsh_base::registry(), crate::registry()];
        config.viewport = size(px(400.0), px(800.0));
        let mut app = Headless::start(config).expect("start");
        app.wait_for("label:歌词").unwrap();
        assert_eq!(
            app.screen().input_text("label:歌词").unwrap(),
            Some("[00:01.00]测试歌词")
        );
        assert_eq!(
            app.screen().input_text("label:注释").unwrap(),
            Some("测试注释")
        );
        assert_eq!(
            app.screen().input_text("label:标题").unwrap(),
            Some("Mock Title")
        );
        app.wait_for("text:二次元").unwrap();
        app.wait_for("text:JPOP").unwrap();
        app.wait_for("text:标签").unwrap();
        app.wait_for("text:流派：二次元; JPOP").unwrap();
    }
}
