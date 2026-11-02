# Plan 007：歌词提前 / 延后（LRC `[offset:]` + 每曲手动偏移）

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- src/metadata/lyrics.rs src/db.rs src/ui/store.rs src/ui/now_playing/lyrics.rs tests/ui.rs README.md`
> 有输出时先核对下文摘录。特别确认 `src/db.rs` 的 `SCHEMA_VERSION` 为 4（005 已加 `rg_` 四列）；若是别的值，本计划用「当前值 + 1」，下文的版本号一并改。

## Status

- **Priority**: P2
- **Effort**: S–M
- **Risk**: LOW（数据库新增一张表）
- **Depends on**: plans/005-replaygain.md（仅数据库版本号顺序；内容无依赖）
- **Category**: direction（issue #3）
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

很多 LRC 与音频版本对不上，整体早或晚零点几秒到几秒。issue #3 要求能「提前 / 延后显示」。两部分：

1. LRC 文件里的 `[offset:±毫秒]` 标签现在被识别后直接丢弃，没应用——这是现成的 bug。
2. 每首歌可手动调整偏移（±0.5 秒一步），记住设置。

## 约定（已定，不要改）

- 符号与 LRC 规范一致：**正值 = 歌词提前显示**。`[offset:+500]` 表示每行时间减 0.5 秒。手动偏移同理：`+0.5s` 让高亮提前 0.5 秒出现。
- LRC 的 `[offset:]` 在解析时直接改行时间；手动偏移不改解析结果，只在「算当前行」与「点行跳转」两处叠加。
- 手动偏移存 SQLite，随文件移动 / 改名迁移，删除文件 / 移除记录时清掉；**重建曲库（`clear_meta`）与扫描清理（`retain_meta`）不清它**。

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
- `src/ui/store.rs`：
  - `active_line` memo（`install()` 内，约 229 行）：
    ```rust
    let active_line = computed(move || {
        let time = playback.with(|p| p.time);
        lyrics.with(|l| match l {
            Some(l) if l.output.timed => active_index(&l.output, time),
            _ => -1,
        })
    });
    ```
  - 换曲 effect（约 377–429 行）在后台 `blocking` 里读 meta、歌词（`Meta::lyrics(&p)`）、波形，`still(&p)` 判断是否仍是当前曲。
  - `pub fn seek_to(seconds)`（约 1137 行）。
- `src/ui/now_playing/lyrics.rs`：`lyrics_column(swap)`（45 行）是歌词区根 view，有歌词时渲染 `lyrics_scroller()`；行点击（约 395–405 行）：
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
  行时间戳在悬停时显示（`stamp`，约 320 行，用 `line.time`）。
- `src/db.rs`：`SCHEMA_VERSION`（005 之后为 4），`migrate(conn)` 按 `if version < N` 递进；写操作经 `with(what, |conn| …)`；异步写用 `db::later(f)`。路径同步函数：
  - `move_meta(rows: &[(old, new, stamp, modified)])`（事务里 `UPDATE tracks SET path = ?1 … WHERE path = ?4`）——文件 / 文件夹移动与改名都走它（经 `MetaCache::move_path` 与 `LibraryStore::move_path`）。
  - `remove_meta(path)`、`remove_meta_under(path)`（`path = ?1 OR substr(path, 1, ?2) = ?3`）——删除记录 / 删除文件夹 / 移除根时调用。
  - `clear_meta()`（重建曲库）、`retain_meta(keep)`（扫描后清理不存在的文件）——**不要**让它们动新表。
  - `db()` 在 `cfg(test)` 且未设 `ZEO_PLAYER_DATA_DIR` 时返回 `None`（单测不碰真库）。

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 测试语料 | `sh scripts/make-test-music.sh` | 打印 wrote … |
| 编译 | `nsh check .` | exit 0 |
| 单测 | `cargo test lyric_offset` | 新用例全部通过 |
| 全部测试 | `nsh test .` | exit 0 |

## Scope

**In scope**：`src/metadata/lyrics.rs`、`src/db.rs`、`src/ui/store.rs`、`src/ui/now_playing/lyrics.rs`、`tests/ui.rs`、`README.md`。

**Out of scope**：把偏移写回 `.lrc` 或内嵌歌词（属于计划 011/012 范畴）；全局默认偏移设置；快捷键（#9 暂不做）；`active_index` 里已有的 0.5 秒前瞻逻辑。

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
- `migrate_from_v4_keeps_tracks`：先建 v4 结构（执行 `SCHEMA_V1` 与 v2/v3/v4 的语句——含 005 的 `rg_` 列——并把 `user_version` 设为 4），插一行 `tracks`，再 `migrate` → `user_version == 5`、`tracks` 行还在、`lyrics_offsets` 表存在。（`db.rs` 的 `mod tests` 若 005 先建了就往里加。）

**Verify**：`cargo test lyric_offset` → 全部通过。

### Step 5：store 接线

`src/ui/store.rs`：
1. `Store` 加 `pub lyrics_offset: Signal<i64>`（毫秒）。
2. 换曲 effect 里，与读歌词并列地在后台读 `crate::db::lyrics_offset(&p)`，`still(&p)` 时写入信号；换曲开始时先置 0。
3. `active_line` 改为 `active_index(&l.output, time + lyrics_offset.get() as f64 / 1000.0)`（注意 `lyrics_offset` 信号要在 `active_line` 之前创建）。
4. 新增：
   ```rust
   /// 当前曲目的歌词偏移加 `delta_ms`（钳到 ±30000）并保存；`None` 时清零。
   pub fn nudge_lyrics_offset(delta_ms: Option<i64>)
   ```
   用 `player().state().path` 取当前曲；更新信号；`crate::db::later(move || crate::db::set_lyrics_offset(&path, v))`。

**Verify**：`nsh check .` → exit 0。

### Step 6：全屏歌词区的偏移控件

`src/ui/now_playing/lyrics.rs`：
1. 行点击改为 `seek_to((time - offset_secs).max(0.0))`，其中 `offset_secs = s.lyrics_offset.get_untracked() as f64 / 1000.0`（点击时读）。悬停时间戳仍显示 `line.time`（文件里的时间）。
2. 在 `lyrics_column` 的根 view 里叠一个右上角小控件（`absolute`，`top`/`right` 留 12pt），仅当歌词是带时间的（`l.output.timed`）时出现：`[−]  +0.5s  [+]`，以及偏移非 0 时出现的「重置」小按钮。
   - 按钮用现有 `components::button::icon_button` / `text_button`（看 `now_playing/column.rs` 里「⋯」按钮的写法与配色，全屏页固定深色，颜色用 `white_a(..)`）。
   - 无障碍标签：`歌词延后`（−0.5s，`nudge_lyrics_offset(Some(-500))`）、`歌词提前`（+0.5s）、`重置歌词偏移`。
   - 中间文字：0 时显示 `偏移 0s`；否则 `+0.5s` / `−1.0s`（一位小数、带符号，用 `−`(U+2212) 或 `-` 都行，与周围文案一致即可）。单行文字配 `.line_height(px(字号))`。
   - 默认半透明（`opacity 0.5`），悬停变 1（`.hover(|st| st.opacity(1.0))`），不抢歌词的视觉。

**Verify**：`nsh check .` → exit 0。

### Step 7：UI 测试

`tests/ui.rs` 新增 `lyrics_offset_nudges_and_resets`，参照 `full_player_opens_and_closes` 的打开全屏方式；需要语料（`isolate()` 返回 false 时直接 return）：播放 `sine.flac`（它有同名 `sine.lrc`），打开全屏，等 `label:歌词提前` 出现；按两次 → 屏幕出现文字 `+1.0s`；按 `label:重置歌词偏移` → 出现 `偏移 0s`。结束时暂停播放（同其它用例 `Player::global().pause()`）。

**Verify**：`cargo test --test ui lyrics_offset` → 1 passed；`nsh test .` → exit 0。

### Step 8：README

「全屏播放」的「行级歌词」一条末尾补：`歌词与音频对不上时，用歌词区右上角的 − / + 按 0.5 秒提前或延后（每首单独记住）；LRC 文件里的 [offset:] 标签也会生效。`

## Test plan

- 解析单测 1 个（4 种输入）、数据库单测 4 个、UI 测试 1 个。

## Done criteria

- [ ] `nsh check .`、`nsh test .` exit 0
- [ ] `cargo test lyric_offset` 全部通过
- [ ] `rg -n "SCHEMA_VERSION: i32 = 5" src/db.rs` 有输出（或「当前值 + 1」）
- [ ] `rg -n "lyrics_offsets" src/db.rs` 只出现在建表、读写、`move_meta`、`remove_meta`、`remove_meta_under` 中，不在 `clear_meta` / `retain_meta` 中
- [ ] README 已更新；`plans/README.md` 状态行更新

## STOP conditions

- `SCHEMA_VERSION` 不是 4 且你无法确定别的计划加了什么迁移（避免两个计划用同一个版本号）。
- `move_meta` / `remove_meta*` 的结构与摘录不同（例如已被重构成别的形式）。
- 全屏页歌词区找不到合适的根 view 挂控件（`lyrics_column` 结构已变）。

## Maintenance notes

- 计划 010 会再升一次 `SCHEMA_VERSION`（播放记录表），并同样接入 `move_meta` 的路径同步——它可以照抄本计划的写法。
- 若以后加「把偏移写回 .lrc」，应写 `[offset:]` 标签而不是改每行时间。
