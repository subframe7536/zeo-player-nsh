# Plan 008：Windows 上争抢硬件媒体键

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- src/engine/audio.rs src/engine/media.rs scripts README.md AGENTS.md`
> 计划 004/005/006 对 `audio.rs` 的改动是预期内的（`Output` / `open_output` / `DeviceEvent` / `rg_factor` / EQ）；确认 004 已完成再开始。

## Status

- **Priority**: P1
- **Effort**: M（含 Windows 实机诊断）
- **Risk**: MED（根因是假设，需实机验证）
- **Depends on**: plans/004-output-device-change.md
- **Category**: bug（issue #11）
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

用户在 Windows 11 上的复现：**先在 Zeo 播放 → 暂停 → 在 AIMP 播放 → 暂停 → 回到 Zeo 播放，系统的媒体控件（音量浮层、媒体键、锁屏）仍然控制 AIMP**。用户按媒体键会去控制一个已暂停的别的播放器，体验很差。

实机诊断结论（见 Step 2）：**不是 SMTC 会话选择问题**——`GetCurrentSession` 已正确指回 `dev.subf.zeo-player`，但硬件媒体键仍被 AIMP 收走。进一步实验证明 AIMP 用低级键盘钩子（`WH_KEYBOARD_LL`）吞键：Zeo 持有 `RegisterHotKey` 注册时（外部注册返回 1409「热键已注册」、Zeo 退出后注册成功）按键依旧全归 AIMP；结束 AIMP 后同一按键立即控制 Zeo。媒体键路由与 `current=` 无关。

修复方向：**与 AIMP 在同一层争抢**——Zeo 自己装 `WH_KEYBOARD_LL`，截下 4 个媒体键转为引擎命令并吞掉；窗口激活时重挂钩子回到钩子链首（LL 钩子后装先调），实现「最后聚焦的播放器拿到键」的争抢语义。

## 假设与结论

- ~~H1（音频会话活动转变决定 current）~~：**已证伪**。暂停时停 cpal 流（`sync_stream`）后 Zeo 有完整的 inactive→active 转变，媒体键仍归 AIMP。改动保留（省电、会话如实非活动）。
- ~~H2（playwire 不刷 DisplayUpdater）~~：与症状无关，`current=` 本来就是对的。
- **结论**：媒体键被 AIMP 的 `WH_KEYBOARD_LL` 吞掉，`RegisterHotKey` 这一层争不过；应用侧修法是装自己的 LL 钩子。

## Current state

- `src/engine/media.rs`：所有 playwire 调用在专属线程串行执行；`MediaSession::publish(snapshot)` 每次都构造完整 `PlaybackState`。Windows 下 `PlayerConfig` 带 `hwnd` 与 `app_media_id("dev.subf.zeo-player")`。
- `src/engine/audio.rs`（004 之后）：`Output { stream, rate, channels, running }`，`sync_stream` 让流运行态跟随 `playing`。
- `src/window.rs`：Windows 平台段已子类化 `NshWindow`（`proc` 处理 `WM_CLOSE`/`WM_ACTIVATE` 等），`platform::start()` 在专属线程做 `hook()` 重试。
- cpal：`StreamTrait::play()` / `pause()`；windows-sys 需 `Win32_UI_Input_KeyboardAndMouse`。
- Windows 媒体控件后端：`playwire-2.0.0/src/platform/windows.rs`（只读参考，**不要修改依赖源码**）。

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 编译 | `nsh check .` | exit 0 |
| 测试 | `nsh test .` | exit 0 |
| Windows 交叉检查（在 Mac 上） | `PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH" cargo xwin check --target x86_64-pc-windows-msvc` | exit 0 |
| Windows 上运行 | `nsh run .` | 应用启动 |
| 会话探针（Windows PowerShell 5.1，不是 pwsh 7） | `powershell -ExecutionPolicy Bypass -File scripts\smtc-probe.ps1` | 每秒一行 `current=… [会话: 状态]` |

## Scope

**In scope**：`scripts/smtc-probe.ps1`（新建）、`src/engine/audio.rs`、`src/e2e.rs`、`src/window.rs`（LL 钩子）、`Cargo.toml`（windows-sys feature）、`AGENTS.md`、`README.md`（若行为对用户可见）、`plans/README.md`。

