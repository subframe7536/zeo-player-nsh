# Plan 008：Windows 上恢复播放时夺回系统媒体控件

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

**根因尚未确认**。本计划先在实机上取证，再按证据实施修复。下面的「假设」不是结论。

## 假设（按可验证性排序）

- **H1（应用侧可修，先验证）**：Zeo 的 cpal 输出流从启动起就一直在跑——暂停时只是往设备写静音（`output_callback` 在 `playing == false` 时填 0）。从 Windows 音频会话的角度看，Zeo 的会话一直是「活动」的，恢复播放时没有「非活动 → 活动」的转变；而 AIMP 暂停时会停掉自己的流、播放时重新启动。若 Windows 在挑选「当前媒体会话」时参考音频会话的活动转变，Zeo 恢复播放就抢不回来。修复：暂停（淡出结束）后暂停 cpal 流，播放前再启动——顺带降低空闲时的 CPU / 功耗。
- **H2（库内问题，应用侧无干净修法）**：playwire 2.0 的 Windows 后端在每次 `set_state` 都调用 `SetPlaybackStatus`，但 `DisplayUpdater.Update()` 只在曲目变化时调用（`playwire-2.0.0/src/platform/windows.rs` 的 `set_state`：`track_changed` 为假时不调 `apply_metadata`）。若 Windows 以 DisplayUpdater 的更新作为「最近活动」依据，恢复播放不更新它就抢不回来。这需要改 playwire（向上游提 issue / PR），**本计划不在应用里做绕法**。

## Current state

- `src/engine/media.rs`：所有 playwire 调用在专属线程串行执行；`MediaSession::publish(snapshot)` 每次都构造完整 `PlaybackState`（含 `playing`）。Windows 下 `PlayerConfig` 带 `hwnd` 与 `app_media_id("dev.subf.zeo-player")`。
- `src/engine/audio.rs`（004 之后）：`Output { stream: Option<cpal::Stream>, rate, channels }` 由 `open_output` 创建，建好后立即 `stream.play()`，之后一直运行。主循环每轮：处理命令（会打断声音的命令先 `fade_out`，淡出期间阻塞到增益归 0）→ `handle_cmd` → 若 `!playing` 则 `shared.gain` 置 0、`fade_target` 置 1 → 解码填充 → 媒体心跳。
  - `handle_cmd` 里 `Play` / `Toggle`（开始播放）/ `Load` / `Jump` / `Next` / `Previous` 会把 `shared.playing` 置 true；`Pause` / `Stop` / `Toggle`（暂停）置 false；队列播完时主循环也会置 false。
  - `output_callback`：`playing == false` 时整块填 0，不推进 `played`。
- cpal：`StreamTrait::play()` / `pause()`（`use cpal::traits::StreamTrait` 已在文件头）。
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

**In scope**：`scripts/smtc-probe.ps1`（新建）、`src/engine/audio.rs`、`src/e2e.rs`、`AGENTS.md`（记录结论）、`README.md`（仅当行为对用户可见地改变）。

**Out of scope**：修改 playwire 源码或 vendoring；在应用里直接调用 WinRT SMTC API 另起一套媒体会话；媒体会话封面的修复（只诊断，见 Step 6）；macOS 的 Now Playing 行为改动（只做回归检查）。

## Git workflow

不要 commit / push。

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

### Step 2：在 Windows 11 上复现并记录基线

需要 Windows 11 实机 + 已安装 AIMP。若没有 Windows 环境，跳到 Step 3，完成后把状态标为 `BLOCKED（等待 Windows 实机验证）` 并汇报。

1. 开着探针，执行用户的复现步骤：Zeo 播放 → 暂停 → AIMP 播放 → 暂停 → Zeo 播放；每步间隔 ≥ 3 秒。
2. 保存探针输出为 `.nsh/tmp/smtc-baseline.txt`；记录每一步后按键盘媒体键「播放/暂停」控制的是谁。
3. 预期（复现成功）：最后一步后 `current=` 仍是 AIMP，Zeo 会话状态为 `Playing`。若 `current=` 已经是 Zeo 但媒体键仍控制 AIMP，记录下来——这说明问题不在会话选择，见 STOP。

### Step 3：实施 H1——暂停时暂停输出流

在 `src/engine/audio.rs`：

1. `Output` 加字段 `running: bool`（`open_output` 建流 `play()` 成功后为 true）。
2. 新增：
   ```rust
   /// 让输出流的运行状态跟随播放状态：停着时暂停流（Windows 据音频会话
   /// 的活动判断谁是当前媒体会话，一直输出静音的流会让我们恢复播放时
   /// 抢不回系统媒体控件；也省电）。淡出已在命令处理里等完。
   fn sync_stream(out: &mut Output, playing: bool)
   ```
   `playing && !running` → `stream.play()`；`!playing && running` → `stream.pause()`；成功才改 `running`；失败只 `eprintln!`，不改状态。
