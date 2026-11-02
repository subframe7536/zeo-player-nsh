# Plan 012：「文件夹」视图右侧改为标签表格（多选 + 批量编辑面板）

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- src/ui/views/folders.rs src/ui/components/tracks.rs src/ui/store.rs src/config/mod.rs src/metadata/write.rs tests/ui.rs README.md`
> 前序计划的改动是预期内的；确认计划 011 已完成（存在 `src/metadata/write.rs`，`store::save_tags` 可用）。

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: MED（重做一个核心视图；批量写文件复用 011 的安全写）
- **Depends on**: plans/011-tag-write-and-edit-dialog.md
- **Category**: direction（issue #8）
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

issue #8 原设想是独立的元数据管理窗口（左文件树、中间类 MusicTag 的表格、右侧编辑表单）。NSH 没有多窗口，用户决定**直接把「文件夹」视图改成这个布局**：左侧目录树保留（含拖放、增删改），右侧曲目列表换成可多选、可排序、可选列的紧凑标签表格，选中后右侧出现编辑面板，可一次改多首。

## 约定（已定，不要改）

- 只改「文件夹」视图的右侧；歌手 / 专辑 / 标签 / 歌单 / 搜索页的曲目列表不动（仍用 `components::tracks::track_row`）。
- 表格行高 32pt、无封面列（标签表格追求密度，也省封面解码内存）；虚拟化保持（`virtual_list`）。
- 默认可见列：`#`、标题、歌手、专辑、年份、音轨、流派、时长、喜欢；可选列：专辑歌手、碟号、作曲、格式、大小、文件名、修改时间。可见列存 `library.json`（`folder_columns`）。
- 点列头排序（升 → 降 → 恢复默认顺序），排序不持久化。
- 选择：单击选中一行；`Cmd`（macOS）/ `Ctrl`（Windows）+ 单击切换；`Shift` + 单击从锚点选到此行；双击播放（队列 = 表格当前顺序）。修饰键从 `EventKind::Press` 的 `EventPayload::Press(PressEvent { modifiers, clicks, .. })` 读取（`nsh_core::ops::PressEvent`，`modifiers: keymap::Modifiers { ctrl, alt, shift, cmd }`）；Rust 侧的 `.on_press` 只给 `()`，需要用通用的 `.on(EventKind::Press, |p: &EventPayload| …)`。双击判定沿用 `track_row` 的 450ms 同行二次按压逻辑（headless 测试不一定提供 `clicks`）。
- 编辑面板：选中 ≥1 行时出现在表格右侧（宽 300pt）；内容区宽度不足（表格可用宽 < 420pt）时改为浮在表格上方右侧的卡片（带阴影），不挤压表格。面板可用右上角 × 收起，再次改变选择时重新出现。
- 面板字段与 011 的单曲弹窗相同（标题 … 注释 + 封面）。多选时，各文件值相同的字段显示该值，不同的显示空输入框 + 占位文字 `多个值`；**只把用户改过的字段写入**；每个字段右侧有 × 按钮表示「清空该字段（所有选中文件）」。
- 保存调用 011 的 `store::save_tags(paths, edits, done)`；保存中面板显示 `正在保存 12 / 120…`；失败的文件在面板里列出（最多 5 条 + `等 N 个`）。
- 拖动行到左侧树移动文件的现有功能保留（拖动的是按下的那一行；多选拖动不在本计划）。右键菜单：右键未选中的行时先单选它，菜单仍是现有 `track_menu`（单文件）。

## Current state

- `src/ui/views/folders.rs`（1750 行）：
  - `pub fn Folders()`（981 行）：`tree`/`here`/`rows`/`files` 等 memo；`files = computed(move || here.with(|p| p.files.clone()))` 是当前文件夹直接包含曲目在 `store().tracks` 里的下标列表。
  - 右侧（1183–1284 行）：`page_header(title, subtitle, None, Some(actions))` + `track_columns(FOLDER_COLS)` 作 header；`file_list = virtual_list(count, |at| { … track_row(track, number, FOLDER_COLS, play) .pan_values(..).claim_touches(ctx.claim()).keep_touches(true).on_responder_grant/move/release/terminate(拖动会话) ; super::playlists::track_menu(row, track, None, play, Some(rename)) })`，`.estimated_item_size(px(ROW)).content_padding_bottom(px(BAR_SPACE)).content_padding_x(px(20.0))`，空态 `empty_state(Icon::Folder, "这个文件夹没有曲目", …)`。
  - `play_indices(indices, start)`（294 行）用下标列表播放。
  - 拖动会话 `DragCtx`（349 行起）：`ctx.begin(path, dir, name)`、`move_to(page)`、`finish(page)`、`cancel()`、`claim()`。
  - `FOLDER_COLS`（35 行附近）：`Columns { cover, album, size: true, format: true }` 之类——以实际为准。
