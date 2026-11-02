# Plan 010：播放统计 + 首页（默认页）

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- src/db.rs src/engine/audio.rs src/engine/mod.rs src/config/mod.rs src/ui/mod.rs src/ui/frame/sidebar.rs src/ui/store.rs src/ui/views src/ui/components/icons.rs tests/ui.rs README.md`
> 前序计划（001–009）的改动是预期内的。开始前确认：`src/db.rs` 的 `SCHEMA_VERSION` 为 5（005 已加 `rg_` 四列、007 已加 `lyrics_offsets` 表）；若是别的值，本计划用「当前值 + 1」，下文的版本号一并改。

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: MED（新表 + 引擎计数 + 新路由，且改变默认启动页）
- **Depends on**: plans/001-split-settings.md、plans/005-replaygain.md 与 plans/007-lyrics-offset.md（仅数据库版本号顺序）
- **Category**: direction（issue #1）
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

issue #1：首页，含随机 5 首的 banner、歌曲 / 歌单统计；统计按「最小播放百分比 / 时长」判定一次播放，阈值可配置。用户确认：**首页作为默认启动页；播放超过 60% 或超过 30 秒计一次**。现在数据库里没有任何播放记录，所以要先在引擎里计数、落库，再做页面。

## 约定（已定，不要改）

- 「播放了多少」按**实际输出的时长**算（输出回调推进的帧数），不是按进度位置——拖动进度条不算听过。
- 判定：`listened >= min_secs` **或** `listened >= duration * min_percent / 100`（两个阈值任一为 0 表示不启用该条件；两个都为 0 则开始播放即计）。默认 `min_percent = 60`、`min_secs = 30`。
- 每次装载曲目（`load_track`）最多计一次；单曲循环每轮重新装载，所以每轮各计一次。在**越过阈值的那一刻**计数（不等播完），应用中途退出也不丢。
- 播放记录跟随文件移动 / 改名（`move_meta`）；删除文件或重建曲库**不删**播放记录（历史保留）；统计排行只显示曲库里还在的曲目。
- 不做「清空播放记录」按钮（以后再说）。

## Current state

- 数据库 `src/db.rs`：`SCHEMA_VERSION`（005+007 之后为 5）、`migrate(conn)` 递进；`with(what, |conn| …)`；`db::later(f)` 在写线程执行；`move_meta(rows)` 事务里更新 `tracks`（007 之后也更新 `lyrics_offsets`）。007 已建立「SQL 写成接收 `&Connection` 的私有函数、公开函数用 `with` 包一层、`mod tests` 用 `Connection::open_in_memory()` + `migrate`」的模式——照抄。
- 引擎 `src/engine/audio.rs`：
  - `Shared.played: AtomicU64`（输出帧数，含 seek 基线）在 `output_callback` 里 `fetch_add(frames)`；`dev_rate` 换算秒。
  - `load_track(…)` 装载曲目并填 `TrackPublic { path, title, artist, album, duration, … }`。
  - 主循环每轮：命令 → 解码填充 → 媒体心跳（每秒）→ 等待。
  - 004/005/006 之后 `output_callback` 的签名与增益链已变，按实际代码修改。
- 导航 `src/ui/mod.rs`：
  ```rust
  #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
  pub enum Nav { #[default] Folders, Artists, Albums, Genres, Playlist(u64) }
  ```
  `page()` 里 `match nav.get() { Nav::Folders => views::folders().into_element(), … }`。`bench()` 里也有 `nav.set(Nav::Folders)`。`src/ui/store.rs:933`（删除歌单后）`nav.set(super::Nav::Folders)`。
- 侧栏 `src/ui/frame/sidebar.rs:17`：
  ```rust
  const LIBRARY: [(Nav, &str, Icon); 4] = [
      (Nav::Folders, "文件夹", Icon::Folder), (Nav::Artists, "歌手", Icon::Mic),
      (Nav::Albums, "专辑", Icon::Disc), (Nav::Genres, "标签", Icon::Tag),
  ];
  ```
  279 行 `.children(LIBRARY.iter().map(|&(target, label, glyph)| …))`。
- 图标 `src/ui/components/icons.rs`：`enum Icon` 没有「首页」图标；新增图标按文件里现有的 SVG path 写法加（例如照 `Folder` 的实现方式加一个 `Home`，用 lucide 的 house 图形）。
- 页面部件 `src/ui/components/page.rs`：`page_header(title, subtitle, back, actions)`、`empty_state(icon, title, body)`；按钮 `components::button::pill_button(icon, label, on_press)`；曲目行 `components::tracks::{track_row, track_columns, Columns, ROW}`；网格封面 `store::cover_for(path, pt)`（AGENTS：≤48pt 槽位用 96px 文件，更大的槽位升到 400px）；`views/browse.rs` 的专辑网格是封面卡片的现成范例；`views/mod.rs` 的 `hero(...)` 是大头部范例。
- 统计已有的数字：`store().tracks`（全部曲目）、`store().playlists`；`store::format_total(seconds)` 格式化总时长；设置「曲库」节的 `stat(value, label)` 部件（`settings/library.rs`）可参考样式。
- 后台读取再回界面：`spawn(async move { let v = blocking(move || …).await; … })`（见 `store.rs` 换曲 effect）。
- 测试：`tests/ui.rs` 的 `start()` 最后 `app.wait_for("文件夹")`（侧栏文字，首页成为默认后仍在）。依赖「默认就是文件夹页」的用例需先 `app.press("文件夹")`。

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 测试语料 | `sh scripts/make-test-music.sh` | 打印 wrote … |
| 编译 | `nsh check .` | exit 0 |
| 数据库 / 计数单测 | `cargo test play_stats` | 全部通过 |
| 首页 UI 测试 | `cargo test --test ui home_` | 全部通过 |
| 全部测试 | `nsh test .` | exit 0 |

## Scope

**In scope**：`src/db.rs`、`src/engine/audio.rs`、`src/engine/mod.rs`、`src/config/mod.rs`、`src/ui/mod.rs`、`src/ui/frame/sidebar.rs`、`src/ui/store.rs`、`src/ui/views/mod.rs`、`src/ui/views/home.rs`（新建）、`src/ui/views/settings/playback.rs`、`src/ui/views/settings/mod.rs`、`src/ui/components/icons.rs`、`tests/ui.rs`、`README.md`。

**Out of scope**：智能歌单的「播放次数」条件；清空 / 导出播放记录；`#12` 侧栏布局与最小窗口宽度（待定，不要动）；首页之外的视图改版。

## Git workflow

不要 commit / push。

## Steps

### Step 1：播放记录表与查询

`src/db.rs`：
1. `SCHEMA_VERSION` +1（预期 6）；迁移：
   ```rust
   if version < 6 {
       // 播放记录：越过统计阈值时写一行。跟随文件移动；删除文件、
       // 重建曲库都不清（历史保留）。
       conn.execute_batch(
           "CREATE TABLE IF NOT EXISTS plays (
               id INTEGER PRIMARY KEY,
               path TEXT NOT NULL,
               played_at INTEGER NOT NULL,
               listened REAL NOT NULL
           );
           CREATE INDEX IF NOT EXISTS plays_path ON plays(path);
           CREATE INDEX IF NOT EXISTS plays_time ON plays(played_at);",
       )?;
   }
   ```
2. `move_meta` 的事务里加 `UPDATE plays SET path = ?1 WHERE path = ?2`。**不要**在 `remove_meta*`、`clear_meta`、`retain_meta` 里动 `plays`。
3. 新增（每个都配 `_on(conn, …)` 私有实现）：
   ```rust
   pub fn record_play(path: &str, played_at: u64, listened: f64)
   /// 统计摘要。
   #[derive(Debug, Clone, Default, PartialEq)]
   pub struct PlayStats {
       pub total: u64,                    // 全部播放次数
       pub since: u64,                    // `since` 之后的次数
       pub top_tracks: Vec<(String, u64)>,  // (路径, 次数)，降序
       pub top_artists: Vec<(String, u64)>, // (tracks.artist, 次数)，只统计曲库里还在的曲目
       pub recent: Vec<String>,           // 最近播放的路径（去重，最新在前）
   }
   pub fn play_stats(since: u64, limit: usize) -> PlayStats
   ```
   - `top_tracks`：`SELECT p.path, COUNT(*) c FROM plays p JOIN tracks t ON t.path = p.path GROUP BY p.path ORDER BY c DESC, MAX(p.played_at) DESC LIMIT ?`。
   - `top_artists`：同样 JOIN，`GROUP BY t.artist`（`artist` 是标签原文，可能是 `A; B` 这样的组合——按原文分组，不拆分，写注释说明）。
   - `recent`：`SELECT p.path FROM plays p JOIN tracks t ON t.path = p.path GROUP BY p.path ORDER BY MAX(p.played_at) DESC LIMIT ?`。
   - 数据库不可用（`db()` 为 `None`）时返回 `PlayStats::default()`。

**Verify**：`nsh check .` → exit 0。

### Step 2：数据库单测

`db.rs` 的 `mod tests` 加（名字都以 `play_stats_` 开头）：
- `play_stats_counts_and_orders`：内存库，`tracks` 插 3 行（a/b/c，artist 分别 X/X/Y），`plays`：a×3、b×1、c×2、外加 1 条路径不在 `tracks` 里的记录 → `total == 7`；`top_tracks` 为 a、c、b（不含孤儿）；`top_artists` 为 X(4)、Y(2)；`since` 只计时间戳大于参数的。
- `play_stats_follow_move`：移动 a → a2 后 `top_tracks` 里是 a2。
- `play_stats_survive_clear`：执行 `clear_meta` 的 SQL 后 `SELECT COUNT(*) FROM plays` 不变（排行因 JOIN 为空，正常）。
- `play_stats_migrate_from_v5`：建好 v5 结构（含 `rg_` 列与 `lyrics_offsets`）、`user_version = 5` → `migrate` → `user_version == 6` 且 `plays` 表存在、原有表数据还在。

**Verify**：`cargo test play_stats` → 4 passed。

### Step 3：引擎计数

`src/engine/audio.rs`：
1. 纯函数（单测用）：
   ```rust
   /// 听了 `listened` 秒、总长 `duration` 秒，是否算一次播放。
   pub(crate) fn counts_as_play(listened: f64, duration: f64, min_percent: u32, min_secs: u32) -> bool
   ```
   规则见「约定」；`duration <= 0` 时百分比条件视为不满足。
2. `Shared` 加 `pub listened: AtomicU64`（本曲已输出帧数）、`pub stats_min_percent: AtomicU32`（默认 60）、`pub stats_min_secs: AtomicU32`（默认 30）、`pub plays_gen: AtomicU64`（每记一次 +1，界面据此刷新）。
3. `output_callback` 里在推进 `played` 的同一处 `listened.fetch_add(frames)`。
4. `Loaded` 加 `counted: bool`；`load_track` 成功装载时 `listened.store(0)`、`counted = false`。
5. 主循环每轮（解码填充之后）：若 `loaded` 存在且 `!counted`，算 `listened_secs = listened / dev_rate`，`counts_as_play(listened_secs, duration, pct, secs)` 为真时：`counted = true`；`let (path, at) = …; crate::db::later(move || crate::db::record_play(&path, at, listened_secs));`（`at` 用 `crate::config::unix_now()`）；`plays_gen.fetch_add(1)`。
6. `Player` 加 `set_stats_thresholds(&self, min_percent: u32, min_secs: u32)`（写原子量，百分比钳到 0..=100、秒钳到 0..=3600）与 `plays_generation(&self) -> u64`。

单测（`audio.rs` 的 `mod tests`）`play_stats_threshold_rules`：
- (31, 300, 60, 30) → true（秒数满足）；(29, 300, 60, 30) → false；(29, 40, 60, 30) → true（29 ≥ 24）；(10, 0, 60, 30) → false；(0, 100, 0, 0) → true；(5, 100, 0, 10) → false；(50, 100, 50, 0) → true。

**Verify**：`cargo test play_stats` → 5 passed；`nsh test .` → exit 0。

### Step 4：统计阈值设置

1. `LibraryConfig` 加 `#[serde(default = "default_stats_min_percent")] pub stats_min_percent: u32`（60）与 `#[serde(default = "default_stats_min_secs")] pub stats_min_secs: u32`（30）；Default 补字段；`LibraryStore` getter/setter。
2. `LibraryState` 加两个字段；`store.rs` 加 `pub fn set_stats_thresholds(percent, secs)`（信号 + `player().set_stats_thresholds` + 后台保存）；`install()` 末尾推一次。
3. `settings/playback.rs` 加 `pub(super) fn stats_section() -> View`：`setting_section("播放统计", "听过多少才算一次播放（用于首页的统计与排行）。任一条件满足即计一次；填 0 表示不用该条件。", None)`，两行数字输入（沿用计划 002 的「草稿 + 提交时校验」写法）：`最少播放百分比`（0–100，单位 `%`，无障碍标签同名）与 `最少播放时长`（0–3600，单位 `秒`）。
4. `SECTIONS` 加 `{ pane: Pane::Playback, title: "播放统计", keys: "统计 播放次数 百分比 时长 首页 排行 阈值", build: playback::stats_section }`。

**Verify**：`nsh check .` → exit 0。

### Step 5：首页路由与侧栏

1. `Nav` 加 `Home` 并设为 `#[default]`；`page()` 加 `Nav::Home => views::home().into_element()`；`store.rs` 删除歌单后跳转改为 `Nav::Home`；`bench()` 保持原逻辑（它显式设 Folders，不受影响）。
2. `icons.rs` 新增 `Icon::Home`（lucide `house`，按文件里现有图标的定义方式）。
3. 侧栏 `LIBRARY` 改为 5 项，首项 `(Nav::Home, "首页", Icon::Home)`；数组长度 `[…; 5]`。图标栏（窄窗口）模式下也要正常显示（它遍历同一个数组）。
4. `src/ui/views/mod.rs` 加 `mod home;` 与 `pub(super) use self::home::home;`。

**Verify**：`nsh check .` → exit 0。

### Step 6：首页 `src/ui/views/home.rs`

模块头注释：`//! 首页：随机推荐、曲库与播放统计、最常 / 最近播放。`

`pub fn home() -> View`，整页一个可滚动容器（`scroll_view` 或 `virtual_list` 带 header 均可；内容不多，用普通纵向滚动即可，参照 `views/browse.rs` 的滚动容器写法），底部留 `BAR_SPACE`：

1. **页头**：`page_header(|| "首页".into(), 副标题, None, None)`，副标题如 `1,234 首 · 56 小时`（`store().tracks` 计数与 `format_total(total_duration)`）。
2. **随机推荐 banner**：从 `store().tracks` 随机取 5 首不重复（种子用 `SystemTime` 纳秒 + xorshift，同 `engine::audio::shuffled` 的写法，不引新依赖）；横排 5 张卡片（封面 `cover_initial(cover_for(path, 160.0), 首字, Some(160.0), 12.0)` + 曲名 + 歌手，单行文字配 `.line_height`）；**单击**卡片播放该曲，队列为这 5 首（`store::play_list(&five, i)`）；右上角「换一批」按钮（`Icon::Refresh`，无障碍标签 `换一批`）重新抽取。曲库少于 5 首时有几首显示几首；曲库为空时显示 `views::no_library()`（它是 `views/mod.rs` 的私有函数，改为 `pub(super)` 或在 `mod.rs` 里判断）。
3. **统计卡片**一行（窄时换行，`flex_wrap`）：`曲目`、`专辑`、`歌手`、`歌单`、`总时长`、`累计播放`、`近 7 天播放`。前五项从 store 现有数据算（专辑按 `(album_artist, album)` 去重，歌手按 `artist` 去重，与 `views/browse.rs` 的分组口径一致——读它的分组函数并复用；歌单数 = `store().playlists` 长度）；后两项来自 `db::play_stats`。
4. **最常播放**（前 10，`track_row` 风格，序号列显示次数 `×12`）与**最近播放**（前 10）两个列表；**最常播放的歌手**（前 5，名字 + 次数的简单行）。没有播放记录时三块合并显示一个 `empty_state(Icon::Wave, "还没有播放记录", "听过的歌会在这里统计。在设置 →「播放」→「播放统计」里调整计数规则。")`。
   - 行双击播放，队列为该列表（复用 `track_row` 的 `on_play`）。
   - 列表数据：`play_stats` 返回路径，用 `store().tracks` 建 `HashMap<&str, TrackEntry>` 映射成 `TrackEntry`；找不到的跳过。
5. **刷新时机**：进入页面时在后台 `blocking(|| crate::db::play_stats(now - 7*86400, 10))` 读一次；之后用一个 `effect` 每当 `store().playback` 的轮询发现 `player().plays_generation()` 变化（在首页内部 `spawn` 一个 2 秒的 `timer` 循环比较代次即可，页面卸载时任务随 owner 结束）就重读。

样式沿用现有页面（标题字号、间距、`theme::*` 颜色）；全部单行 `text()` 配 `.line_height(px(字号))`。

**Verify**：`nsh check .` → exit 0；`nsh run .` 手动看首页（汇报截图路径，存 `.nsh/tmp/home.png`）。

### Step 7：UI 测试

`tests/ui.rs`：
1. 先跑一遍 `nsh test .`，把因默认页变为首页而失败的用例逐个修：在用例开头 `app.press("文件夹").unwrap();`（只加这一行，不改断言）。在汇报里列出改了哪些用例。
2. 新增 `home_is_default_and_shows_stats`：`start(1200, 800)` 后断言出现 `首页`（页头）与统计标签 `曲目`、`累计播放`；有语料时（`isolate()` 为 true）断言 banner 至少一张卡片（`label:换一批` 存在且曲名之一可见），按 `换一批` 不报错。
3. 新增 `home_counts_a_play`（需要语料，持 `play_lock()`）：先在设置里把「最少播放时长」设为 1、「最少播放百分比」设为 0（沿用计划 002 的 `clear` + `type_text` 写法）；播放 `sine.flac`；等 3 秒；回到首页；等待文字 `测试歌曲` 出现在「最常播放」区（或 `累计播放` 的数值 ≥ 1）；最后把两个阈值恢复为 60 / 30 并暂停。

**Verify**：`cargo test --test ui home_` → 2 passed；`nsh test .` → exit 0。

### Step 8：README

在「曲库」之前加一节：

```
### 首页

- 打开应用默认进入首页：随机推荐 5 首（可「换一批」）、曲库统计（曲目 / 专辑 / 歌手 / 歌单 / 总时长）与播放统计（累计、近 7 天）、最常播放与最近播放、最常听的歌手。
- 播放超过 60% 或超过 30 秒计一次播放，两个阈值可在设置 →「播放」→「播放统计」里修改；拖动进度不算听过。
```

## Test plan

- 数据库单测 4 个、阈值规则单测 1 个、UI 测试 2 个（Step 2/3/7）；既有 UI 用例只为默认页变化补一行导航。

## Done criteria

- [ ] `cargo test play_stats` 5 passed；`cargo test --test ui home_` 2 passed；`nsh check .`、`nsh test .` exit 0
- [ ] `rg -n "#\[default\]\s*Home" src/ui/mod.rs`（或 `Home` 紧跟 `#[default]`）有输出
- [ ] `rg -n "plays" src/db.rs` 不出现在 `clear_meta`、`retain_meta`、`remove_meta`、`remove_meta_under` 函数体内
- [ ] README 已更新；`plans/README.md` 状态行更新

## STOP conditions

- `SCHEMA_VERSION` 既不是 5 也无法确定前序迁移（避免撞版本号）。
- 修复既有 UI 用例需要改动断言本身（不只是加一行 `press("文件夹")`）。
- `output_callback` 的结构与「004/005/006 之后」的描述差太多，找不到推进 `played` 的位置。
- 首页在 720pt 最小宽度下布局溢出且无法用 `flex_wrap` 解决（#12 待定，不要改最小宽度）——汇报截图。

## Maintenance notes

- 以后加智能歌单「播放次数 / 最近播放」条件，可直接查 `plays` 表（`playlists.rs` 的 `Field` 枚举）。
- `top_artists` 按标签原文分组；若以后统一拆分多歌手，记得同步这里。
- 审阅重点：计数只在越过阈值时写一次；`listened` 在 `load_track` 时归零；seek 不增加 `listened`（它只由回调推进）。
