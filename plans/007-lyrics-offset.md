# Plan 007：歌词提前 / 延后（LRC `[offset:]` + 每曲手动偏移 + 全局偏移）

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- src/metadata/lyrics.rs src/db.rs src/config/mod.rs src/ui/store.rs src/ui/now_playing/lyrics.rs src/ui/views/settings.rs src/ui/views/settings tests/ui.rs README.md`
> 有输出时先核对下文摘录。特别确认两点：`src/db.rs` 的 `SCHEMA_VERSION`（005 之后为 4；若是别的值，本计划用「当前值 + 1」，下文的版本号一并改）；设置文件是否已按 001 拆成 `src/ui/views/settings/` 目录（决定 Step 8 改哪个文件）。

## Status

- **Priority**: P2
- **Effort**: S–M
- **Risk**: LOW（数据库新增一张表 + 一个配置字段）
- **Depends on**: plans/001-split-settings.md（仅设置节的落点文件）、plans/005-replaygain.md（仅数据库版本号顺序；内容无依赖）
- **Category**: direction（issue #3）
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

很多 LRC 与音频版本对不上，整体早或晚零点几秒到几秒。issue #3 要求能「提前 / 延后显示」。三部分：

1. LRC 文件里的 `[offset:±毫秒]` 标签现在被识别后直接丢弃，没应用——这是现成的 bug。
2. 每首歌可手动调整偏移（±0.5 秒一步），记住设置。
3. 一个**全局偏移**（毫秒）：对所有曲目生效，补偿整库歌词的固定偏差（例如蓝牙耳机的输出延迟让高亮总是慢半拍）；与每曲的手动偏移相加生效。

## 约定（已定，不要改）

- 符号与 LRC 规范一致：**正值 = 歌词提前显示**。`[offset:+500]` 表示每行时间减 0.5 秒。手动偏移（每曲与全局）同理：`+0.5s` 让高亮提前 0.5 秒出现。
- LRC 的 `[offset:]` 在解析时直接改行时间；手动偏移不改解析结果，只在「算当前行」与「点行跳转」两处按**全局 + 每曲之和**叠加。
- 每曲手动偏移存 SQLite（新表 `lyrics_offsets`），随文件移动 / 改名迁移，删除文件 / 移除记录时清掉；**重建曲库（`clear_meta`）与扫描清理（`retain_meta`）不清它**。
- 全局偏移存 `library.json`（`LibraryConfig.global_lyrics_offset_ms`，默认 0），是设置不是曲目数据，**不进 SQLite**，也不参与路径迁移。
- 两个值各自钳到 ±30000 毫秒。全屏控件只显示与调节每曲那份（「重置」只清它），全局那份只在设置里改、在全屏控件里以小字提示。

## Current state

- `src/metadata/lyrics.rs`：
  - `parse_lyric(text, options)`（47 行）逐行 `split_timestamps`；没有时间戳的行若 `is_meta_tag_line(line)` 就丢弃：
    ```rust
    if times.is_empty() {
        // 纯元数据标签行（[ti:…] 等）整体丢弃，其余作为无时间文本保留。
        if !is_meta_tag_line(line) {
            untimed.push(clean_content(line).trim().to_owned());
        }
        continue;
    }
    ```
  - `is_meta_tag_line`（约 215 行）把 `ti ar al by offset re ve length au` 视为元数据。
  - 时间排序后经 `merge_leading_metadata`、`merge_translations`、`apply_empty_line_policy` 生成 `LrcLine { index, time, raw_content, trans_content, metadata }`；`time < 0` 表示无时间。
  - 文件里已有 `#[cfg(test)] mod tests`（解析用例的现成范例）。