- `src/ui/components/tracks.rs`：`track_columns(cols)`（列头样式：11pt、SEMIBOLD、`letter_spacing 0.6`、`faint()`）、`track_row`（双击逻辑、当前曲高亮、喜欢按钮）、`value_cell(width, value)`、`format_cell`、`format_bytes`、`audio_format(path)`；`content_width()` 在文件夹页会减去 `FOLDERS_TREE`（240）。这些是表格单元格样式的范例，**表格应复用 `value_cell` / `format_cell` / `format_bytes` / `audio_format`，不复制实现**。
- `TrackEntry` 经 `Deref` 可直读 `MetaInfo`：`title`、`artist`、`album`、`album_artist`、`genres`（原始标签值，未拆分）、`year`、`track`、`track_total`、`disk`、`disk_total`、`composers`、`duration`；`TrackShared` 另有 `path`、`file_name`、`size_bytes`、`modified`、`genres`（**按分隔符拆好的**——表格显示原始值用 `t.meta.genres`）。注释（Comment）不在 `MetaInfo` 里。
- 011 提供：`crate::metadata::write::{TagField, Edit, CoverEdit, TagEdits, read_fields, write_tags}`、`store::save_tags`、`LibraryStore::refresh_paths`、单曲弹窗 `views/edit_info.rs`（表单布局可复用：把表单部分抽成函数供面板调用是允许的）。
- 设置模式（`folder_columns` 存储）：`LibraryConfig` 字段 + getter/setter + `LibraryState` + `store::set_xxx`（见 `show_covers` 一套）。
- 菜单写法：`nsh_base::menu()` + `store::menu_control()` 的 `gate`（AGENTS：所有菜单必须走 `menu_control()` 互斥）；`views::item(label, icon, on_select)`、`popup!` 宏（`views/mod.rs`）。
- 测试：`tests/ui.rs` 的 `folders_page_shows_tree_and_files`、`folders_page_tree_ops`、`track_row_double_click_like_and_menu`、`narrow_windows_drop_album_and_size_columns`、`double_press(app, sel)` 助手。计划 010 之后启动默认是首页，文件夹相关用例开头有 `app.press("文件夹")`。

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 测试语料 | `sh scripts/make-test-music.sh` | 打印 wrote … |
| 编译 | `nsh check .` | exit 0 |
| 选择 / 排序 / 合并单测 | `cargo test tag_table` | 全部通过 |
| 文件夹 UI 测试 | `cargo test --test ui folders_` | 全部通过 |
| 全部测试 | `nsh test .` | exit 0 |
| Windows 交叉检查 | `PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH" cargo xwin check --target x86_64-pc-windows-msvc` | exit 0 |

## Scope

**In scope**：`src/ui/views/tag_table.rs`（新建：列定义、选择 / 排序纯函数、表头、行）、`src/ui/views/tag_panel.rs`（新建：编辑面板）、`src/ui/views/folders.rs`（只替换右侧）、`src/ui/views/edit_info.rs`（可抽出共用表单）、`src/ui/views/mod.rs`、`src/ui/store.rs`、`src/config/mod.rs`、`tests/ui.rs`、`README.md`。

**Out of scope**：其它视图的曲目列表；多选拖动；键盘全选 / 方向键选择；在表格单元格里直接编辑；左侧树的任何行为；`components/tracks.rs` 的修改（只复用其中的公开函数）。

## Git workflow

不要 commit / push。

## Steps

### Step 1：纯逻辑（先写、先测）

`src/ui/views/tag_table.rs`：

