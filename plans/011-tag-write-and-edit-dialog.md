# Plan 011：标签写回核心 + 单曲「编辑信息」弹窗

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- src/metadata src/config/mod.rs src/provider src/engine src/ui/store.rs src/ui/views/playlists.rs src/ui/now_playing/column.rs src/ui/mod.rs scripts/make-test-music.sh README.md`
> 前序计划（001–010）的改动是预期内的；确认计划 003 已完成（`scripts/make-test-music.sh` 会生成 `.nsh/tmp/zeo-player-test-formats`）。

## Status

- **Priority**: P2
- **Effort**: M–L
- **Risk**: HIGH（写用户的音频文件）
- **Depends on**: plans/003-more-audio-formats.md
- **Category**: direction（issue #8）
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

issue #8 要求元数据管理：能改标签。用户决定不做独立窗口，分两步：本计划做**写回核心**（安全写文件、写后刷新曲库与正在播放的曲目）和**单曲编辑弹窗**（全屏页「⋯」与曲目右键菜单进入）；计划 012 在此基础上把「文件夹」视图改成多选表格 + 批量编辑面板。

## 约定（已定，不要改）

- **原子写**：先把原文件复制到同目录的临时文件 `.<文件名>.zeo-tmp`，在临时文件上用 lofty 写标签，成功后 `rename` 覆盖原文件；任何一步失败都删掉临时文件、原文件不动。
- 可编辑字段（`TagField`）：标题、歌手、专辑、专辑歌手、流派、年份、音轨号、音轨总数、碟号、碟总数、作曲、注释；外加封面（保留 / 替换 / 移除）。内嵌歌词不在本计划。
- 每个字段的编辑语义：`Keep`（不改）/ `Set(String)`（设为该值；空串等价于 `Clear`）/ `Clear`（删除该键）。批量编辑（012）也用同一结构。
- 写到**主标签**（`primary_tag_type()`）；文件没有主标签时新建一个。
- 写完后：曲库索引与 `MetaCache` / SQLite 按单路径刷新（不整库重扫）；若改的是正在播放的曲目，引擎重新装载它并停在原位置（保持播放 / 暂停状态），界面与系统媒体会话显示新标题。
- Windows 上正在播放的文件被解码器打开着，`rename` 覆盖会失败：引擎先「释放」当前曲目（关闭文件），写完再重新装载。macOS 无此限制，但为了统一与刷新标题，两端都走「释放 → 写 → 重新装载」。

## Current state

- 读标签 `src/metadata/mod.rs`：`read_meta(path) -> Result<MetaInfo, String>`（109 行）用 `lofty::read_from_path`、`tagged.primary_tag().or_else(|| tagged.first_tag())`，字段映射：`TrackTitle`、`TrackArtist`/`TrackArtists`、`AlbumTitle`、`AlbumArtist`/`AlbumArtists`、`Genre`（多值）、`Year`/`RecordingDate`、`TrackNumber`（`pair` 解析 `n/total`）、`DiscNumber`、`Composer`；`has_art = !tag.pictures().is_empty()`。`MetaInfo` 字段见 21 行。
- lofty 0.25 写 API：`Tag::insert_text(ItemKey, String)`、`Tag::remove_key(ItemKey)`、`Tag::push_picture(Picture)`、`Tag::remove_picture_type(PictureType)`；`TaggedFile::primary_tag_mut()`、`insert_tag(Tag)`、`primary_tag_type()`；保存 `TaggedFileExt::save_to_path(path, WriteOptions::default())`（trait 在 `lofty::file` / `lofty::prelude`）。音轨 / 碟号可用 `Accessor` 的 `set_track` / `set_track_total` / `set_disk` / `set_disk_total` / `remove_*`。
- 缓存与索引：
  - `MetaCache::entry(path)`（`metadata/mod.rs:423`）：文件指纹变了就重读并写库——写完标签后指纹必变，调用它即刷新缓存与 SQLite。
  - `provider/local/mod.rs:126` 私有 `fn read_entry(path, separators) -> Option<TrackEntry>`（经 `MetaCache` 读并按流派分隔符拆分）。
  - `LibraryStore`（`config/mod.rs`）用 `RwLock<Arc<Vec<TrackEntry>>>` 存索引；`move_path`（634 行）是「按路径替换索引条目」的现成范例。UI 每 120ms 轮询 `library_store().tracks()` 刷新 `store().tracks`。
- 引擎 `src/engine/audio.rs`：`EngineCmd`、`handle_cmd`、`load_track(shared, media, loaded, path, queue, index, …)`；`Cue { path, queue, time }` 装载但不播放并 seek。004 之后设备格式从 `out` 取。`Player`（`engine/mod.rs`）的方法发命令。
- 正在播放的文件：`src/ui/views/folders.rs:618` `stop_if_playing_under` 只 `player().stop()`（不关文件）；`Ops::rename` 停掉后重试 `rename` 10 次 × 150ms。
- 菜单：
  - 曲目右键 `src/ui/views/playlists.rs:99` `pub fn track_menu(row, track, from, play, rename: Option<Rc<dyn Fn(&str)>>)`，菜单项用 `item(label, Some(Icon), on_select)`；危险项（删除）在最后一组。
  - 全屏页「⋯」`src/ui/now_playing/column.rs:294` `fn more_menu()`：添加到歌单 / 在访达中显示 / 拷贝文件路径。
- 弹窗：`src/ui/components/form.rs` 的 `dialog_content(width)`、`dialog_header(title, desc)`、`dialog_footer()`、`modal(content, on_close)`、`input_field(small)`；按钮 `components::button::{text_button, Variant, Size}`。根组件按叠放顺序挂弹窗（`src/ui/mod.rs` 的 `root` 里 `.child(when(move || s.editor.with(Option::is_some), views::playlists::editor_dialog))` 是范例：状态放 `Store` 的一个 `Signal<Option<…>>`，非空时挂载）。
- 文件选择器：`rfd::FileDialog`（`src/ui/store.rs:1227` 的 `pick_folders` 在 `blocking` 里调用）。
- 封面缓存：`covers` 的缓存键含文件指纹，写完标签指纹变化后 `store.rs` 里「文件内容变了 → 重新生成封面」的 effect（约 453 行）会自动刷新列表封面。
- 测试语料：计划 003 的 `.nsh/tmp/zeo-player-test-formats`（mp3 之外的 aac.m4a、alac.m4a、raw.aac、vorbis.ogg、pcm.aiff，005 可能加了 rg.flac）与 `.nsh/tmp/zeo-player-test-music`（mp3 / flac / wav）。**测试必须复制到临时目录再写，绝不写语料原件。**

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 测试语料 | `sh scripts/make-test-music.sh` | 打印 wrote … |
| 编译 | `nsh check .` | exit 0 |
| 写回单测 | `cargo test tag_write -- --nocapture` | 全部通过且未跳过 |
| UI 测试 | `cargo test --test ui edit_info` | 通过 |
| 全部测试 | `nsh test .` | exit 0 |
| Windows 交叉检查 | `PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH" cargo xwin check --target x86_64-pc-windows-msvc` | exit 0 |

