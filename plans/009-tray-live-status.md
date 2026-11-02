# Plan 009：托盘实时显示播放状态

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- src/window.rs src/ui/store.rs src/config/mod.rs src/ui/views/settings README.md`
> 计划 001/002/004/005/006 对 `store.rs`、`config/mod.rs`、`settings/` 的改动是预期内的；核对下文 `window.rs` 与 `store.rs` 托盘相关摘录。

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Depends on**: plans/001-split-settings.md
- **Category**: direction（issue #4，仅「播放状态实时显示」；用户已决定不做信息 / 控制小窗）
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

窗口最小化到托盘后，现在只能点开菜单才看到曲名；托盘图标的悬停提示固定是「Zeo Player」。issue #4 要求托盘实时显示播放状态。本计划：悬停提示显示曲名、歌手、播放 / 暂停与进度；菜单里多一行专辑与进度；macOS 可选在菜单栏图标旁显示曲名（Windows 通知区不能显示文字，靠悬停提示，这是平台差异）。

## Current state

- `src/window.rs`：
  - `TrayState`（121 行）：
    ```rust
    #[derive(Debug, Clone, Default, PartialEq)]
    pub struct TrayState { pub title: String, pub playing: bool, pub has_track: bool }
    ```
  - `pub fn update_tray(state: TrayState) { platform::update(state); }`（223 行）。macOS 的 `platform::update` 用 `DispatchQueue::main().exec_async(move || super::tray::apply(&state))`；Windows 经 `TRAY_TX` 发 `TrayMsg::State(state)` 到托盘线程，再 `super::tray::apply(&state)`。
  - `mod tray`（247–356 行，两平台共用）：`struct Tray { _icon: TrayIcon, now, toggle, prev, next: MenuItem }`，`create()` 建菜单 `now（禁用） / 分隔 / 播放 / 上一首 / 下一首 / 分隔 / 显示 Zeo Player / 退出 Zeo Player`，`TrayIconBuilder::new().with_menu(..).with_tooltip("Zeo Player")`；`apply(state)` 改 `now` 文本（无曲目时 `未在播放`）、`toggle` 文本（`暂停`/`播放`）、三个按钮的启用状态。
- `src/ui/store.rs` `install()` 里（约 328–342 行）：
  ```rust
  // 托盘菜单跟着当前曲目与播放状态。
  effect(move || {
      let state = crate::window::TrayState {
          title: s.track.with(|t| if t.artist.is_empty() { t.title.clone() } else { format!("{} — {}", t.title, t.artist) }),
          playing: s.playback.with(|p| p.playing),
          has_track: s.track.with(|t| t.path.is_some()),
      };
      crate::window::update_tray(state);
  });
  ```
  `s.playback` 每 100ms 轮询更新（`time` 变化就通知），但现在的 effect 没读 `time`，所以只在曲目 / 播放状态变化时触发。窗口隐藏到托盘时只拆界面，这些 effect 仍在运行。
- `s.track`（`NowPlaying`）有 `title`、`artist`、`album`、`path`；`s.playback` 有 `time`、`duration`、`playing`。`store::format_time(secs) -> String`（`m:ss`）。
- tray-icon 0.26.1 的 `TrayIcon` 有 `set_tooltip(Option<S>) -> Result<()>` 与 `set_title(Option<S>)`（后者只在 macOS 有效，在菜单栏图标旁显示文字）。
- 设置：001 之后窗口分类在 `src/ui/views/settings/window.rs`（`window_section`），开关行 `pref_row(title, desc, checked, set: fn(bool))`；配置 / 状态 / setter 模式见 `remember_frame`（`config/mod.rs`、`store.rs` 的 `set_remember_frame`）。

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 编译 | `nsh check .` | exit 0 |
| 单测 | `cargo test tray_` | 新用例通过 |
| 全部测试 | `nsh test .` | exit 0 |
| Windows 交叉检查 | `PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH" cargo xwin check --target x86_64-pc-windows-msvc` | exit 0 |

## Scope

**In scope**：`src/window.rs`（`TrayState`、`mod tray`）、`src/ui/store.rs`（托盘 effect 与新 setter）、`src/config/mod.rs`、`src/ui/views/settings/window.rs`、`src/ui/views/settings/mod.rs`（`SECTIONS` 的 keys）、`README.md`。

**Out of scope**：托盘弹出的信息 / 控制窗口；托盘图标随状态换图；托盘菜单的新操作项（喜欢、循环模式等）；`platform` 模块的线程模型。

## Git workflow

不要 commit / push。

## Steps

### Step 1：扩展 `TrayState` 与文案生成（纯函数）

```rust
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrayState {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub has_track: bool,
    /// 位置与时长（整秒，`PartialEq` 去重用；不到一秒的变化不更新托盘）。
    pub position: u32,
    pub duration: u32,
    /// macOS：在菜单栏图标旁显示曲名。
    pub show_title: bool,
}
```

在 `mod tray` 外（两平台都编译、可单测）加纯函数：

```rust
/// 悬停提示：`曲名 — 歌手\n▶ 1:23 / 4:56`（暂停为 ❚❚）；无曲目为 `Zeo Player`。
fn tray_tooltip(s: &TrayState) -> String
/// 菜单里的专辑 · 进度行：`专辑 · 1:23 / 4:56`（专辑为空只显示进度）。
fn tray_detail(s: &TrayState) -> String
/// 菜单栏文字（macOS）：`曲名 — 歌手`，超过 32 个字符截断加 `…`；关闭或无曲目时 `None`。
fn tray_title(s: &TrayState) -> Option<String>
```

