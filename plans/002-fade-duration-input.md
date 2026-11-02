# Plan 002：淡入淡出时长改为数字输入框（毫秒）

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- src/ui/views/settings src/ui/views/settings.rs src/ui/store.rs src/engine/mod.rs tests/ui.rs README.md`
> 计划 001 已把 `settings.rs` 拆成 `settings/` 目录，这是预期内的改动；其余文件若改过，先核对下文摘录。

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: plans/001-split-settings.md
- **Category**: direction（issue #7）
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

用户要求（issue #7）：淡入淡出时长从 6 个固定档（关闭 / 0.1 / 0.2 / 0.4 / 0.8 / 1.5 秒）改成可以自由输入的数字框。引擎本来就支持任意毫秒值（上限 5000），只是界面限制了选项。

## Current state

- 播放设置节（f6e6821 时在 `src/ui/views/settings.rs:1180-1199`，001 之后在 `src/ui/views/settings/playback.rs`）：
  ```rust
  /// 播放：淡入淡出时长。
  fn playback_section() -> View {
      let s = store();
      const STEPS: &[(u32, &str)] = &[
          (0, "关闭"), (100, "0.1 秒"), (200, "0.2 秒"), (400, "0.4 秒"), (800, "0.8 秒"), (1500, "1.5 秒"),
      ];
      setting_section(
          "淡入淡出",
          "播放、暂停、拖动进度和切歌时，音量在这段时间里渐变，不会突然响起或戛然而止。一首自然播完接下一首时不淡变。",
          None,
      )
      .child(super::super::components::radio::group_row().children(STEPS.iter().map(|&(ms, label)| {
          super::super::components::radio::item(move || s.library.with(|l| l.fade_ms == ms), label, move || super::super::store::set_fade_ms(ms))
      })))
  }
  ```
- 保存入口 `src/ui/store.rs:605`：
  ```rust
  pub fn set_fade_ms(ms: u32) {
      store().library.update(|l| l.fade_ms = ms);
      player().set_fade_ms(ms);
      std::thread::spawn(move || library_store().set_fade_ms(ms));
  }
  ```
- 引擎 `src/engine/mod.rs:179`：`set_fade_ms` 把值 `min(5000)` 后写入原子量。默认值 `DEFAULT_FADE_MS = 200`（`src/engine/audio.rs:25`）。
- 现成的数字输入写法（`src/ui/views/playlists.rs:606-617`，智能歌单「数量上限」）：
  ```rust
  let limit = input_field(true)
      .w(px(80.0))
      .label("数量上限")
      .keyboard_type(KeyboardType::Number)
      .default_value(untracked(|| rule(&|r| r.limit.map(|n| n.to_string()).unwrap_or_default())))
      .placeholder("不限")
      .on_change(move |t: &str| { /* parse */ });
  ```
  `input_field` 在 `src/ui/components/form.rs`（shadcn Input 外观，`small=true` 为 32pt 高）。`Input` 还有 `.on_submit(|t: &str| ..)`（回车）与 `.on_blur(|| ..)`（见 `src/ui/frame/sidebar.rs:103`）。
- 单行 `text()` 约定：一律 `.line_height(px(字号))`。
- UI 测试工具（`nsh_testing::Headless`）有 `clear(selector)`、`type_text(selector, text)`（文本里的 `\n` 触发 submit）、`submit(selector)`；选择器 `label:<无障碍标签>`。

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 编译 | `nsh check .` | exit 0 |
| 测试 | `nsh test .` | exit 0 |
| 只跑新 UI 测试 | `cargo test --test ui fade_duration` | 1 passed |

## Scope

**In scope**：`src/ui/views/settings/playback.rs`、`src/ui/views/settings/mod.rs`（仅 `SECTIONS` 里「淡入淡出」的 `keys`）、`src/ui/store.rs`（仅 `set_fade_ms` 的取值钳制）、`tests/ui.rs`、`README.md`。

**Out of scope**：引擎淡变算法（`fade_step` / `fade_curve`）、默认值、`components/radio.rs`。

## Git workflow

不要 commit / push。

## Steps

### Step 1：替换单选组为数字输入

在 `playback_section` 里去掉 `STEPS` 与 radio 组，改为一行：说明文字左侧 + 右侧「输入框 + `毫秒` 单位文字」。行为：

- `input_field(true).w(px(88.0)).label("淡入淡出时长").keyboard_type(KeyboardType::Number)`
- 本地草稿 `let draft = signal(当前 fade_ms.to_string())`，`.value(move || draft.get())`，`on_change` 只更新草稿（不保存，避免每个按键写一次 `library.json`）。
- 提交（`on_submit` 与 `on_blur` 都触发同一个 `commit`）：`draft.trim().parse::<u32>()`；成功则 `min(5000)` 后调 `store::set_fade_ms(v)` 并把草稿改成规范化的值（如输入 `9999` → 草稿变 `5000`）；解析失败（空串、负号、字母）则草稿回退为当前 `s.library` 里的 `fade_ms`。
- 输入框下方或说明文字末尾加一句提示：`0 为关闭，最大 5000 毫秒。`
- 外部改了 `fade_ms`（例如另一个实例或测试）时草稿同步：`effect(move || { let ms = s.library.with(|l| l.fade_ms); draft.set(ms.to_string()); })` —— 只在输入框未聚焦时同步即可；若实现聚焦判断复杂，直接同步也可接受（提交后值相同，不会打断输入）。

`SECTIONS` 里「淡入淡出」的 `keys` 追加 `毫秒 时长`。

**Verify**：`nsh check .` → exit 0。

### Step 2：store 侧钳制

`store::set_fade_ms` 第一行加 `let ms = ms.min(5000);`（与引擎上限一致，避免 `library.json` 存进大于 5000 的值）。

**Verify**：`nsh check .` → exit 0。

### Step 3：UI 测试

在 `tests/ui.rs` 新增 `fade_duration_input_saves_clamped_value`，参照现有的 `settings_search_filters_sections`（打开设置：`app.press("设置")`，再 `app.press("播放")` 进分类）：

1. `app.clear("label:淡入淡出时长")`，`app.type_text("label:淡入淡出时长", "750\n")`。
2. 断言保存值为 750。`config` 模块在 `src/lib.rs` 里是私有的，测试拿不到 `library_store()`；改为读 `isolate()` 设好的数据目录里的配置文件：写一个小助手 `fn saved_fade_ms() -> u64`，读 `std::env::var("ZEO_PLAYER_DATA_DIR")` 下的 `library.json`，`serde_json` 取 `fade_ms` 字段；保存在后台线程进行，所以用最多 2 秒的轮询（每 50ms 读一次）等到期望值。
3. `clear` 后输入 `99999\n` → 值为 `5000`。
4. `clear` 后输入 `abc\n` → 值仍为 `5000`。
5. 用例结束前把值改回 `200`（共享全局配置，避免影响其它用例）。

**Verify**：`cargo test --test ui fade_duration` → 1 passed；`nsh test .` → exit 0。

### Step 4：README

`README.md`「播放」一节里「淡入淡出（设置里选 关闭 / 0.1–1.5 秒，默认 0.2 秒）」改为「淡入淡出（设置里输入 0–5000 毫秒，0 为关闭，默认 200 毫秒）」。

**Verify**：`rg -n "0.1–1.5" README.md` → 无输出。

## Test plan

- 新 UI 测试 1 个（Step 3），覆盖：正常值、超上限钳制、非法输入回退。

## Done criteria

- [ ] `rg -n "STEPS" src/ui/views/settings/playback.rs` → 无输出
- [ ] `nsh check .`、`nsh test .` exit 0
- [ ] `cargo test --test ui fade_duration` 通过
- [ ] README 已更新
- [ ] `plans/README.md` 状态行更新

## STOP conditions

- `src/ui/views/settings/playback.rs` 不存在（计划 001 未完成）。
- `Input` 没有 `.value(...)` 或 `.on_blur(...)`，且无法用 `default_value` + `on_submit` 实现等价行为。
- UI 测试里 `clear` / `type_text` 对该输入框无效（报找不到选择器）且改用 `id:` 选择器也不行。

## Maintenance notes

- 004/005/006 会往同一个 `playback.rs` 加节；保持每节一个 `pub(super) fn`。
- 数字输入的「草稿 + 提交时校验」写法后续 005（预放大 dB）、010（统计阈值）会复用；如果第三次复用时觉得重复，可抽成 `components::form::number_input`，但不在本计划做。