## Scope

**In scope**：`src/metadata/write.rs`（新建）、`src/metadata/mod.rs`（`pub mod write;`、必要的可见性）、`src/provider/mod.rs` / `src/provider/local/mod.rs`（暴露单文件读取）、`src/config/mod.rs`（`refresh_paths`）、`src/engine/audio.rs`、`src/engine/mod.rs`、`src/ui/store.rs`、`src/ui/views/edit_info.rs`（新建）、`src/ui/views/mod.rs`、`src/ui/views/playlists.rs`（菜单项）、`src/ui/now_playing/column.rs`（菜单项）、`src/ui/mod.rs`（挂弹窗）、`tests/ui.rs`、`README.md`。

**Out of scope**：批量编辑与文件夹表格（计划 012）；内嵌歌词编辑；按文件名推标签 / 联网补全；写回 `.lrc`；ReplayGain 标签编辑；改文件名。

## Git workflow

不要 commit / push。

## Steps

### Step 1：写回模块 `src/metadata/write.rs`

```rust
//! 标签写回：复制到同目录临时文件 → lofty 写标签 → rename 覆盖原文件。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagField { Title, Artist, Album, AlbumArtist, Genre, Year, Track, TrackTotal, Disc, DiscTotal, Composer, Comment }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit { Keep, Set(String), Clear }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverEdit { Keep, Replace { data: Vec<u8>, mime: String }, Remove }

#[derive(Debug, Clone, Default)]
pub struct TagEdits { pub fields: Vec<(TagField, Edit)>, pub cover: Option<CoverEdit> } // cover None = Keep

/// 读出可编辑字段的当前值（弹窗 / 表格初值）。缺失为空串。
pub fn read_fields(path: &Path) -> Result<std::collections::HashMap<TagField, String>, String>

/// 写回。成功返回 Ok；失败时原文件不变。
pub fn write_tags(path: &Path, edits: &TagEdits) -> Result<(), String>
```