3. 主循环在「处理完本轮命令」之后、「解码填充」之前调用 `sync_stream(&mut out, shared.playing.load(Relaxed))`；队列播完置 `playing = false` 的分支之后也调用一次。
4. 注意「开始播放」时流要先运行回调才会从静音淡入：`sync_stream` 在 `handle_cmd` 把 `playing` 置 true 之后、同一轮里执行即可（回调开始取样时 `gain` 从 0 推进）。
5. 004 的 `handle_device` 重建流后，按当前 `playing` 设 `running`（重建出的流若不在播放应立即 `pause()`）。

**Verify**：`nsh check .` → exit 0；交叉检查 → exit 0。

### Step 4：回归测试

1. e2e（`src/e2e.rs`，沿用 `corpus()` 跳过逻辑）`paused_engine_resumes_after_stream_pause`：播放 `sine.flac` → 等 `current_time > 0.5` → `pause()` → 等 1 秒，记录 `t1 = current_time` → 再等 0.5 秒断言 `current_time == t1`（暂停时不走）→ `play()` → 1 秒内 `current_time > t1 + 0.3`（流重新启动后继续走）。结束时 `pause()`。
2. 启动恢复场景：现有 `restore_last` 走 `Cue`（不播放），确认 `nsh test .` 里涉及「恢复上次播放后按播放键」的用例仍通过。

**Verify**：`cargo test paused_engine_resumes -- --nocapture` → 通过；`nsh test .` → exit 0。

### Step 5：Windows 实机复验

重复 Step 2，输出存为 `.nsh/tmp/smtc-after.txt`。

- 若最后一步后 `current=` 变为 Zeo、媒体键控制 Zeo → H1 成立，进入 Step 7。
- 若仍是 AIMP → H1 不成立。**保留 Step 3 的改动**（省电、无副作用），停止，按 STOP 条件汇报两份探针日志；后续是否向 playwire 提 H2 的修复由审阅者决定。

### Step 6：封面诊断（只记录，不修复）

在 Windows 上播放一首有封面的曲目，按音量键弹出媒体浮层，截图存 `.nsh/tmp/smtc-artwork.png`，记录浮层是否显示封面。背景：playwire 的 Windows 后端用 `RandomAccessStreamReference::CreateFromUri(file:///…)` 加载封面，其源码注释只考虑过 https 远程 URL；未打包（非 MSIX）的 Win32 应用用 `file://` 可能不显示。结果写进汇报。

### Step 7：记录结论

- `AGENTS.md` 的 Conventions 末尾加一条中文说明（只写已验证的结论）：引擎停着时暂停 cpal 输出流（`sync_stream`），原因是 Windows 据此判断当前媒体会话；改动输出流生命周期时保持这一点。
- 若 Step 6 确认 Windows 不显示封面，在 `AGENTS.md`「待上游（NSH）」之后另起一小节「待上游（其它依赖）」记一条：playwire Windows 后端用 `CreateFromUri(file://)` 设封面，未打包应用不显示；希望改为 `StorageFile` + `CreateFromFile`。
- README 不需要改（用户可见行为是「修好了」，不是新功能）。

## Test plan

- e2e 1 个（暂停后时间不走、恢复后继续走）；Windows 实机前后对比（探针日志）。

## Done criteria

- [ ] `scripts/smtc-probe.ps1` 存在且内容与 Step 1 一致
- [ ] `nsh check .`、`nsh test .` exit 0；交叉检查 exit 0
- [ ] `cargo test paused_engine_resumes` 通过
- [ ] 汇报里附 `.nsh/tmp/smtc-baseline.txt` 与 `.nsh/tmp/smtc-after.txt` 的关键行（或注明 BLOCKED：无 Windows 环境）
- [ ] `plans/README.md` 状态行更新（DONE / BLOCKED，附一句原因）

## STOP conditions

- 004 未完成（没有 `Output` 结构）。
- Step 2 中 `current=` 已是 Zeo 但媒体键仍控制 AIMP（问题不在会话选择，本计划的假设都不适用）。
- Step 5 显示 H1 不成立。
- `stream.pause()` 后 `stream.play()` 在任一平台上恢复失败（e2e 超时）。
- 任何修复需要改 playwire 源码或在应用里直接操作 SMTC。

## Maintenance notes

- 暂停输出流后，`output_callback` 在停着时基本不再被调用；依赖「停着时回调仍在跑」的逻辑（如果以后有）会失效。
- macOS 上暂停 AudioUnit 也没问题，且减少空闲 CPU；但若以后发现 macOS Now Playing 在暂停期间被系统回收，可以考虑只在 Windows 上暂停流（`cfg`）。
