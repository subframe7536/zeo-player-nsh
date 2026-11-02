# Plan 001：把 `settings.rs` 按分类拆成目录模块（纯搬移，零行为变化）

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- src/ui/views/settings.rs src/ui/views/mod.rs`
> 有输出说明文件自计划编写后改过：先对照下文「现状」的函数列表与实际代码，对不上即 STOP。

## Status

- **Priority**: P1（后续 002/004/005/006/009/010 都要往设置里加节）
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: tech-debt
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

`src/ui/views/settings.rs` 有 1780 行，七个设置分类的全部 UI、弹窗框架、搜索逻辑都在一个文件里。接下来至少六个计划要往里加新节（数字输入、设备变化、ReplayGain、均衡器、托盘、统计），全挤在一个文件里既难审阅又容易冲突。本计划只做搬移：按分类拆成 `settings/` 目录下的子模块，行为、文案、测试完全不变。

## Current state

- `src/ui/views/settings.rs` — 设置弹窗。顶层条目（行号为 f6e6821 时）：
  - 共用小部件：`setting_section`(21)、`note`(47)、`pref_row`(407)、`stat`(552)
  - 曲库分类：`folders_section`(59)、`library_section`(241)、`watch_row`(466)、`filter_section`(814)、`delete_section`(374)
  - 封面分类：`covers_source_section`(385)
  - 标签分类：`PRESETS`(572)、`separator_label`(581)、`genres_section`(589)
  - 外观分类：`appearance_section`(881)、`appearance_select`(890)、`accent_section`(939)、`accent_swatch`(962)、`font_section`(1007)、`font_select`(1017)、`font_choice`(1068)、`font_panel`(1104)
  - 播放分类：`playback_section`(1181)
  - 歌词分类：`lyrics_section`(1202)、`lyrics_source_section`(1224)
  - 窗口分类：`window_section`(1240)
  - 弹窗框架：`Pane`(1279，`pub(super)`)、`impl Pane`（含 `GROUPS`、`all`、`title`、`glyph`）、`Section`(1340)、`SECTIONS`(1347)、`matches`、`pane_hits`、`section_hits`、`search_box`、`pane_row`、`result_row`、`group_heading`、`nav_column`、`content_column`、`dialog`(1730，`pub(crate)`)、`open`(1757，`pub(super)`)
  - `#[cfg(test)] mod tests`(1762)：`file_urls_decode`，测的是 `super::url_to_path` / `last_segment`（来自 `views/mod.rs`）。
- 文件头的 use：
  ```rust
  use std::collections::{HashMap, HashSet};
  use std::rc::Rc;
  use nsh_core::element::input;
  use nsh_core::prelude::*;
  use super::super::components::button::{Size, Variant, icon_button, outline_button, pill_button, text_button};
  use super::super::components::icons::{Icon, icon, icon_dyn};
  use super::super::store::{Same, format_total, scan, set_theme_color, store};
  use super::super::theme::*;
  use super::{SEP, add_dropped, last_segment, total_duration, url_to_path};
  use crate::config::{ThemeColor, library_store};
  ```
  注意文件里大量写成 `super::super::store::set_xxx`、`super::super::components::radio::item` —— 拆到子目录后多了一层，路径要变成 `super::super::super::...`。
- 外部引用（`src/ui/views/mod.rs`）：
  - `mod settings;`
  - `pub(super) use self::settings::dialog as settings_dialog;`
  - `settings::open(settings::Pane::Library)`（`no_library()` 里）
- 模块可见性约定（AGENTS.md）：兄弟模块之间用 `pub(super)`，不要放宽到 `pub`。

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 编译 + 类型检查 | `nsh check .` | exit 0，无 warning 新增 |
| 测试 | `nsh test .` | exit 0 |
| 列出顶层条目 | `rg -n "^(pub(\([a-z]+\))? )?(fn\|struct\|enum\|const) " src/ui/views/settings` | 见 Step 4 |

## Scope

**In scope**：
- `src/ui/views/settings.rs`（删除，内容搬走）
- `src/ui/views/settings/mod.rs`、`appearance.rs`、`library.rs`、`playback.rs`、`lyrics.rs`、`covers.rs`、`window.rs`、`tags.rs`（新建）

**Out of scope**：
- 任何文案、样式、逻辑改动——哪怕看到可以顺手改进的地方。
- `src/ui/views/mod.rs` 除非为保持编译必须（理论上 `mod settings;` 自动指向 `settings/mod.rs`，不用改）。
- `tests/ui.rs`。

## Git workflow

- 不要 commit / push；改动留在工作区。

## Steps

### Step 1：建目录，把整个文件原样移成 `settings/mod.rs`