实现要点：
- 字段 → `ItemKey`：Title→`TrackTitle`，Artist→`TrackArtist`，Album→`AlbumTitle`，AlbumArtist→`AlbumArtist`，Genre→`Genre`，Year→`Year`（写 `Year` 时同时 `remove_key(RecordingDate)` 以免读回旧值；读取顺序与 `read_meta` 一致），Composer→`Composer`，Comment→`Comment`；Track/TrackTotal/Disc/DiscTotal 用 `Accessor` 的 setter（值必须是正整数，否则返回 `Err("音轨号必须是正整数")` 这类中文错误，**在复制文件之前**校验）。
- `Set` 时先 `remove_key` 再 `insert_text`（清掉多值残留，如多个 Artist 帧）。`Set("")` 视为 `Clear`。
- 封面 `Replace`：`remove_picture_type(PictureType::CoverFront)` 后 `push_picture(Picture::new_unchecked(PictureType::CoverFront, Some(MimeType::from_str(mime)), None, data))`（按 lofty 0.25 实际构造函数调整）；`Remove`：移除全部图片。
- 原子写：`tmp = dir.join(format!(".{}.zeo-tmp", file_name))`；`std::fs::copy(path, &tmp)`；`lofty::read_from_path(&tmp)` → 改 → `save_to_path(&tmp, WriteOptions::default())`；`std::fs::rename(&tmp, path)`（Windows 上 Rust 的 `rename` 会替换已存在的目标）。错误路径都 `let _ = std::fs::remove_file(&tmp);`。
- 写一行注释说明为什么不用 lofty 原地写（中途失败会留下半写的文件）。

**Verify**：`nsh check .` → exit 0。

### Step 2：写回单测（在接任何 UI 之前完成）

`write.rs` 的 `#[cfg(test)] mod tests`，用例名都以 `tag_write_` 开头；两个语料目录都不存在时打印「跳过」返回。每个用例把语料文件 `std::fs::copy` 到 `std::env::temp_dir().join(format!("zeo-tag-write-{}-{}", std::process::id(), 用例名))` 再操作：

- `tag_write_roundtrip_all_containers`：对 `sine.flac`、`quiet.mp3`、`sine.wav`（music 语料）与 `aac.m4a`、`alac.m4a`、`vorbis.ogg`、`pcm.aiff`（formats 语料）逐个：写 Title=`新标题`、Artist=`歌手甲`、Album=`专辑乙`、Genre=`Rock`、Year=`2024`、Track=`3`、TrackTotal=`12`、Disc=`1` → `crate::metadata::read_meta` 读回相同；且 `Source::open` 仍能解码（前 1 秒帧数 > 0）——写标签不能破坏音频。
- `tag_write_clear_removes_key`：Set Album 后 Clear → `read_meta(...).album == "Unknown Album"`（`read_meta` 的缺省值）。
- `tag_write_rejects_bad_numbers_without_touching_file`：Track=`abc` → `Err`，文件字节与写前完全相同、目录里没有 `.zeo-tmp` 残留。
- `tag_write_cover_replace_and_remove`：用一张 8×8 PNG（`image` crate 现场生成）Replace → `read_meta().has_art == true` 且 `read_artwork` 字节等于写入的；Remove → `has_art == false`。
- `tag_write_leaves_no_temp_on_failure`：对只读文件（`set_permissions` 只读）写 → `Err`，原文件不变、无 `.zeo-tmp`（Windows 上只读文件的 rename 行为不同——若该平台断言不成立，用 `#[cfg(unix)]` 限定此用例并注释原因）。

`raw.aac`（ADTS 无标签容器）不在往返列表里；另加 `tag_write_reports_unsupported_container`：对 `raw.aac` 写 → 返回 `Err`（或 lofty 若能写则读回一致——以实际行为为准，在测试注释里写明）。