**Out of scope**：修改 playwire 源码或 vendoring；直接调用 WinRT SMTC API 另起一套媒体会话；媒体会话封面的修复（只诊断）；macOS 的 Now Playing / 媒体键行为改动（macOS 由系统分发，无此问题）。

## Git workflow

不要 commit / push（除非用户另行要求）。

## Steps

### Step 1：会话探针脚本

新建 `scripts/smtc-probe.ps1`（内容如下，原样写入）：

```powershell
# 每秒打印系统「当前媒体会话」与全部会话的播放状态（诊断 issue #11 用）。
# 用法（Windows PowerShell 5.1）：powershell -ExecutionPolicy Bypass -File scripts\smtc-probe.ps1
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager, Windows.Media.Control, ContentType = WindowsRuntime]
$asTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
  $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
} | Select-Object -First 1
function Await($op, [Type]$type) {
  $task = $asTask.MakeGenericMethod($type).Invoke($null, @($op))
  $task.Wait(-1) | Out-Null
  $task.Result
}
$manager = Await ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager])
while ($true) {
  $current = $manager.GetCurrentSession()
  $name = if ($current) { $current.SourceAppUserModelId } else { '<none>' }
  $line = (Get-Date -Format 'HH:mm:ss') + '  current=' + $name
  foreach ($s in $manager.GetSessions()) {
    $line += '  [' + $s.SourceAppUserModelId + ': ' + $s.GetPlaybackInfo().PlaybackStatus + ']'
  }
  Write-Output $line
  Start-Sleep -Seconds 1
}
```

**Verify**（Windows）：运行后每秒输出一行，Zeo 运行并播放时能看到含 `dev.subf.zeo-player` 的会话。

### Step 2：在 Windows 11 上复现并取证

需要 Windows 11 实机 + 已安装 AIMP。

1. 开着探针，执行复现步骤：Zeo 播放 → 暂停 → AIMP 播放 → 暂停 → Zeo 播放；每步间隔 ≥ 3 秒。保存探针输出为 `.nsh/tmp/smtc-baseline.txt`。
2. 每步后用 `keybd_event(VK_MEDIA_PLAY_PAUSE)` 注入媒体键，记录控制的是谁。
3. 取证实验：
   a. Zeo 用 `RegisterHotKey` 注册 4 个媒体键后，外部进程再注册同键应返回 1409；此时按键若仍归 AIMP → AIMP 的拦截层在热键分发之上（LL 钩子）。
   b. `Stop-Process AIMP` 后再按键：若改控 Zeo → 实锤 AIMP 自行吞键。

**本机结果**：`current=` 始终正确（AIMP 一停即回 `dev.subf.zeo-player`）；Zeo 持 `RegisterHotKey` 期间按键仍全归 AIMP；杀 AIMP 后按键立即控制 Zeo。

### Step 3：暂停时暂停输出流（保留的省电改进）

在 `src/engine/audio.rs`：`Output` 加 `running: bool`；`sync_stream(out, playing)` 按状态 `stream.play()`/`stream.pause()`；主循环命令处理后与队列播完分支各调一次；`handle_device` 重建/重试出的流也按当前 `playing` 同步。

**Verify**：`nsh check .` → exit 0。

### Step 4：回归测试

e2e（`src/e2e.rs`）`paused_engine_resumes_after_stream_pause`：播放 `sine.flac` → 等 `current_time > 0.5` → `pause()` → 等 1 秒记 `t1` → 再等 0.5 秒断言时间不动 → `play()` → 1 秒内 `current_time > t1 + 0.3`。结束时 `pause()`。

**Verify**：`cargo test paused_engine_resumes -- --nocapture` → 通过；`nsh test .` → exit 0。

### Step 5：争抢硬件媒体键——低级键盘钩子

在 `src/window.rs` 的 Windows 平台段：

