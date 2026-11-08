//! 右侧面板：单选（m==1）全字段编辑 + 顶栏内联改名 + 「音频参数」
//! 节（常开、值可复制）；多选（m≥2）批量修改（字段三态 + 音轨编号 /
//! 大小写 / 模板重命名工具 + 统一封面），写回走 `store::save_tags` /
//! `save_tags_batch`。操作都收在固定顶栏（单选「保存」/ 批量「应用」），
//! 状态行是滚动区第一条；滚动区末位不再放操作行。

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nsh_core::prelude::*;

use super::super::super::components::button::{
    RADIUS_MD, Size, Variant, icon_button, shell, text_button,
};
use super::super::super::components::checkbox::checkbox;
use super::super::super::components::cover::cover;
use super::super::super::components::form::{SelectOption, input_field, select};
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

/// 面板（w 360，左边框）：单选表单 / 批量修改，按选中数切换——两个
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
        .w(px(360.0))
        .flex_shrink_0()
        .border_l_1()
        .border_l_color(border())
        .child(dynamic(move || {
            if let Some(path) = single.get() {
                return single_panel(path, rows, selected, close.clone()).into_element();
            }
            if let Some(paths) = batch.get() {
                return batch_panel(paths, displayed, rows, close.clone(), open_rename.clone())
                    .into_element();
            }
            view().into_element()
        }))
}