```rust
//! 「文件夹」页右侧的标签表格：列、排序、多选（纯逻辑在前，界面在后）。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Col { Number, Title, Artist, Album, AlbumArtist, Year, Track, Disc, Genre, Composer, Format, Size, FileName, Modified, Duration, Like }

impl Col {
    pub const DEFAULT: &'static [Col] = &[Col::Number, Col::Title, Col::Artist, Col::Album, Col::Year, Col::Track, Col::Genre, Col::Duration, Col::Like];
    pub const ALL: &'static [Col] = &[…全部，显示顺序…];
    pub fn label(self) -> &'static str   // "#", "标题", "歌手", "专辑", "专辑歌手", "年份", "音轨", "碟号", "流派", "作曲", "格式", "大小", "文件名", "修改时间", "时长", ""
    pub fn width(self) -> Option<f32>    // None = 弹性（标题、歌手、专辑、文件名）；其余定宽
    pub fn sortable(self) -> bool        // Number、Like 不可排序
}

/// 单元格文本（排序与显示共用）。
pub fn cell_text(col: Col, t: &TrackEntry) -> String
/// 稳定排序（不排序时调用方不调用，保持文件夹原顺序）。数字列按数值、缺失值排最后，文本列不分大小写。
pub fn sort_rows(rows: &mut [usize], tracks: &[TrackEntry], col: Col, ascending: bool)

/// 选择状态：选中的路径集合（按路径而不是下标，排序 / 刷新后仍有效）+ 锚点行下标。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Selection { pub paths: BTreeSet<String>, pub anchor: Option<usize> }

/// 处理一次单击：`order` 是当前表格顺序的路径，`index` 是点到的行。
pub fn click(sel: &Selection, order: &[String], index: usize, toggle: bool, range: bool) -> Selection

/// 多选的字段合并：全部相同返回 `Some(值)`，否则 `None`（面板显示「多个值」）。
pub fn common_value<'a>(values: impl Iterator<Item = &'a str>) -> Option<String>
```

`toggle` = macOS 上 `modifiers.cmd`、其它平台 `modifiers.ctrl`；`range` = `modifiers.shift`。规则：普通单击 → 只选这一行、锚点 = 此行；toggle → 切换此行、锚点 = 此行；range → 选中锚点到此行之间（含两端）的全部路径（替换原选择；无锚点时等同普通单击），锚点不变。

单测（`mod tests`，名字以 `tag_table_` 开头）：
- `tag_table_click_plain_toggle_range`：覆盖普通 / toggle 加入 / toggle 取消 / shift 向下 / shift 向上 / 无锚点 shift。
- `tag_table_selection_survives_resort`：选中两行后对 `order` 重排，`paths` 不变。
- `tag_table_sort_numeric_and_text`：年份、音轨按数值（缺失排最后，升降序都在最后），标题不分大小写，降序反转，相等元素保持原顺序。
- `tag_table_common_value`：全同 / 有不同 / 空迭代器（返回 `None`）。

**Verify**：`cargo test tag_table` → 4 passed。

### Step 2：列设置持久化

`config` 不能依赖 `ui`，所以存字符串：`LibraryConfig` 加 `#[serde(default = "default_folder_columns")] pub folder_columns: Vec<String>`（值为 `Col` 的 serde 名，如 `"title"`、`"album_artist"`），默认值是 `Col::DEFAULT` 对应的字符串列表（在 `config` 里直接写字面量）；`ui` 侧解析时忽略未知值、去重，并保证 `Title` 永远在。getter/setter + `LibraryState` + `store::set_folder_columns(Vec<String>)` 照 `show_covers` 一套写。

**Verify**：`nsh check .` → exit 0。

### Step 3：表格界面

`tag_table.rs` 继续：
- `pub(super) fn header(cols, sort: Signal<Option<(Col, bool)>>) -> View`：样式照 `track_columns`；可排序列可点（`role(Button)`、无障碍标签 `按<列名>排序`），当前排序列在文字后加 `↑`/`↓`；最右侧一个「列」小按钮（`Icon::More` 或 `Icon::ChevronDown`，无障碍标签 `显示的列`）打开菜单，列出 `Col::ALL`（除 `Title`）带勾选状态（选中前加 `Icon::Check`），点一项切换并保存。
- `pub(super) fn row(track: Memo<Option<TrackEntry>>, number: Memo<String>, cols, selected: impl Fn() -> bool, on_click: impl Fn(bool, bool), on_play: impl Fn()) -> View`：高 32pt，`rounded(6)`；选中背景 `hover()` 加深一档（用 `theme` 里已有的选中色——看侧栏选中项用什么色，照用），当前播放曲标题用 `primary()`（同 `track_row`）；单元格 13pt 单行 `.line_height(px(13.0))`，数字列 `tabular_nums` 右对齐（复用 `value_cell`），格式列复用 `format_cell`，大小 `format_bytes`，时长 `store::format_time`，修改时间 `YYYY-MM-DD`，喜欢列与 `track_row` 相同的心形按钮（复用 `store::is_liked` / `toggle_like`）。
  - 按压：`.on(EventKind::Press, move |p: &EventPayload| { let m = if let EventPayload::Press(e) = p { e.modifiers } else { Default::default() }; … })`；`toggle = if cfg!(target_os = "macos") { m.cmd } else { m.ctrl }`、`range = m.shift`；先 `on_click(toggle, range)`，再走 450ms 同行二次按压 → `on_play()`（不带修饰键时才判双击）。
  - 无障碍：`role(Button)`、`label(标题)`、`accessibility_state.selected = selected()`。