- `src/config/mod.rs`：`LibraryConfig`（`library.json`；各字段 `#[serde(default)]`，`Default` 是手写的完整字面量——新字段两处都要加）；`LibraryStore::update_config(f)` 克隆当前配置 → 改 → `save_config` 写盘 → 换新 `Arc`，setter 范式见 `set_lyrics_align`（约 423 行）。
- `src/ui/store.rs`：
  - `active_line` memo（`install()` 内，约 214 行）：
    ```rust
    let active_line = computed(move || {
        let time = playback.with(|p| p.time);
        lyrics.with(|l| match l {
            Some(l) if l.output.timed => active_index(&l.output, time),
            _ => -1,
        })
    });
    ```
  - `active_index(output, time)`（约 839 行）：`line.time <= time + lookahead` 的最后一条（前瞻 0.5 s，开头 1 s 内不前瞻）。
  - 换曲 effect（约 386–439 行）在后台 `blocking` 里读 meta、歌词（`Meta::lyrics(&p)`）、波形，`still(&p)` 判断是否仍是当前曲。
  - `update_config(f)`（约 515 行）改 `library` 信号的配置快照让界面立即生效；正式落盘是 `std::thread::spawn(move || library_store().set_*(…))`——范式 `set_lyrics_align`（约 534 行）。`library` 信号每 120ms 被 `set` 一次但 `LibraryState` 有 `PartialEq`、没变不通知，memo 订阅它是安全的。
  - `pub fn seek_to(seconds)`（约 1162 行）。
- `src/ui/now_playing/lyrics.rs`：`lyrics_column(swap)`（45 行）是歌词区根 view，有歌词时渲染 `lyrics_scroller()`；行点击（约 394–405 行）：
  ```rust
  .on_press(move |_: &()| {
      if seekable {
          …
          seek_to(time);   // time = line.time
          scroll_to_line(i);
          follow.set(true);
      }
  }),
  ```
  行时间戳在悬停时显示（`stamp`，约 320 行，用 `line.time`）。全屏页固定深色，文字色用 `white_a(..)`；「⋯」按钮的写法在 `now_playing/column.rs`。
- 设置弹窗 `src/ui/views/settings.rs`（001 之后拆成 `src/ui/views/settings/`，歌词节在 `lyrics.rs`，节在 `mod.rs` 的 `SECTIONS` 注册表里；分类枚举 `Pane::Lyrics`）：节由 `setting_section(标题, 说明, 右上角操作)` 起手；数字输入用 `components::form::input_field`（现成写法见 `src/ui/views/playlists.rs:606`「数量上限」；计划 002 把「淡入淡出」改成同款，其「本地草稿 + `on_submit`/`on_blur` 提交校验」模式直接照搬）。`input` 有 `.label(..)`（无障碍标签 = UI 测试选择器）、`.value(..)`、`.on_submit(|t: &str|)`、`.on_blur(||)`。
- `src/db.rs`：`SCHEMA_VERSION`（005 之后为 4），`migrate(conn)` 按 `if version < N` 递进；写操作经 `with(what, |conn| …)`；异步写用 `db::later(f)`。路径同步函数：
  - `move_meta(rows: &[(old, new, stamp, modified)])`（事务里 `UPDATE tracks SET path = ?1 … WHERE path = ?4`）——文件 / 文件夹移动与改名都走它（经 `MetaCache::move_path` 与 `LibraryStore::move_path`）。
  - `remove_meta(path)`、`remove_meta_under(path)`（`path = ?1 OR substr(path, 1, ?2) = ?3`）——删除记录 / 删除文件夹 / 移除根时调用。
  - `clear_meta()`（重建曲库）、`retain_meta(keep)`（扫描后清理不存在的文件）——**不要**让它们动新表。
  - `db()` 在 `cfg(test)` 且未设 `ZEO_PLAYER_DATA_DIR` 时返回 `None`（单测不碰真库；UI 测试经 `isolate()` 设了它，库里能真读写）。

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 测试语料 | `sh scripts/make-test-music.sh` | 打印 wrote … |
| 编译 | `nsh check .` | exit 0 |
| 单测 | `cargo test lyric_offset` | 新用例全部通过 |
| UI 测试 | `cargo test --test ui lyrics_offset` | 新用例全部通过 |
| 全部测试 | `nsh test .` | exit 0 |

## Scope