**Verify**：`cargo test tag_write -- --nocapture` → 全部通过，输出无「跳过」。

### Step 3：单路径刷新

1. `provider/local/mod.rs` 的 `read_entry` 改为 `pub(crate)`；`provider/mod.rs` 门面加 `pub(crate) fn read_entry(path: &Path, separators: &[String]) -> Option<TrackEntry>`（转调 local）。
2. `LibraryStore` 加：
   ```rust
   /// 这些文件的内容变了（例如写了标签）：按新内容重读并替换索引条目，
   /// 标签缓存与数据库经 `MetaCache` 一并更新。不整库重扫。
   pub fn refresh_paths(&self, paths: &[String])
   ```
   对每个路径 `crate::provider::read_entry(Path::new(p), &self.separators())`，在索引里按 `path` 替换（不存在则忽略），写回 `self.tracks`。

**Verify**：`nsh check .` → exit 0。

### Step 4：引擎「释放 / 重新装载」当前曲目

`audio.rs`：
1. `EngineCmd` 加 `Release`：若有 `loaded`，记下 `(path, queue, index, time, playing)` 到引擎线程局部的 `released: Option<Released>`，`fade_out` 后 `*loaded = None`（关闭文件）、`shared.playing = false`；`TrackPublic` 保持不变（界面不闪）。
2. `EngineCmd` 加 `Reload`：若 `released` 有值，用 `load_track` 重新装载同一路径（同队列、同下标），`seek` 到记下的时间（同 `Cue` 的写法），恢复原 `playing`；若没有 `released` 但当前有 `loaded`，同样按「当前路径 + 当前时间 + 当前播放状态」重新装载（用于 macOS 只刷新标题的场景，统一处理）。
3. `Player` 加 `release_current(&self)` 与 `reload_current(&self)`。

**Verify**：`nsh check .` → exit 0；`nsh test .` → exit 0。

### Step 5：store 侧的保存流程

`src/ui/store.rs`：
```rust
/// 把 `edits` 写进 `paths` 的每个文件（后台线程）；完成后回界面刷新曲库，
/// `done(失败的 (路径, 错误) 列表)` 在界面线程回调。正在播放的曲目先
/// 释放、写完重新装载。
pub fn save_tags(paths: Vec<String>, edits: crate::metadata::write::TagEdits, done: impl FnOnce(Vec<(String, String)>) + 'static)
```
- 界面侧拿 `poster()`，`std::thread::spawn`：若 `player().state().path` 在 `paths` 里 → `player().release_current()`，再等 ≤ 300ms（轮询 `state().is_ready == false` 或固定 150ms，参照 `Ops::rename` 的重试风格）。
- 逐个 `write_tags`；Windows 上 `rename` 失败时最多重试 10 次 × 150ms（文件可能尚未释放）。
- `library_store().refresh_paths(&ok_paths)`；若释放过 → `player().reload_current()`。
- `post` 回界面执行 `done(failures)`。`done` 不是 `Send`：做法是在界面侧把 `done` 存进一个 `thread_local` 的 `HashMap<u64, Box<dyn FnOnce(..)>>`，线程里只带 id，`post` 回来时取出调用（或参照 `store.rs` 里现有 `poster()` 用法找更简单的等价写法）。

**Verify**：`nsh check .` → exit 0。

### Step 6：单曲编辑弹窗 `src/ui/views/edit_info.rs`