**Verify**：`nsh check .` → exit 0。

### Step 4：编辑面板

`src/ui/views/tag_panel.rs`：
- `pub(super) fn panel(paths: Memo<Vec<String>>, on_close: impl Fn()) -> View`。
- 顶部：`已选 N 首` + × 关闭（无障碍标签 `关闭编辑面板`）。
- 单选时显示封面（`cover_for_always(path, 96.0)`）；多选时显示 `N 首的封面` 文字占位。按钮 `更换封面…` / `移除封面`（作用于全部选中）。
- 字段表单：可从 011 的 `edit_info.rs` 抽出 `fn tag_form(initial: Signal<HashMap<TagField, Option<String>>>, edits: Signal<HashMap<TagField, Edit>>) -> View` 供两处共用（`Option<String>`：`None` = 多个值）。初值：从选中的 `TrackEntry` 的 `MetaInfo` 用 `common_value` 计算（标题、歌手、专辑、专辑歌手、流派 `genres.join("; ")`、年份、音轨、音轨总数、碟号、碟总数、作曲 `composers.join("; ")`）；**注释**不在缓存里：选中 ≤ 200 首时后台 `read_fields` 逐个读取后合并，> 200 首时注释输入框禁用并提示 `选中太多，注释不可批量编辑`。
- 每个字段右侧 × → `Edit::Clear`（输入框显示占位 `将清空`）；输入后 → `Edit::Set(值)`；未动 → 不进 `TagEdits`。
- 底部：`还原`（丢弃改动）/ `保存`（无改动时禁用）。保存：`store::save_tags(paths, edits, done)`；保存期间显示进度 `正在保存 i / N…`（011 的 `save_tags` 若没有进度回调，本计划给它加一个可选的 `progress: Option<Signal<(usize, usize)>>` 参数——经 `poster()` 每写完一个文件 post 一次；改动保持向后兼容，011 的弹窗传 `None`）。完成后保持选择、面板显示新值（来自刷新后的 `store().tracks`）；失败列表显示在面板底部。
- 选择变化时：若有未保存的改动，直接丢弃（不弹确认——简单优先；在面板里用一行小字提示 `切换选择会丢弃未保存的修改`）。

**Verify**：`nsh check .` → exit 0。

### Step 5：替换「文件夹」页右侧

在 `Folders()` 里：
1. 新增本页状态：`sort: Signal<Option<(Col, bool)>>`、`selection: Signal<Selection>`、`panel_open: Signal<bool>`、`cols`（从 `s.library` 的 `folder_columns` 解析）。
2. `order = computed(|| { let mut rows = files.get(); if let Some((c, asc)) = sort.get() { sort_rows(&mut rows, &tracks, c, asc) } rows })`（下标 → 路径用于选择）。切换文件夹（`here` 变化）时清空选择、关闭面板。
3. `file_list` 的 item 改为 `tag_table::row(...)`，**保留**原来挂在行上的拖动 responder 链与 `track_menu` 包裹（右键时若该行未选中 → 先 `selection.set(click(&sel, &order, i, false, false))`）；`estimated_item_size(px(32.0))`；header 换成 `page_header(...)` + `tag_table::header(cols, sort)`。双击播放：`play_indices(&order.get_untracked(), at.index())`。
4. 右侧布局：`view().flex_row()` 包 `[表格列(flex_1, min_w 0), when(选择非空 && panel_open, 面板)]`；表格可用宽（`window().width - sidebar_width() - FOLDERS_TREE - 300`）< 420 时面板改 `absolute().right(px(12)).top(px(12)).bottom(px(BAR_SPACE))` 浮层 + `shadow_lg()`。选择从空变非空时 `panel_open.set(true)`。
5. 删除不再使用的 `FOLDER_COLS`（若 `tracks.rs` 的 `content_width()` 依赖 `Nav::Folders` 判断，保持不动——它仍服务于别的页面在文件夹导航下的情况；若因此出现未使用 warning，按编译器提示最小化处理并在汇报里说明）。