**In scope**：`src/metadata/lyrics.rs`、`src/db.rs`、`src/config/mod.rs`、`src/ui/store.rs`、`src/ui/now_playing/lyrics.rs`、`src/ui/views/settings/lyrics.rs`（001 未执行时改 `src/ui/views/settings.rs`，连同 `SECTIONS`）、`tests/ui.rs`、`README.md`。

**Out of scope**：把偏移写回 `.lrc` 或内嵌歌词（属于计划 011/012 范畴）；快捷键（#9 暂不做）；`active_index` 里已有的 0.5 秒前瞻逻辑；把全局偏移做成每源（provider）配置。

## Git workflow

不要 commit / push。

## Steps

### Step 1：解析并应用 `[offset:]`

在 `parse_lyric` 的逐行循环里，遇到无时间戳且 `is_meta_tag_line` 的行时，额外检查是否是 `offset`：新增

```rust
/// `[offset:+500]` / `[offset:-250]` / `[offset: 300 ]` → 毫秒；不是 offset 行或解析失败为 `None`。
fn parse_offset_tag(line: &str) -> Option<i64>
```

记下最后一个成功解析的值 `offset_ms`。在把 `timed_entries` 转成 `Entry` 之前（排序之后即可），对每个时间 `t` 做 `t = (t - offset_ms as f64 / 1000.0).max(0.0)`。无时间戳回退分支不受影响。

**Verify**：`nsh check .` → exit 0。

### Step 2：解析单测

在 `lyrics.rs` 的 `mod tests` 加 `lyric_offset_tag_shifts_times`：
- `"[offset:+500]\n[00:02.00]a\n[00:05.00]b"` → 两行时间 1.5、4.5。
- `"[offset:-1000]\n[00:01.00]a"` → 2.0。
- `"[offset:+3000]\n[00:01.00]a"` → 0.0（钳到 0）。
- `"[offset:abc]\n[00:01.00]a"` → 1.0（非法忽略）。
- 无 offset 的现有用例结果不变（不改现有断言）。

**Verify**：`cargo test lyric_offset` → 通过；`cargo test lyrics` → 原有歌词用例全部通过。

### Step 3：数据库表与 API

`src/db.rs`（版本号取「执行时当前值 + 1」，下面按预期的 4→5 写）：
1. `SCHEMA_VERSION` 改为 5；`migrate` 加：
   ```rust
   if version < 5 {
       // 每首曲目的手动歌词偏移（毫秒，正值 = 提前）。跟随文件移动；
       // 重建曲库不清。
       conn.execute_batch(
           "CREATE TABLE IF NOT EXISTS lyrics_offsets (
               path TEXT PRIMARY KEY,
               offset_ms INTEGER NOT NULL
           );",
       )?;
   }
   ```
2. 新增：
   ```rust
   /// `path` 的手动歌词偏移（毫秒）；没有为 0。
   pub fn lyrics_offset(path: &str) -> i64
   /// 设置偏移；0 删除该行。
   pub fn set_lyrics_offset(path: &str, offset_ms: i64)
   ```
3. 路径同步：
   - `move_meta` 的事务里，对每行再执行 `UPDATE lyrics_offsets SET path = ?1 WHERE path = ?2`。
   - `remove_meta` 里加 `DELETE FROM lyrics_offsets WHERE path = ?1`；`remove_meta_under` 里加同样前缀条件的删除。
   - `clear_meta`、`retain_meta` **不改**。
4. 为了能单测，把 SQL 写成接收 `&Connection` 的私有函数（如 `fn lyrics_offset_on(conn: &Connection, path: &str) -> rusqlite::Result<i64>`），公开函数用 `with(…)` 包一层。

**Verify**：`nsh check .` → exit 0。

### Step 4：数据库单测