1. `Store` 加 `pub edit_info: Signal<Option<String>>`（要编辑的路径）；`store.rs` 加 `pub fn open_edit_info(path: &str)`。
2. `src/ui/mod.rs` 的 `root` 里，在 `editor_dialog` 之后挂 `.child(when(move || s.edit_info.with(Option::is_some) && !s.hidden.get(), views::edit_info_dialog))`；`views/mod.rs` 加 `mod edit_info;` 与 `pub(super) use self::edit_info::dialog as edit_info_dialog;`。
3. 弹窗内容（`dialog_content(560.0)`）：
   - 标题 `编辑信息`，描述显示文件名。
   - 左侧封面 120pt（`cover_for_always(path, 120.0)`；`rg -n "fn cover_for_always" src/ui/store.rs` 确认名字）+ 下方两个小按钮 `更换封面…`（`rfd::FileDialog` 选 jpg/jpeg/png/webp，在 `blocking` 里读字节，按扩展名定 mime）与 `移除封面`。
   - 右侧表单（两列网格，标签 + `input_field(true)`）：标题、歌手、专辑、专辑歌手、流派、年份、音轨 `[ ] / [ ]`、碟号 `[ ] / [ ]`、作曲、注释。初值来自后台 `read_fields(path)`；载入前显示「正在读取…」。每个输入框的无障碍标签就是字段名（如 `标题`）。
   - 只有改过的字段进 `TagEdits`（对比初值）；全都没改时「保存」禁用。
   - 底部：`取消`（Outline）/ `保存`（Default）。保存中按钮显示 `正在保存…` 并禁用；完成后关闭弹窗；有失败时不关闭，在弹窗内用红色文字显示第一条错误。
   - Esc / 点遮罩 = 取消（`modal` 已处理）。
4. 入口：
   - `track_menu`（`playlists.rs`）在「在访达中显示」之后加 `item("编辑信息…", Some(Icon::Edit), move || if let Some(p) = path() { store::open_edit_info(&p) })`。
   - 全屏页 `more_menu`（`column.rs`）在「在访达中显示」之前加同名菜单项（用该文件局部的 `item` 写法）。

**Verify**：`nsh check .` → exit 0；`nsh run .` 手动：改一首歌的标题保存，列表与播放条（若在播放）立即显示新标题。

### Step 7：UI 测试

`tests/ui.rs` 新增 `edit_info_dialog_opens_with_current_tags`（需要语料；持 `play_lock()`）：
- **不能改共享语料**（e2e 断言语料文件数与标签，UI 测试共用全局曲库单例），所以这个用例**不点保存**，只验证弹窗流程：打开 `sine.flac` 的右键菜单（参照 `track_row_double_click_like_and_menu` 的右键写法）→ `编辑信息…` → 等 `label:标题` 出现且值为 `测试歌曲` → 按 `取消` → 弹窗消失。写入正确性由 Step 2 的单测覆盖。

**Verify**：`cargo test --test ui edit_info` → 1 passed；`nsh test .` → exit 0。

### Step 8：README

「浏览与搜索」里曲目右键菜单那条，「重命名文件」之前加「编辑信息（标题、歌手、专辑、流派、年份、音轨 / 碟号、作曲、注释、封面，直接写回文件）」；「全屏播放」第一条的「⋯」菜单补「编辑信息」。

## Test plan

- 写回单测 6 个（覆盖 8 种容器往返、清除、非法输入不动文件、封面替换 / 移除、失败无残留、裸 AAC）；UI 测试 1 个（只验证弹窗打开 / 初值 / 取消）。

## Done criteria

- [ ] `cargo test tag_write -- --nocapture` 全部通过且未跳过；`cargo test --test ui edit_info` 通过；`nsh check .`、`nsh test .` exit 0；交叉检查 exit 0
- [ ] 测试没往语料目录写文件：`find .nsh/tmp/zeo-player-test-music .nsh/tmp/zeo-player-test-formats -name '*.zeo-tmp'` 无输出，且重跑 `sh scripts/make-test-music.sh` 前后 `cargo test` 结果一致
- [ ] README 已更新；`plans/README.md` 状态行更新

## STOP conditions

- 计划 003 未完成（没有 formats 语料）。
- 任一容器往返后 `Source::open` 解码失败（写标签破坏了音频）——立即停，不要试图「修」文件格式。
- lofty 对某容器 `save_to_path` 报错且不是 `raw.aac`：汇报容器名与错误。
- 引擎 `Release` 之后 Windows 上 `rename` 仍持续失败（交叉编译无法验证时，在汇报里注明需 Windows 实机验证）。
- 需要修改 `MetaInfo` 结构或数据库表结构。

## Maintenance notes

- 012 直接复用 `TagEdits` / `save_tags` / `refresh_paths` / `read_fields`。
- 新增可编辑字段：同时改 `TagField`、字段映射、`read_fields`、弹窗表单与 012 的表格列。
- 审阅重点：临时文件在所有错误路径都被删除；`Set("")` 等价 `Clear`；数字字段在复制文件前校验；正在播放的曲目写完后位置与播放状态恢复。