`mkdir src/ui/views/settings && git mv src/ui/views/settings.rs src/ui/views/settings/mod.rs`

**Verify**：`nsh check .` → exit 0（`mod settings;` 自动解析到 `settings/mod.rs`，路径层级不变）。

### Step 2：按分类搬出子模块

在 `settings/mod.rs` 顶部声明子模块：

```rust
mod appearance;
mod covers;
mod library;
mod lyrics;
mod playback;
mod tags;
mod window;
```

把函数原样剪切到对应文件（函数体一个字不改，只改 use 路径与可见性）：

| 文件 | 搬入 |
|---|---|
| `library.rs` | `folders_section`、`library_section`、`watch_row`、`filter_section`、`delete_section`、`stat`（若 `stat` 只被曲库用；否则留 mod.rs） |
| `covers.rs` | `covers_source_section` |
| `tags.rs` | `PRESETS`、`separator_label`、`genres_section` |
| `appearance.rs` | `appearance_section`、`appearance_select`、`accent_section`、`accent_swatch`、`font_section`、`font_select`、`font_choice`、`font_panel` |
| `playback.rs` | `playback_section` |
| `lyrics.rs` | `lyrics_section`、`lyrics_source_section` |
| `window.rs` | `window_section` |

留在 `mod.rs`：`setting_section`、`note`、`pref_row`、`Pane` 及其 impl、`Section`、`SECTIONS`、搜索与导航（`matches` … `content_column`）、`dialog`、`open`、`mod tests`。

可见性规则：
- 子模块里被 `SECTIONS` 引用的 `*_section` 函数改为 `pub(super) fn`。
- `mod.rs` 里的共用部件（`setting_section`、`note`、`pref_row`，以及留在 mod.rs 的 `stat`）**不用改可见性**：Rust 里子模块能访问祖先模块的私有项，子模块里写 `use super::{pref_row, setting_section};` 即可。
- `SECTIONS` 里的 `build: folders_section` 改成 `build: library::folders_section` 等。
- 每个子模块开头写一行中文 `//!` 说明（例：`//! 设置 →「曲库」分类的各节。`），风格同现有模块头。
- use 路径：子模块里 `super::` 指 `settings`，`super::super::` 指 `views`，`super::super::super::` 指 `ui`。把原来的 `super::super::store::…` 改为 `super::super::super::store::…`，`super::{SEP, add_dropped, …}` 改为 `super::super::{SEP, …}`。沿用现有的相对路径写法（不要改成 `crate::ui::…`，与周围代码风格不一致）。只导入实际用到的项，不要留未使用导入。

**Verify**：`nsh check .` → exit 0，且没有新的 `unused import` / `dead_code` warning。

### Step 3：测试

**Verify**：`nsh test .` → exit 0（`file_urls_decode` 与 `tests/ui.rs` 里的 `settings_search_filters_sections`、`sidebar_navigates_between_library_views` 都通过）。

### Step 4：确认是纯搬移

**Verify**：
- `rg -c "^\s*(pub(\([a-z]+\))? )?fn " src/ui/views/settings/ | awk -F: '{s+=$2} END {print s}'` 的结果等于 f6e6821 时的函数总数：`git show f6e6821:src/ui/views/settings.rs | rg -c "^\s*(pub(\([a-z]+\))? )?fn "`。
- `wc -l src/ui/views/settings/*.rs` → 每个文件 < 900 行。
- `git diff --stat` 只涉及 `src/ui/views/settings*`。

## Test plan

不新增测试：本计划零行为变化，现有单测与 UI 测试即回归保障。

## Done criteria

- [ ] `src/ui/views/settings.rs` 不存在，`src/ui/views/settings/` 下有 8 个文件
- [ ] `nsh check .` exit 0，无新 warning
- [ ] `nsh test .` exit 0
- [ ] 函数总数与搬移前一致（Step 4）
- [ ] `git status` 只显示 `src/ui/views/settings*` 与 `plans/README.md`
- [ ] `plans/README.md` 状态行更新

## STOP conditions

- 顶层条目与「现状」列表对不上（多出或缺少函数）。
- 搬移需要改动任何函数体（除 use 路径与可见性之外）才能编译。
- `nsh test .` 有失败，且原样回退到 Step 1 状态也失败（说明是既有问题，不是搬移造成）——汇报失败用例。

## Maintenance notes

- 之后新增设置节：在对应分类文件里写 `pub(super) fn xxx_section() -> View`，再在 `mod.rs` 的 `SECTIONS` 加一条（`pane`、`title`、`keys` 搜索词、`build`）。
- 审阅重点：diff 应几乎全是「删 + 加」同样的代码块；任何文案差异都是错误。