`db.rs` 末尾新建 `#[cfg(test)] mod tests`：`Connection::open_in_memory()` → `migrate(&conn)` →
- `lyric_offset_roundtrip`：设 `/m/a.flac` 为 500，读回 500；设 0 后读回 0 且表里无行。
- `lyric_offset_follows_move`：插 `/m/a.flac`=500 与 `tracks` 里一行同路径（或只测 offsets 表的 UPDATE 语句——若 `move_meta` 依赖全局 `db()`，则把它的事务体也抽成 `fn move_meta_on(tx, rows)` 再测）；移到 `/n/a.flac` 后旧路径 0、新路径 500。
- `lyric_offset_removed_under_folder`：`/m/x/a.flac`=300，`remove_meta_under("/m/x")` 的 SQL 执行后为 0；`/m/y.flac` 不受影响。
- `migrate_from_v4_keeps_tracks`：先建 v4 结构（执行 `SCHEMA_V1` 与 v2/v3/v4 的语句——含 005 的 `rg_` 列——并把 `user_version` 设为 4），插一行 `tracks`，再 `migrate` → `user_version == 5`、`tracks` 行还在、`lyrics_offsets` 表存在。（`db.rs` 的 `mod tests` 若 005 先建了就往里加；版本号同样按执行时实际值调整。）

**Verify**：`cargo test lyric_offset` → 全部通过。

### Step 5：全局偏移的配置字段

`src/config/mod.rs`：
1. `LibraryConfig` 加字段（`Default` 的字面量里同步加 `0`）：
   ```rust
   /// 全局歌词偏移（毫秒，正值 = 歌词提前显示；与每首曲目的手动偏移相加生效）。
   #[serde(default)]
   pub global_lyrics_offset_ms: i64,
   ```
2. `LibraryStore` 加（照 `set_lyrics_align` 的写法）：
   ```rust
   /// 全局歌词偏移（毫秒，正值提前）。
   pub fn global_lyrics_offset_ms(&self) -> i64
   /// 设置全局歌词偏移（保存）。
   pub fn set_global_lyrics_offset_ms(&self, ms: i64)
   ```

**Verify**：`nsh check .` → exit 0。

### Step 6：store 接线

`src/ui/store.rs`：
1. `Store` 加 `pub lyrics_offset: Signal<i64>`（当前曲目的每曲手动偏移，毫秒）。
2. `install()` 里在 `active_line` **之前**创建 `let lyrics_offset = signal(0_i64);`，`active_line` 改为按**生效偏移**（每曲 + 全局）算：
   ```rust
   let active_line = computed(move || {
       let time = playback.with(|p| p.time);
       let offset = lyrics_offset.get() + library.with(|l| l.config.global_lyrics_offset_ms);
       lyrics.with(|l| match l {
           Some(l) if l.output.timed => active_index(&l.output, time + offset as f64 / 1000.0),
           _ => -1,
       })
   });
   ```
3. 换曲 effect 里，开头与 `s.lyrics.set(None)` 并列处 `s.lyrics_offset.set(0)`；与读歌词并列地在后台读 `crate::db::lyrics_offset(&p)`，`still(&p)` 时写入信号。
4. 新增（都放在「命令」区或 `set_lyrics_align` 附近，保持文件分区）：
   ```rust
   /// 当前曲目的歌词偏移加 `delta_ms`（钳到 ±30000）并保存；`None` 时清零。
   pub fn nudge_lyrics_offset(delta_ms: Option<i64>)
   /// 全局歌词偏移（立即生效并保存；钳到 ±30000 毫秒）。
   pub fn set_global_lyrics_offset(ms: i64)
   ```
   `nudge_lyrics_offset` 用 `player().state().path` 取当前曲；更新信号；`crate::db::later(move || crate::db::set_lyrics_offset(&path, v))`。
   `set_global_lyrics_offset` 按设置范式：`let ms = ms.clamp(-30000, 30000);` → `update_config(|c| c.global_lyrics_offset_ms = ms)` → `std::thread::spawn(move || library_store().set_global_lyrics_offset_ms(ms))`。

**Verify**：`nsh check .` → exit 0。

### Step 7：全屏歌词区的偏移控件

`src/ui/now_playing/lyrics.rs`：
1. 格式化助手：
   ```rust
   /// `+0.5s` / `−0.5s`（一位小数、带符号；负号用 `−` U+2212）。
   fn fmt_offset(ms: i64) -> String
   ```