时间格式与界面一致（`m:ss`，≥1 小时 `h:mm:ss`）——不要依赖 `ui::store::format_time`（`window` 不应依赖 `ui`），在 `window.rs` 写一个私有 `fn clock(secs: u32) -> String`。截断按 `chars()` 计数，不要按字节切（中文）。

**Verify**：`nsh check .` → exit 0。

### Step 2：单测

`window.rs` 加 `#[cfg(test)] mod tests`：
- `tray_tooltip_formats_playing_and_paused`：播放 / 暂停两种状态的完整字符串；无曲目返回 `Zeo Player`；歌手为空时没有 ` — `。
- `tray_detail_omits_empty_album`。
- `tray_title_truncates_by_chars`：40 个汉字的标题截成 32 个字符 + `…`；`show_title == false` 返回 `None`。
- `tray_clock_formats_hours`：`clock(83) == "1:23"`、`clock(3725) == "1:02:05"`。

**Verify**：`cargo test tray_` → 4 passed。

### Step 3：菜单与提示

`mod tray`：
1. `Tray` 加 `detail: MenuItem`（禁用），菜单顺序：`now` / `detail` / 分隔 / … 其余不变；`Tray` 里同时保存 `TrayIcon` 的可用引用（把 `_icon` 改名 `icon` 并使用）。
2. `apply(state)`：
   - `now.set_text`：有曲目时 `title — artist`（歌手空则只有曲名），否则 `未在播放`。
   - `detail.set_text(tray_detail(state))`，无曲目时文本为空并 `set_enabled(false)`；（若 tray-icon 不支持隐藏菜单项，空文本即可）。
   - `icon.set_tooltip(Some(tray_tooltip(state)))`（错误忽略）。
   - `#[cfg(target_os = "macos")] icon.set_title(tray_title(state))`。
   - `toggle` / 启用状态逻辑不变。
3. 去重：`apply` 开头若 `state` 与上次应用的相同（在 `thread_local` 里存一份 `RefCell<Option<TrayState>>`）直接返回。

**Verify**：`nsh check .` → exit 0；交叉检查 → exit 0。

### Step 4：store 的 effect 与设置

1. `LibraryConfig` 加 `#[serde(default)] pub tray_title: bool`（默认 false）+ getter/setter；`LibraryState` 加字段；`store.rs` 加 `pub fn set_tray_title(on: bool)`（信号 + 后台保存）。
2. 托盘 effect 补字段：`artist`、`album` 从 `s.track` 取；`position: s.playback.with(|p| p.time.max(0.0) as u32)`、`duration: s.playback.with(|p| p.duration.max(0.0) as u32)`；`show_title: s.library.with(|l| l.tray_title)`。因为 `position` 取整秒且 `TrayState: PartialEq`，effect 虽每 100ms 运行，但平台层去重后每秒最多更新一次托盘。为避免每 100ms 都跨线程投递，在 effect 里也保存上次发送的 `TrayState`（`Rc<RefCell<Option<TrayState>>>`），相同就不调用 `update_tray`。
3. `settings/window.rs` 的 `window_section` 末尾加（仅 macOS 显示，`cfg!(target_os = "macos")`）：
   `pref_row("在菜单栏显示当前曲目", "在菜单栏的 Zeo Player 图标旁显示正在播放的曲名。", …, set_tray_title)`。
   `SECTIONS` 里「关闭窗口时」那节的 `keys` 追加 `菜单栏 曲名 托盘提示`。

**Verify**：`nsh check .` → exit 0；`nsh test .` → exit 0。

### Step 5：手动验证（汇报结果）

`nsh run .`，播放后关闭窗口到托盘：
- 悬停托盘图标：显示曲名 — 歌手与进度，进度每秒更新（macOS 的提示可能要移开再悬停才刷新，这是系统行为）。
- 打开托盘菜单：第二行显示专辑与进度。
- macOS 打开设置项后，菜单栏图标旁出现曲名；关掉后消失。

### Step 6：README

「托盘」一节第二条改为：`- 最小化后播放继续；托盘图标的悬停提示显示当前曲目、播放状态与进度，菜单里有专辑与进度，可播放 / 暂停、上一首 / 下一首、打开窗口、退出；macOS 上从 Dock 点开也恢复窗口，还可在设置 →「窗口」里让菜单栏图标旁显示曲名。`

## Test plan

- 纯函数单测 4 个（Step 2）；托盘本身靠 Step 5 手动验证（headless 测试里没有托盘）。

## Done criteria

- [ ] `cargo test tray_` 4 passed；`nsh check .`、`nsh test .` exit 0；交叉检查 exit 0
- [ ] `rg -n 'with_tooltip\("Zeo Player"\)' src/window.rs` 仍在（初始值），且 `apply` 里调用了 `set_tooltip`
- [ ] README 已更新；`plans/README.md` 状态行更新

## STOP conditions

- tray-icon 0.26.1 的 `set_tooltip` / `set_title` 签名与描述不符，且无等价 API。
- 每秒更新托盘在 Windows 托盘线程上导致菜单打开时闪烁或关闭（手动验证发现）——汇报，可能需要改成菜单打开时不更新。

## Maintenance notes

- 去重有两层（store effect、平台 `apply`），改 `TrayState` 字段时注意 `PartialEq` 语义：只放「变化需要刷新托盘」的字段，位置保持整秒精度。
- 013（i18n）会把这里的文案（`未在播放`、`播放`、`暂停` 等）迁到 `tr!`；托盘文案在 `window.rs`，不在 `ui/` 下，迁移时别漏。