**Verify**：`nsh check .` → exit 0；`nsh run .` 手动：选择 / 多选 / 排序 / 列开关 / 面板保存 / 拖动到树 / 右键菜单都正常。截图存 `.nsh/tmp/folders-table.png`。

### Step 6：UI 测试

`tests/ui.rs`：
1. 跑 `nsh test .`，修复因右侧改版而失败的文件夹用例：只允许把「曲目行」的选择器 / 断言改成表格行的等价物（例如行高、列头文字 `大小` → 表格列名），不允许删除断言；在汇报里逐条列出改动。`narrow_windows_drop_album_and_size_columns` 若测的是 `track_row` 在文件夹页的列收起行为，改为在「专辑」详情页验证同样的收起（该行为仍属于 `track_row`），并在汇报里说明。
2. 新增 `folders_table_selects_and_shows_panel`（需要语料）：`app.press("文件夹")`；按 `测试歌曲` 所在行一次 → 等 `已选 1 首` 与 `label:标题`（值 `测试歌曲`）出现；按 `label:关闭编辑面板` → 面板消失。
3. 新增 `folders_table_sorts_by_title`：按 `label:按标题排序` → 第一行标题按升序；再按一次 → 降序（读 `texts(&app)` 中行标题出现的先后）。
4. 新增 `folders_table_column_toggle_persists`：打开 `label:显示的列` 菜单，勾选 `作曲` → 列头出现 `作曲`；`library.json` 的 `folder_columns` 含 `composer`（读法同计划 002）；再取消勾选恢复。

**Verify**：`cargo test --test ui folders_` → 全部通过；`nsh test .` → exit 0。

### Step 7：README 与 AGENTS

- README「浏览与搜索」第一条里「右侧显示选中文件夹直接包含的曲目」改为：`右侧是标签表格：可点列头排序、选择显示的列；单击选中、⌘ / Ctrl 加选、Shift 连选，双击播放；选中后右侧出现编辑面板，可一次修改多首的标题、歌手、专辑、流派、年份、音轨 / 碟号、作曲、注释与封面（直接写回文件）`，其余（树、拖放、右键增删改）保留。
- AGENTS.md Conventions 加一句：`「文件夹」页右侧是标签表格（views/tag_table.rs + tag_panel.rs），其它页的曲目列表仍用 components::tracks::track_row；表格选择按路径存，排序 / 刷新后不丢。`

## Test plan

- 纯逻辑单测 4 个（选择、排序、合并）；UI 测试 3 个新增 + 既有文件夹用例按规则适配。批量写入的正确性由 011 的写回单测覆盖。

## Done criteria

- [ ] `cargo test tag_table` 4 passed；`cargo test --test ui folders_` 通过；`nsh check .`、`nsh test .` exit 0；交叉检查 exit 0
- [ ] `rg -n "track_row\(" src/ui/views/folders.rs` 无输出（文件夹页已改用表格行）
- [ ] 其它视图仍用 `track_row`：`rg -n "track_row\(" src/ui/views/mod.rs` 有输出
- [ ] README / AGENTS 已更新；`plans/README.md` 状态行更新

## STOP conditions

- 计划 011 未完成。
- `.on(EventKind::Press, …)` 收不到 `EventPayload::Press`（修饰键不可得）：汇报；不要改成用全局按键状态猜修饰键。
- 加了 `.on(EventKind::Press, …)` 后行上的拖动 responder 或右键菜单失效，且两次调整无法兼得。
- 既有 UI 用例的适配需要删除断言。
- 在 720pt 最小窗口宽度下表格完全不可用（#12 待定，不改最小宽度）——汇报截图。

## Maintenance notes

- 新增可编辑字段：011 的 `TagField` + 本计划的 `Col` + 面板表单三处一起改。
- 若以后其它页面也要多选，`Selection` / `click` 是纯函数，可直接复用。
- 审阅重点：选择按路径存；切换文件夹清空选择；只写改过的字段；拖动与右键行为未回退。