/// 固定顶栏（滚动区外）：`px-4 pt-3 pb-2` + 底部分隔线，标题件横排
/// 后接一颗关闭钮（清空选中集，面板随之收掉）。
fn panel_header(parts: Vec<AnyElement>, close: Rc<dyn Fn()>) -> View {
    view()
        .px(px(16.0))
        .pt(px(12.0))
        .pb(px(8.0))
        .border_b_1()
        .border_b_color(border())
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .children(parts)
        .child(icon_button(
            || Icon::Close,
            14.0,
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

/// 行（`MetaRow`）值 → 种子映射：注释 / 歌词在 MetaRow 里是截断的，
/// 不种（完整值由后台读回填）。
fn row_seed(row: &MetaRow) -> HashMap<TagField, String> {
    row.fields
        .iter()
        .filter(|(f, _)| !matches!(f, TagField::Comment | TagField::Lyrics))
        .map(|(f, v)| (*f, v.clone()))
        .collect()
}

fn single_panel(
    path: String,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    close: Rc<dyn Fn()>,
) -> View {
    // 完整字段值后台线程读（lofty 整文件过一遍，注释 / 歌词可能很长；
    // 读不出按全空，仍可填），入库时间随同一趟 `db::added_at` 捎回
    // （SQLite 读别上界面线程）。`post` 回排把权威值并进草稿——面板
    // 拆掉（换选 / 换页）后 `alive` 挡住写入。
    let full = signal(Option::<FullRow>::None);
    let alive = Arc::new(AtomicBool::new(true));
    cleanup({
        let alive = alive.clone();
        move || alive.store(false, Ordering::Relaxed)
    });
    // 「已进过种子」标记：行（MetaRow）先到用它，完整值回填注释 / 歌词
    // 与权威原值；完整值先到直接全量。
    let seeded = signal(false);
    let draft = signal(SingleDraft {
        path: path.clone(),
        original: HashMap::new(),
        values: HashMap::new(),
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
    // 滚动视口高：内容矮于视口时把歌词输入拉到底（见下面 min_h / flex
    // 链），首轮 on_layout 未到时为 0——min_h(0) 是空操作。
    let viewport_h = signal(0.0_f32);
    {
        let post = poster();
        let read = path.clone();
        let alive_t = alive.clone();
        std::thread::spawn(move || {
            let (fields, tag_type) = write::read_row(Path::new(&read)).unwrap_or_default();
            let added = crate::db::added_at(&read);
            post.post(move || {
                if !alive_t.load(Ordering::Relaxed) {
                    return;
                }
                let row_seeded = seeded.get_untracked();
                seeded.set(true);
                draft.update(|d| {
                    if row_seeded {
                        // 行种子只缺注释 / 歌词——补它俩即可（它们的输入
                        // 等 `full` 才挂载，不可能已被编辑）。
                        for f in [TagField::Comment, TagField::Lyrics] {
                            d.values
                                .insert(f, fields.get(&f).cloned().unwrap_or_default());
                        }
                    } else {
                        d.values = fields.clone();
                    }
                    d.original = fields.clone();
                });
                full.set(Some((fields, tag_type, added)));
            });
        });
    }
    // 表单门槛：完整值或行种子谁先到用谁的（行通常先到——表格加载批）。
    let ready = {
        let path = path.clone();
        computed(move || full.get().is_some() || rows.with(|r| r.contains_key(path.as_str())))
    };
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
            // 包一层量滚动视口高（on_layout 报的是自身 bounds，滚动条
            // 进出不影响）：内容据此 min_h 撑到视口高，歌词框的 flex 链
            // 才有空余可分。
            view()
                .flex_1()
                .min_h(px(0.0))
                .on_layout(move |b: &Bounds<Points>| viewport_h.set(b.size.height.0))
                .child(
                    scroll().flex_1().min_h(px(0.0)).child(
                        view()
                            .px(px(16.0))
                            .pt(px(16.0))
                            // 底部避开浮动播放条：滚动内容能滚过它。
                            .pb(px(BAR_SPACE))
                            .gap(px(16.0))
                            // 内容矮于视口时撑满：歌词行吸收剩余高度，
                            // 「音频参数」节随之贴到底边（播放条上方）。
                            .min_h(move || px(viewport_h.get()))
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
                            .child(dynamic(move || {
                                if !ready.get() {
                                    return view()
                                        .h(px(160.0))
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            text("正在读取元数据…")
                                                .text_size(px(12.0))
                                                .line_height(px(12.0))
                                                .text_color(muted_fg()),
                                        )
                                        .into_element();
                                }
                                // 行种子兜底（`untracked`：构建期读不把表单
                                // 订到 rows / full；`full` 先到时 post 已填，
                                // 这里幂等空转）。
                                untracked(|| {
                                    if !seeded.get()
                                        && full.get().is_none()
                                        && let Some(seed) =
                                            rows.get().get(&path).map(|row| row_seed(row))
                                    {
                                        seeded.set(true);
                                        draft.update(|d| {
                                            d.original = seed.clone();
                                            d.values = seed;
                                        });
                                    }
                                });
                                single_form(path.clone(), draft, rows, full).into_element()
                            })),
                    ),
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
        .px(px(16.0))
        .pt(px(12.0))
        .pb(px(8.0))
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
                    icon_button(|| Icon::Check, 14.0, "完成重命名", muted_fg, {
                        let commit = commit.clone();
                        move || commit()
                    })
                    .disabled(move || {
                        rename_state.busy.get() || rename_state.err.with(|e| e.is_some())
                    })
                    .into_element(),
                );
                parts.push(
                    icon_button(|| Icon::Close, 14.0, "取消重命名", muted_fg, {
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
                    text(file_label.clone())
                        .flex_1()
                        .min_w(px(0.0))
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg())
                        .into_element(),
                    icon_button(|| Icon::Edit, 14.0, "重命名", muted_fg, {
                        let stem = stem.clone();
                        move || {
                            rename_state.name_input.set((*stem).clone());
                            rename_state.err.set(None);
                            rename_state.renaming.set(true);
                        }
                    })
                    .into_element(),
                    text_button(Variant::Default, Size::Sm, None, "保存", {
                        let alive = alive.clone();
                        move || save_single(draft, rows, alive.clone())
                    })
                    .accessibility_label("保存")
                    .disabled(move || {
                        full.get().is_none()
                            || draft.with(|d| {
                                d.saving || !valid(&d.values) || build_edits(d).is_empty()
                            })
                    })
                    .into_element(),
                    icon_button(|| Icon::Close, 14.0, "关闭", muted_fg, {
                        let close = close.clone();
                        move || close()
                    })
                    .into_element(),
                ]
            };
            view()
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
        // 滚动内容根撑到视口高后，表单吃满其中的余高（内容超高时
        // flex_1 不收缩——min-height 兜底自然高），「歌词与注释」节的
        // flex_1 才有空余可吸收。
        .flex_1()
        .gap(px(16.0))
        .child(section(
            "基本信息",
            vec![
                field_row("标题", TagField::Title, draft),
                field_row("歌手", TagField::Artist, draft),
                field_row("专辑", TagField::Album, draft),
                field_row("专辑歌手", TagField::AlbumArtist, draft),
                field_row("流派", TagField::Genre, draft),
                narrow_row("年份", TagField::Year, draft, 64.0),
            ],
        ))
        .child(section(
            "编号",
            vec![
                pair_row(
                    "音轨",
                    "音轨号",
                    TagField::Track,
                    "音轨总数",
                    TagField::TrackTotal,
                    draft,
                ),
                pair_row(
                    "碟号",
                    "碟号",
                    TagField::Disc,
                    "碟总数",
                    TagField::DiscTotal,
                    draft,
                ),
            ],
        ))
        .child(section(
            "人员",
            vec![
                field_row("作曲", TagField::Composer, draft),
                field_row("指挥", TagField::Conductor, draft),
                field_row("作词", TagField::Lyricist, draft),
            ],
        ))
        .child(section(
            "其他",
            vec![
                field_row("发行商", TagField::Publisher, draft),
                field_row("版权", TagField::Copyright, draft),
                rating_row(draft),
            ],
        ))
        .child(section(
            "ReplayGain",
            vec![
                narrow_row("音轨增益", TagField::TrackGain, draft, 88.0),
                narrow_row("音轨峰值", TagField::TrackPeak, draft, 88.0),
                narrow_row("专辑增益", TagField::AlbumGain, draft, 88.0),
                narrow_row("专辑峰值", TagField::AlbumPeak, draft, 88.0),
            ],
        ))
        .child(section("封面", vec![cover_row(draft)]))
        .child(
            section(
                "歌词与注释",
                vec![
                    multi_row("注释", TagField::Comment, draft, full, 72.0, false),
                    multi_row("歌词", TagField::Lyrics, draft, full, 160.0, true),
                ],
            )
            // 吃掉表单剩余高度：歌词框跟着拉伸到视口底边（「音频参数」
            // 节自然高、贴底收尾；表单超出一屏时本节自然高，照常滚）。
            .flex_1(),
        )
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

/// 一节：标题 + 行距 8 的若干行；节与节之间由父级 gap(16) 分。
fn section(title: &'static str, rows: Vec<View>) -> View {
    view()
        .gap(px(8.0))
        .child(section_title(title))
        .children(rows)
}

/// 行首标签：64pt 列，13pt 淡色，与控件垂直居中。
fn label_cell(label: &'static str) -> Text {
    text(label)
        .w(px(64.0))
        .text_size(px(13.0))
        .line_height(px(13.0))
        .text_color(muted_fg())
}

/// 一行 `标签 + 输入框`（标签列 64pt，控件占满余宽）。
fn field_row(label: &'static str, field: TagField, draft: Signal<SingleDraft>) -> View {
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
        .child(
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
                }),
        )
}

/// 一行 `标签 + 定宽输入框`（年份 / ReplayGain 类，控件列从同一 x 起）。
fn narrow_row(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    width: f32,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell(label))
        .child(number_input(field.label(), field, draft, width))
}

/// 一行 `标签 + 号 / 总数`：两个等宽输入，`/` 居中，从控件列起点排。
fn pair_row(
    label: &'static str,
    a_label: &'static str,
    a: TagField,
    b_label: &'static str,
    b: TagField,
    draft: Signal<SingleDraft>,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell(label))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(number_input(a_label, a, draft, 56.0))
                .child(
                    text("/")
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(faint()),
                )
                .child(number_input(b_label, b, draft, 56.0)),
        )
}

