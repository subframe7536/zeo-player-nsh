# Plan 004：输出设备变化时重建输出流，并可选择自动暂停

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- src/engine/audio.rs src/engine/mod.rs src/config/mod.rs src/ui/store.rs src/ui/views/settings README.md`
> 有输出时先核对下文摘录；对不上即 STOP。（计划 001/002 对 `settings/`、`store.rs` 的改动是预期内的。）

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED（改音频引擎线程的主循环）
- **Depends on**: plans/001-split-settings.md
- **Category**: direction + bug（issue #7「输出设备变更的时候是否暂停」）
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

现在输出流只在引擎线程启动时打开一次，之后设备的任何变化都只打一行日志。实际后果：

- **Windows**：拔耳机 / 换默认设备后，cpal 0.18 的 WASAPI 后端报 `StreamInvalidated`（流必须重建），我们不重建 → 声音直接没了，进度不走，直到重启应用。
- **macOS**：默认设备变了 cpal 会自动改道并报 `DeviceChanged`；没有可用设备时暂停流并报 `DeviceNotAvailable`。我们都不处理。

用户还要求一个设置：「输出设备变化时暂停」（类似手机拔耳机暂停，避免外放）。本计划同时做两件事：设备事件进引擎命令队列、需要时重建流；按设置决定是否暂停。

## Current state

- `src/engine/audio.rs:441` 引擎线程入口，输出流在这里一次性打开：
  ```rust
  fn run(rx: std::sync::mpsc::Receiver<EngineCmd>, shared: Arc<Shared>, media: MediaSession) {
      // 输出流：默认设备 + 默认配置（f32）。
      let mut dev_rate: u32 = 48_000;
      let mut dev_channels: usize = 2;
      let mut _stream = None;
      match cpal::default_host().default_output_device() {
          Some(device) => match device.default_output_config() {
              Ok(config) => {
                  dev_rate = config.sample_rate();
                  dev_channels = config.channels() as usize;
                  let stream_config: cpal::StreamConfig = config.into();
                  let build = device.build_output_stream(
                      stream_config,
                      { let shared = shared.clone(); let dev_channels = dev_channels;
                        move |data: &mut [f32], _| output_callback(&shared, data, dev_channels) },
                      |err| eprintln!("zeo-player cpal: {err}"),
                      None,
                  );
                  match build { Ok(stream) => { stream.play().ok(); _stream = Some(stream); }
                                Err(err) => set_error(&shared, &format!("音频输出打开失败: {err}")) }
              }
              Err(err) => set_error(&shared, &format!("音频设备读取失败: {err}")),
          },
          None => set_error(&shared, "没有可用的音频输出设备"),
      }
      shared.dev_rate.store(dev_rate, Ordering::Relaxed);
      shared.ring.init(dev_rate as usize * RING_CAPACITY_SECS * dev_channels);
      let target = ring_target(dev_rate, dev_channels);
      let has_output = _stream.is_some();
      raise_thread_priority();
      // … 主循环：命令 → 解码填充 → 媒体心跳 → 等待
  }
  ```
  `dev_rate`、`dev_channels`、`target`、`has_output` 之后全程不变，并作为参数传给 `handle_cmd`、`load_track`、`fill_ring`、`fade_out`、`reset_ring`。
- `EngineHandle::start()`（`audio.rs:301`）创建 `(tx, rx)` 通道，`MediaSession::start(tx.clone())`，再 `run(rx, shared, media)`——`run` 拿不到 `tx`。
- `EngineCmd`（`audio.rs:164`）：`Load`/`Cue`/`Jump`/`Play`/`Pause`/`Toggle`/`Stop`/`Seek`/`SeekBy`/`SetVolume`/`Next`/`Previous`/`SetLoopMode`。主循环里「会打断声音的命令先淡出」：
  ```rust
  let interrupts = match &cmd { EngineCmd::Load{..} | … | EngineCmd::Pause => true, EngineCmd::Toggle => playing, _ => false };
  if interrupts { fade_out(&shared, &mut loaded, target, dev_channels); }
  handle_cmd(&shared, &media, &mut loaded, &mut base_queue, cmd, dev_rate, dev_channels);
  ```
- 环形缓冲 `Ring`（`audio.rs:41`）：`slots: OnceLock<Box<[AtomicU32]>>`，`init` **只有第一次生效**；`write`/`read`/`discard` 是累计样本数，取模容量定位，推入时按整帧截断。容量 = `dev_rate * RING_CAPACITY_SECS(3) * dev_channels`；目标 `ring_target = dev_rate * RING_TARGET_SECS(2) * dev_channels`。
- 位置：`shared.played`（输出帧数，含 seek 基线）/ `shared.dev_rate` = 秒；`reset_ring(shared, seconds, dev_rate)` 清缓冲并把基线设成 `seconds * dev_rate`。
- 每首曲目的重采样器在 `load_track` 里按 `(src_rate, dev_rate, dev_channels)` 建（`ResamplerState::new`），`Loaded { source, resampler, carry, out, backlog, src_channels, drain, cover_pending }`；`Loaded::reset_buffers()` 清暂存并重置重采样器。
- cpal 0.18.2 的错误：回调参数是 `cpal::Error`，用 `.kind()` 取 `cpal::ErrorKind`（`#[non_exhaustive]`）。相关变体：
  - `DeviceChanged`：流已自动改道到新默认设备，**不需要重建**（macOS 默认设备流）。
  - `StreamInvalidated`：流配置失效，**必须重建**（Windows 换默认设备时报这个）。
  - `DeviceNotAvailable`：设备没了且没有替代（两端都可能）。