2. 行点击改为按**生效偏移**跳：`seek_to((time - offset_secs).max(0.0))`，其中
   `offset_secs = (s.lyrics_offset.get_untracked() + s.library.with(|l| l.config.global_lyrics_offset_ms)) as f64 / 1000.0`（点击时读）。悬停时间戳仍显示 `line.time`（文件里的时间）。
3. 在 `lyrics_column` 的根 view 里叠一个右上角小控件（`absolute`，`top`/`right` 留 12pt），仅当歌词是带时间的（`l.output.timed`）时出现：`[−]  +0.5s  [+]`，以及每曲偏移非 0 时出现的「重置」小按钮。
   - 按钮用现有 `components::button::icon_button` / `text_button`（看 `now_playing/column.rs` 里「⋯」按钮的写法与配色，全屏页固定深色，颜色用 `white_a(..)`）。
   - 无障碍标签：`歌词延后`（−0.5s，`nudge_lyrics_offset(Some(-500))`）、`歌词提前`（+0.5s）、`重置歌词偏移`（`nudge_lyrics_offset(None)`）。
   - 中间文字：每曲偏移 0 时显示 `偏移 0s`；否则 `fmt_offset(每曲)`。**全局偏移非 0 时**在其下加一行小字 `全局 {fmt_offset(全局)}`（11pt、`white_a(0.4)`）——提醒这里显示的与调节的只是每曲那份。单行文字都配 `.line_height(px(字号))`。
   - 默认半透明（`opacity 0.5`），悬停变 1（`.hover(|st| st.opacity(1.0))`），不抢歌词的视觉。

**Verify**：`nsh check .` → exit 0。

### Step 8：设置里的全局偏移节

`src/ui/views/settings/lyrics.rs`（001 未执行时在 `settings.rs` 里加同样一节）新增：

```rust
/// 全局歌词偏移：所有曲目统一提前 / 延后（毫秒；与每首曲目的手动偏移相加生效）。
pub(super) fn lyrics_offset_section() -> View
```

- `setting_section("全局歌词偏移", "对所有曲目生效：蓝牙耳机等固定延迟用它补偿。正值提前显示，负值延后；与全屏歌词区里每首曲目的偏移相加生效。", None)`。
- 数字输入写法同计划 002：本地草稿 `signal(当前 global_lyrics_offset_ms.to_string())`；`input_field(true).w(px(88.0)).label("全局歌词偏移").keyboard_type(KeyboardType::Number).value(move || draft.get())`，右侧跟 `毫秒` 单位文字；`on_change` 只更新草稿，`on_submit` 与 `on_blur` 触发同一个 `commit`：`trim()` → 去开头的 `+` → `parse::<i64>()`，成功则 `store::set_global_lyrics_offset(v)`（它内部钳 ±30000）并把草稿改成规范化值；失败（空串、字母、超范围数字解析错误）草稿回退为当前配置值。
- `effect(move || …)` 把草稿同步成 `s.library.with(|l| l.config.global_lyrics_offset_ms).to_string()`——外部改了配置（如另一实例）时输入框跟上；提交后值相同，不打断输入。
- `SECTIONS` 里注册 `{ pane: Pane::Lyrics, title: "全局歌词偏移", keys: "歌词 偏移 全局 提前 延后 同步 offset 蓝牙 延迟", build: lyrics_offset_section }`，放在「歌词对齐」「歌词来源」之后。

**Verify**：`nsh check .` → exit 0。

### Step 9：UI 测试

`tests/ui.rs` 新增两个用例（都需语料：`isolate()` 返回 false 时直接 return；都持 `play_lock()`、用 `start_inner`）：