/// 窄数字输入（行内并排用）。
fn number_input(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    width: f32,
) -> impl IntoElement {
    let initial = untracked(|| {
        draft
            .with(|d| d.values.get(&field).cloned())
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

/// 评分：5 颗星；点当前星清空。
fn rating_row(draft: Signal<SingleDraft>) -> View {
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
            .hover(move |st| st.bg(hover()))
            .on_press(move || {
                draft.update(|d| {
                    let current = d.values.get(&TagField::Rating).cloned().unwrap_or_default();
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
        .items_center()
        .gap(px(8.0))
        .child(label_cell("评分"))
        .child(
            view()
                .flex_row()
                .gap(px(2.0))
                .children((1..=5).map(|n| star(n).into_element())),
        )
}

/// 多行输入（注释 / 歌词）：只在 `full` 落地后挂真输入——值必须是完整
/// 的（MetaRow 里注释截 120 字、歌词只剩预览），挂载也晚一拍，顺手避
/// 开滚动区里多行控件首轮布局缺内容的渲染问题。等待期画同款外观占位。
/// `fill`（歌词行）：行在节列里 `flex_1` 吃剩余高度，输入只保 `min_h`
/// 随行拉高（交叉轴 stretch）；普通行定高、标签顶对齐（`items_start`）。
fn multi_row(
    label: &'static str,
    field: TagField,
    draft: Signal<SingleDraft>,
    full: Signal<Option<FullRow>>,
    height: f32,
    fill: bool,
) -> View {
    let row = view().flex_row().gap(px(8.0));
    // 填满变体：标签包装仍由 pt(8) 顶在上方，行的交叉轴拉伸（默认
    // stretch，写明显式）只作用于输入 / 占位的高度方向。
    let row = if fill {
        row.flex_1().items_stretch()
    } else {
        row.items_start()
    };
    row.child(view().w(px(64.0)).pt(px(8.0)).child(label_cell(label)))
        .child(dynamic(move || {
            if full.get().is_none() {
                let slot = view()
                    .flex_1()
                    .min_w(px(0.0))
                    .px(px(12.0))
                    .py(px(10.0))
                    .rounded(px(RADIUS_MD))
                    .border_1()
                    .border_color(nsh_base::theme().input)
                    .bg(nsh_base::theme().card);
                let slot = if fill {
                    slot.min_h(px(height))
                } else {
                    slot.h(px(height))
                };
                return slot
                    .child(
                        text("加载中…")
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(faint()),
                    )
                    .into_element();
            }
            let initial = untracked(|| {
                draft
                    .with(|d| d.values.get(&field).cloned())
                    .unwrap_or_default()
            });
            let input = input_field(false)
                .flex_1()
                .min_w(px(0.0))
                .py(px(10.0))
                .multiline(true)
                .label(label)
                .default_value(initial)
                .readonly(move || draft.with(|d| d.saving))
                .on_change(move |t: &str| {
                    draft.update(|d| {
                        d.values.insert(field, t.to_owned());
                        d.saved = false;
                    })
                });
            // 填满变体要先把 input_field 烤死的 .h(36) 解回 auto——交叉
            // 轴 stretch 只对 auto 高度生效，定高件在放长的行里不跟着拉。
            let input = if fill {
                input.h(auto()).min_h(px(height))
            } else {
                input.h(px(height))
            };
            input.into_element()
        }))
}

/// 封面行：缩略图 + 更换 / 移除（写内嵌图片）。
fn cover_row(draft: Signal<SingleDraft>) -> View {
    let src = move || {
        let d = draft.get();
        match &d.cover {
            Some(CoverEdit::Replace { .. }) => d.cover_src.clone().map(SharedString::from),
            Some(CoverEdit::Remove) => None,
            None => store::cover_for_always(&d.path, 96.0),
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
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(label_cell("封面"))
        .child(cover(src, Some(48.0), 8.0))
        .child(
            view()
                .gap(px(6.0))
                .child(text_button(
                    Variant::Outline,
                    Size::Sm,
                    None,
                    "更换图片…",
                    pick,
                ))
                .child(
                    text_button(
                        Variant::Outline,
                        Size::Sm,
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
                    .disabled(move || draft.with(|d| d.saving)),
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

/// 技术参数行：64pt 标签 + 值 + 复制钮（`set_clipboard`，标签拼进
/// 无障碍名便于测试断言）。
fn tech_line_dyn(label: &'static str, value: impl Fn() -> String + 'static) -> View {
    let value = Rc::new(value);
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text(label)
                .w(px(64.0))
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
                    Size::Sm,
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
                    Size::Sm,
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
        // 提示跟控件列对齐（64 标签 + 8 间隔），行内不再挤。
        .child(
            text("作用于标题 / 歌手 / 专辑")
                .pl(px(72.0))
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
                    Size::Sm,
                    None,
                    "重命名文件…",
                    move || open_rename(),
                )),
        )
        // 提示跟控件列对齐（同上行的大小写提示）。
        .child(
            text("按模板重命名所选文件（文件留在原文件夹）")
                .pl(px(72.0))
                .text_size(px(11.0))
                .line_height(px(11.0))
                .text_color(faint()),
        );
    let mut fields = view().gap(px(8.0));
    for field in TRI_FIELDS {
        fields = fields.child(tri_row(*field, paths.clone(), displayed, rows, draft));
    }
    let form = view()
        .px(px(16.0))
        .pt(px(16.0))
        .pb(px(BAR_SPACE))
        .gap(px(16.0))
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
                    Size::Sm,
                    None,
                    format!("应用到 {m} 首"),
                    {
                        let paths = paths.clone();
                        move || apply_batch(paths.clone(), displayed, rows, draft)
                    },
                )
                .disabled(move || draft.with(|d| d.applying))
                .into_element(),
            ],
            close,
        ))
        .child(scroll().flex_1().min_h(px(0.0)).child(form))
}

/// 一行的当前字段值：行已加载用标签值，未加载回落 `TrackEntry`。
fn field_of(
    path: &str,
    field: TagField,
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: &HashMap<String, Arc<MetaRow>>,
) -> String {
    if let Some(row) = rows.get(path) {
        return row.fields.get(&field).cloned().unwrap_or_default();
    }
    let Some(t) = displayed.with_untracked(|v| v.iter().find(|t| t.path == path).cloned()) else {
        return String::new();
    };
    match field {
        TagField::Title => t.title.clone(),
        TagField::Artist => t.artist.clone(),
        TagField::Album => t.album.clone(),
        TagField::AlbumArtist => t.album_artist.clone(),
        TagField::Genre => t.genres.join("; "),
        TagField::Year => t.year.map(|n| n.to_string()).unwrap_or_default(),
        TagField::Track => t.track.map(|n| n.to_string()).unwrap_or_default(),
        TagField::TrackTotal => t.track_total.map(|n| n.to_string()).unwrap_or_default(),
        TagField::Disc => t.disk.map(|n| n.to_string()).unwrap_or_default(),
        TagField::DiscTotal => t.disk_total.map(|n| n.to_string()).unwrap_or_default(),
        TagField::Composer => t.composers.join("; "),
        _ => String::new(),
    }
}

/// 三态行：标签 + 模式下拉（保持原值 / 统一设为 / 清空）+ 输入框
/// （仅「统一设为」可写，占位显示共同值或「多个值」）。
fn tri_row(
    field: TagField,
    paths: Rc<Vec<String>>,
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    draft: Signal<BatchDraft>,
) -> View {
    let label: &'static str = field.label();
    let mode = move || draft.with(|d| d.modes.get(&field).copied().unwrap_or(Tri::Keep));
    // 占位：全部选中行同值显示它，否则「多个值」（行加载完会修正）。
    let placeholder = {
        let map = rows.get_untracked();
        let mut it = paths.iter().map(|p| field_of(p, field, displayed, &map));
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
                .child(text_button(Variant::Outline, Size::Sm, None, "替换…", pick))
                .child(text_button(
                    Variant::Outline,
                    Size::Sm,
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
                        Size::Sm,
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
        |p, f| field_of(p, f, displayed, &map),
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
}