- 配置：`LibraryConfig`（`src/config/mod.rs:133`），每个字段 `#[serde(default…)]`；getter/setter 在 `impl LibraryStore`（模式见 `fade_ms` / `set_fade_ms`，`config/mod.rs:413-421`）。界面状态 `LibraryState`（`src/ui/store.rs:72` 起）+ `library_state()`（约 102 行）+ `store::set_xxx`（模式见 `set_delete_permanently`，约 625 行）。启动时把设置推给引擎在 `store::install()` 末尾（`player().set_fade_ms(library_store().fade_ms());`，约 484 行）。
- 设置 UI：001 之后 `src/ui/views/settings/playback.rs`；开关行用 `mod.rs` 里的 `pref_row(title, desc, checked, set: fn(bool))`。

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 测试语料 | `sh scripts/make-test-music.sh` | 打印 wrote … |
| 编译 | `nsh check .` | exit 0 |
| 单测 | `cargo test device_` | 新增用例全部通过 |
| 全部测试 | `nsh test .` | exit 0 |
| Windows 交叉检查 | `PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH" cargo xwin check --target x86_64-pc-windows-msvc` | exit 0 |

## Scope

**In scope**：`src/engine/audio.rs`、`src/engine/mod.rs`、`src/config/mod.rs`、`src/ui/store.rs`、`src/ui/views/settings/playback.rs`、`src/ui/views/settings/mod.rs`（`SECTIONS` 加一条）、`src/e2e.rs`、`README.md`。

**Out of scope**：选择指定输出设备（只跟随系统默认）；独占模式；`media.rs`；淡变算法；`Ring` 的无锁结构（不改成可替换的存储）。

## Git workflow

不要 commit / push。

## Steps

### Step 1：设备事件进命令队列

1. 新增：
   ```rust
   /// 输出设备的变化（cpal 错误回调转来）。
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub enum DeviceEvent {
       /// 已自动改道到新的默认设备，流照常（macOS）。
       Rerouted,
       /// 流失效，要在新的默认设备上重建（Windows 换默认设备）。
       Invalidated,
       /// 没有可用的输出设备。
       Lost,
   }
   ```
   `EngineCmd` 加 `Device(DeviceEvent)`。
2. `run` 的签名加 `tx: std::sync::mpsc::Sender<EngineCmd>`，`EngineHandle::start` 传 `tx.clone()`。
3. 错误回调：
   ```rust
   move |err: cpal::Error| {
       eprintln!("zeo-player cpal: {err}");
       let event = match err.kind() {
           cpal::ErrorKind::DeviceChanged => DeviceEvent::Rerouted,
           cpal::ErrorKind::StreamInvalidated => DeviceEvent::Invalidated,
           cpal::ErrorKind::DeviceNotAvailable => DeviceEvent::Lost,
           _ => return,
       };
       let _ = tx.send(EngineCmd::Device(event));
   }
   ```
   （若 cpal 0.18.2 的回调参数类型 / `kind()` 名字与此不同，按实际 API 调整，语义不变。）

**Verify**：`nsh check .` → exit 0。

### Step 2：把输出流抽成可重建的结构

在 `audio.rs` 新增：

```rust
/// 当前输出：流 + 设备格式。`stream` 为 `None` 时没有可用设备。
struct Output {
    stream: Option<cpal::Stream>,
    rate: u32,
    channels: usize,
}

/// 在系统默认设备上打开输出流（失败时 `stream: None`，格式沿用 `fallback`）。
fn open_output(shared: &Arc<Shared>, tx: &Sender<EngineCmd>, fallback: (u32, usize)) -> Output
```