1. `media_hook`（`WH_KEYBOARD_LL` 回调）：`HC_ACTION` + `WM_KEYDOWN`/`WM_SYSKEYDOWN` 时按 `KBDLLHOOKSTRUCT.vkCode` 分发——`VK_MEDIA_PLAY_PAUSE` → `Player::global().toggle()`、`VK_MEDIA_NEXT_TRACK` → `.next()`、`VK_MEDIA_PREV_TRACK` → `.previous()`、`VK_MEDIA_STOP` → `.stop()`；命中返回 `1` 吞键（键不会再到 AIMP 的钩子，也不会被系统转成 SMTC 命令——不会与 playwire 回调双触发）。其余键 `CallNextHookEx` 放行。
2. `media_hook_thread()`：`platform::start()` 里 spawn（线程名 `zeo-player-kbhook`），记录 `GetCurrentThreadId` 到 `HOOK_TID`，`SetWindowsHookExW(WH_KEYBOARD_LL, …)` 装钩子后跑 `GetMessageW` 消息泵；收到 `WM_REHOOK_MEDIA` 先卸再装（回到钩子链首）；退出时 `UnhookWindowsHookEx`。
3. `proc` 的 `WM_ACTIVATE`（`w != 0`，即 WA_ACTIVE / WA_CLICKACTIVE）分支 `PostThreadMessageW(HOOK_TID, WM_REHOOK_MEDIA, …)`——窗口每次被激活都把我们的钩子挪回链首。
4. `Cargo.toml` 的 windows-sys features 加 `Win32_UI_Input_KeyboardAndMouse`。

**Verify**：`nsh check .` → exit 0。

### Step 6：Windows 实机复验

重复 Step 2 的对抗场景，探针输出存 `.nsh/tmp/smtc-after.txt`：

- Zeo（新构建）运行 → AIMP 播放 → 注入媒体键：AIMP 钩子较新，键归 AIMP（预期）。
- 激活 Zeo 窗口（触发重挂）→ 再注入：键应控制 Zeo（`toggle()` 生效、AIMP 不动）。
- 杀 AIMP 后按键仍控制 Zeo；Zeo 退出后按键不再被吞（钩子随进程消失）。

**本机结果**：激活 Zeo 后注入键，`dev.subf.zeo-player` 由 Paused 切 Playing、AIMP 保持 Paused——争抢生效。

### Step 7：封面诊断（只记录，不修复）

播放有封面的曲目，按音量键弹媒体浮层，截图存 `.nsh/tmp/smtc-artwork.png`。背景：playwire 用 `CreateFromUri(file:///…)` 设封面，未打包应用可能不显示。

**本机结果**：注入音量键不弹浮层，未取证。

### Step 8：记录结论

- `AGENTS.md` Conventions：记 `sync_stream` 维护约定 + issue #11 结论（LL 钩子争抢机制、`WM_ACTIVATE` 重挂语义）。
- `README.md`：媒体键行为对用户可见（Windows 上硬件媒体键控制 Zeo），按需补一句。
- `plans/README.md`：状态改 DONE。

## Test plan

- e2e 1 个（暂停后时间不走、恢复后继续走）；Windows 实机对抗验证（探针日志 + 注入按键）。

## Done criteria

- [x] `scripts/smtc-probe.ps1`（Step 1）按内容建成并用于诊断（验证完成后按用户要求删除，不入库）
- [x] `nsh check .`、`nsh test .` exit 0
- [x] `cargo test paused_engine_resumes` 通过
- [x] `.nsh/tmp/smtc-baseline.txt` 与 `.nsh/tmp/smtc-after.txt` 关键行附在汇报
- [x] `plans/README.md` 状态行更新

## STOP conditions

- 004 未完成（没有 `Output` 结构）。
- `stream.pause()` 后 `stream.play()` 在任一平台上恢复失败（e2e 超时）。
- 任何修复需要改 playwire 源码或直接操作 SMTC（LL 钩子不碰 SMTC，不受影响）。

## Maintenance notes

- LL 钩子全局生效：Zeo 在钩子链首时会吞掉媒体键（其他播放器收不到）；`WM_ACTIVATE` 重挂使「最后聚焦者拿键」，与 AIMP 行为对等。若将来出现更合理的系统级仲裁再撤掉。
- 钩子回调有 `LowLevelHooksTimeout` 时限（默认几百 ms），`Player::global()` 的命令发送是非阻塞的，勿在回调里加等待。
- 进程退出时 Windows 自动卸钩子；`WM_DESTROY` 路径 `TerminateProcess` 不会泄漏。
- 暂停输出流后 `output_callback` 停着时基本不再被调；依赖「停着时回调仍在跑」的逻辑会失效。macOS 媒体键走系统分发 + MPRemoteCommandCenter，无此问题，不需要对应实现。