1. `lyrics_offset_nudges_and_resets`：参照 `full_player_opens_and_closes` 的打开全屏方式与播放用例的语料路径（`app.press("zeo-player-test-music")` → `double_press(&mut app, "role:button[测试歌曲]")` 播 `sine.flac`，它有同名 `sine.lrc`）→ `app.press("label:全屏播放")` → 等 `label:歌词提前` 出现；按两次 → 屏幕出现文字 `+1.0s`；按 `label:重置歌词偏移` → 出现 `偏移 0s`。结束时 `Player::global().pause()`。
2. `lyrics_offset_global_saves`：设置 →「歌词」分类 → 等 `label:全局歌词偏移`；
   - `clear` + `type_text "300\n"`（`\n` 触发 submit）→ 写盘在后台线程，轮询（每 50ms、最多 2s）读 `ZEO_PLAYER_DATA_DIR` 下的 `library.json`，直到 `global_lyrics_offset_ms == 300`（读法照抄计划 002 的 `saved_fade_ms`；002 未执行就自己写个 `saved_global_offset()` 小助手，`serde_json` 取字段）。
   - `clear` + `type_text "99999\n"` → 库值为 `30000`（钳制）；`clear` + `type_text "abc\n"` → 仍为 `30000`（回退）。
   - 改为 `300` 后关设置、播 `sine.flac`、开全屏 → 等 `label:歌词提前`，歌词区出现文字 `全局 +0.3s`，且 `偏移 0s` 仍在（每曲没动）。
   - 回设置把值改回 `0` 并等 `library.json` 归 0（共享全局配置，别把值留给同进程其它用例），最后 `Player::global().pause()`。

**Verify**：`cargo test --test ui lyrics_offset` → 全部 passed；`nsh test .` → exit 0。

### Step 10：README

「全屏播放」的「行级歌词」一条末尾补：`歌词与音频对不上时，用歌词区右上角的 − / + 按 0.5 秒提前或延后（每首单独记住）；整库歌词有固定偏差（如蓝牙耳机延迟）时，在设置 →「歌词」→「全局歌词偏移」里设统一的毫秒偏移，两者相加生效；LRC 文件里的 [offset:] 标签也会生效。`

## Test plan

- 解析单测 1 个（4 种输入）、数据库单测 4 个、UI 测试 2 个（每曲 nudge / 重置、全局设置的保存 / 钳制 / 回退与全屏小字提示）。

## Done criteria

- [ ] `nsh check .`、`nsh test .` exit 0
- [ ] `cargo test lyric_offset`、`cargo test --test ui lyrics_offset` 全部通过
- [ ] `rg -n "SCHEMA_VERSION: i32 = 5" src/db.rs` 有输出（或「当前值 + 1」）
- [ ] `rg -n "lyrics_offsets" src/db.rs` 只出现在建表、读写、`move_meta`、`remove_meta`、`remove_meta_under` 中，不在 `clear_meta` / `retain_meta` 中
- [ ] `rg -n "global_lyrics_offset_ms" src/config/mod.rs src/ui/store.rs` 有输出；`rg -n "全局歌词偏移" src/ui/views` 有输出
- [ ] README 已更新；`plans/README.md` 状态行更新

## STOP conditions

- `SCHEMA_VERSION` 不是 4 且你无法确定别的计划加了什么迁移（避免两个计划用同一个版本号）。
- `move_meta` / `remove_meta*` 的结构与摘录不同（例如已被重构成别的形式）。
- 全屏页歌词区找不到合适的根 view 挂控件（`lyrics_column` 结构已变）。
- `input_field`/`Input` 没有 `.value(..)` + `.on_submit(..)` + `.on_blur(..)`，无法实现草稿 + 提交校验。
- 设置文件既不是 `settings.rs`（带 `SECTIONS`）也没有拆出 `settings/lyrics.rs`（001 以别的形式落地）。

## Maintenance notes

- 计划 010 会再升一次 `SCHEMA_VERSION`（播放记录表），并同样接入 `move_meta` 的路径同步——它可以照抄本计划的写法。
- 若以后加「把偏移写回 .lrc」，应写 `[offset:]` 标签而不是改每行时间。
- 全局偏移在 `library.json`；换设备 / 清数据目录即归 0，这是有意的（它补偿的是当前输出链路的延迟）。