把 `run` 开头那段打开逻辑原样搬进 `open_output`（错误仍用 `set_error` 报），`run` 里改成 `let mut out = open_output(&shared, &tx, (48_000, 2));`。

主循环里原来的 `dev_rate`/`dev_channels`/`target`/`has_output` 改从 `out` 取：`out.rate`、`out.channels`、`target_for(&out)`、`out.stream.is_some()`。

**环形缓冲容量**：`Ring::init` 只能调一次。第一次打开后按 `max(本次需要, 48_000 * RING_CAPACITY_SECS * 2)` 分配。新增
```rust
/// 填充目标：设备格式算出的目标，但不超过容量的 2/3（换到更高采样率 /
/// 更多声道的设备后容量不变，只是缓冲的秒数变少）。
fn target_for(out: &Output, capacity: usize) -> usize
```
给 `Ring` 加 `fn capacity(&self) -> usize`（`slots.get().map_or(0, |s| s.len())`）。缓冲是累计计数 + 取模，换声道数不影响正确性（推入按整帧截断）。

**Verify**：`nsh check .` → exit 0；`nsh test .` → exit 0（行为与改前相同）。

### Step 3：处理设备事件

1. `Shared` 加 `pub pause_on_device_change: AtomicBool`（默认 `true`），`Player` 加 `pub fn set_pause_on_device_change(&self, on: bool)`（直接写原子量，模式同 `set_fade_ms`）。
2. 主循环里，`EngineCmd::Device(ev)` **不走** `interrupts`（设备已经变了，淡出没有意义），在调用 `handle_cmd` 之前单独处理：
   ```rust
   EngineCmd::Device(ev) => handle_device(ev, &shared, &media, &tx, &mut out, &mut loaded),
   ```
3. 决策逻辑抽成纯函数，便于单测：
   ```rust
   /// 设备事件之后：要不要重建流、要不要暂停。
   fn device_plan(ev: DeviceEvent, playing: bool, pause_pref: bool) -> (bool /*rebuild*/, bool /*pause*/) {
       let rebuild = matches!(ev, DeviceEvent::Invalidated | DeviceEvent::Lost);
       let pause = playing && (pause_pref || ev == DeviceEvent::Lost);
       (rebuild, pause)
   }
   ```
4. `handle_device`：
   - 按 `device_plan` 先暂停（`shared.playing.store(false)`、`shared.gain` 置 0、`publish_media(…, true)`）。
   - 需要重建时：记下当前秒数 `let at = shared.current_time();`，`out.stream = None`（先 drop 旧流），`out = open_output(&shared, &tx, (out.rate, out.channels))`。
     - 拿到流：`shared.dev_rate.store(out.rate)`；若有 `loaded`，按新格式重建重采样器（`src_rate != out.rate` 时 `ResamplerState::new(src_rate, out.rate, out.channels)`，否则 `None`），`l.reset_buffers()`，`l.source.seek(at)` 后 `reset_ring(&shared, actual, out.rate)`；没有 `loaded` 时只 `reset_ring(&shared, at, out.rate)`。
     - 拿不到流（`Lost` 且系统里确实没设备）：记一个 `retry_at = Instant::now() + 1s`，主循环每轮检查到期就再 `open_output`，成功后按上一条恢复；**不自动恢复播放**（Lost 时一定已暂停）。
   - `Rerouted`：不重建，只按决策暂停或继续。
5. `load_track` / `fill_ring` / `convert_channels` 等现有函数的 `dev_rate`/`dev_channels` 参数改为传 `out.rate`/`out.channels`（签名不变，调用处取值变了）。

**Verify**：`nsh check .` → exit 0。

### Step 4：设置项

1. `LibraryConfig` 加：
   ```rust
   /// 输出设备变化（拔耳机、换默认设备）时暂停播放。
   #[serde(default = "default_true")]
   pub pause_on_device_change: bool,
   ```
   `Default` / `normalized()` 若逐字段列出也要补（照 `delete_permanently` 的位置抄）。`LibraryStore` 加 getter/setter。
2. `LibraryState` 加同名字段，`library_state()` 填；`store.rs` 加 `pub fn set_pause_on_device_change(on: bool)`（更新信号 + `player().set_pause_on_device_change(on)` + 后台线程保存，同 `set_fade_ms` 模式）；`install()` 末尾 `player().set_pause_on_device_change(library_store().pause_on_device_change());`。
3. `settings/playback.rs` 新增 `pub(super) fn device_section() -> View`：
   `setting_section("输出设备", "拔下耳机或切换系统默认输出设备时的行为。", None).child(pref_row("设备变化时暂停", "关闭后会在新的默认设备上继续播放。设备全部断开时总会暂停。", checked, set_pause_on_device_change))`。
   `SECTIONS` 加 `{ pane: Pane::Playback, title: "输出设备", keys: "输出 设备 耳机 拔出 暂停 默认设备 声卡 蓝牙", build: playback::device_section }`。

**Verify**：`nsh check .` → exit 0。

### Step 5：测试

1. 单测（`audio.rs` 的 `mod tests`）`device_plan_decides_rebuild_and_pause`：覆盖 3 个事件 × playing true/false × pref true/false 的 12 种组合（表驱动）。关键断言：`Rerouted` 不重建；`Lost` 播放中总会暂停；pref=false 时 `Invalidated` 不暂停但重建。
2. 单测 `device_target_never_exceeds_capacity`：`target_for` 在 (192_000, 8) 设备、容量 = 48_000*3*2 时返回值 ≤ 容量*2/3 且是正数。
3. e2e（`src/e2e.rs`，复用 `corpus()` 跳过逻辑）`device_invalidated_rebuilds_and_resumes`：
   - 给 `Player` 加 `#[cfg(test)] pub(crate) fn simulate_device_event(&self, ev: DeviceEvent)`（直接 `send(EngineCmd::Device(ev))`）。
   - 播放 `sine.flac`（10 秒那首），等 `current_time > 1.0`；`set_pause_on_device_change(false)`；`simulate_device_event(Invalidated)`；等 1.5 秒后断言 `is_playing == true` 且 `current_time` 比事件前大（流重建后继续走）。
   - 再 `set_pause_on_device_change(true)`，`simulate_device_event(Rerouted)`，断言 300ms 内 `is_playing == false`。
   - 用例结束 `pause()`，恢复 `set_pause_on_device_change(true)`。
   - 若 CI 没有音频设备导致 `open_output` 失败，e2e 现有用例已处理这种环境——照它们的方式容忍（看 `end_to_end_playback` 怎么处理无输出设备），不要为本用例另造跳过逻辑。

**Verify**：`cargo test device_` → 全部通过；`nsh test .` → exit 0。

### Step 6：手动验证（有条件就做，结果写进汇报）

- macOS：播放中在「系统设置 → 声音 → 输出」切换设备；开关「设备变化时暂停」分别验证暂停 / 继续。
- Windows（若有）：播放中拔耳机或在声音设置切默认设备；关掉开关时应在新设备上继续出声（改前会无声）。

### Step 7：README

「播放」一节加一条：`- 输出设备变化（拔耳机、切换默认设备）时自动暂停；可在设置 →「播放」→「输出设备」关掉，改为在新设备上继续播放。`

**Verify**：`rg -n "输出设备" README.md` → 有输出。

## Test plan

- 2 个纯函数单测 + 1 个 e2e（Step 5）。设备真实插拔无法自动化，靠 Step 6 手动验证。

## Done criteria

- [ ] `nsh check .`、`nsh test .` exit 0；Windows 交叉检查 exit 0
- [ ] `rg -n 'eprintln!\("zeo-player cpal' src/engine/audio.rs` 所在闭包里有 `tx.send`
- [ ] `run` 里不再有 `let has_output = _stream.is_some();` 这种一次性求值
- [ ] 新增单测与 e2e 通过
- [ ] README 已更新；`plans/README.md` 状态行更新

## STOP conditions

- cpal 0.18.2 的错误回调里拿不到 `ErrorKind`（或没有 `StreamInvalidated` / `DeviceChanged` 变体）。
- drop 旧 `cpal::Stream` 在引擎线程上卡住（e2e 超时）——不要加 `std::mem::forget` 之类的绕法，汇报。
- 重建后 `current_time` 跳变超过 1 秒（位置基线换算错误）且两次修正仍不对。
- 需要改 `Ring` 的无锁读写逻辑才能工作。

## Maintenance notes

- 005（ReplayGain）、006（均衡器）、008（暂停时暂停输出流）都建立在 `Output` / `open_output` 上：新的回调内状态（如 EQ 滤波器状态）要在 `open_output` 建流时创建，重建时随流重置。
- 审阅重点：所有用到设备格式的地方是否都改成读 `out`，没有残留旧的局部变量；`handle_device` 里 drop 旧流与建新流的顺序（必须先 drop，Windows 上同一设备可能不允许并存两个共享模式流——不确定，先 drop 最稳）。
