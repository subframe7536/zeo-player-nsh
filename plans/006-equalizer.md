# Plan 006：10 段图形均衡器

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- src/engine src/config/mod.rs src/ui/store.rs src/ui/views/settings src/ui/components/slider.rs README.md`
> 计划 001/002/004/005 的改动是预期内的；确认 004（`open_output`）与 005（`rg_factor`）已在代码里，再核对下文摘录。

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED（实时音频回调里的 DSP）
- **Depends on**: plans/001-split-settings.md、plans/004-output-device-change.md、plans/005-replaygain.md
- **Category**: direction（issue #7）
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

issue #7 要求均衡器。实现成标准的 10 段图形 EQ（31 Hz … 16 kHz，±12 dB，带预设），处理放在 cpal 输出回调里，拖动滑块立即生效（不用等 2 秒的环形缓冲）。

## 设计决定（已定，不要改）

- 频点：`[31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]` Hz；每段 RBJ Audio EQ Cookbook 的 **peaking** 双二阶滤波器，`Q = 1.41`；增益 −12…+12 dB，步进 0.5 dB；另有前级增益（preamp）−12…+12 dB。
- 中心频率 ≥ `0.45 * 采样率` 的段直接跳过（避免接近奈奎斯特时滤波器不稳定）。
- **回调零分配、零阻塞**：滤波器状态（每段每声道的 z1/z2）放在输出回调闭包自己的局部变量里（cpal 回调是 `FnMut`）；参数经 `Shared` 里的「代次原子量 + `Mutex`」交给回调：回调每块先比较代次，变了才 `try_lock`（拿不到就下一块再试），拿到后按本流的采样率重算系数。
- 关闭 EQ 或所有段为 0 且 preamp 为 0 时完全旁路（不跑滤波）。
- 处理顺序：环形缓冲取样 → EQ → `× rg × volume × fade` → clamp（与 005 的约定一致）。
- 预设：平直、流行、摇滚、爵士、古典、人声、低音增强、高音增强；用户手动拖任一段后预设显示为「自定义」。

## Current state

- 计划 004 之后：`src/engine/audio.rs` 有 `struct Output { stream, rate, channels }` 与 `fn open_output(shared, tx, fallback) -> Output`，其中 `device.build_output_stream(config, move |data: &mut [f32], _| output_callback(&shared, data, channels), …)`。
- 计划 005 之后：`output_callback` 内样本为 `(sample * rg * volume * fade_curve(gain)).clamp(-1.0, 1.0)`，`ring.pop(max, |i, sample| …)` 按交错样本逐个回调（`i % ch == 0` 是帧首）。
- 设置模式：`LibraryConfig`（`src/config/mod.rs`）+ `LibraryStore` getter/setter + `LibraryState`/`library_state()`/`store::set_xxx`（`src/ui/store.rs`）+ `install()` 末尾推给引擎。005 的 `ReplayGainConfig` 是同一路数的现成范例——照它写。
- 滑块组件 `src/ui/components/slider.rs:62`：
  ```rust
  pub fn slider(
      fraction: impl Fn() -> f64 + Copy + 'static,   // 0..1
      track: AdaptiveColor, fill: AdaptiveColor, thumb: AdaptiveColor,
      on_move: impl Fn(f64) + 'static,               // 拖动中
      on_release: impl Fn(f64) + 'static,            // 松手
  ) -> View
  ```
  水平；播放条音量用它（看 `src/ui/frame/bar.rs` 里的调用拿颜色参数写法）。
- 下拉写法：`src/ui/views/playlists.rs:591` 的 `select(current_label, "排序", options, width)`（`options: Vec<(Option<Icon>, &str, Box<dyn Fn()>)>`）；`select` 定义在 `playlists.rs` 或 `components::form`（`rg -n "fn select\(" src/ui` 找到它，若是 `playlists.rs` 私有函数，见 Step 4 说明）。

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 编译 | `nsh check .` | exit 0 |
| DSP 单测 | `cargo test eq_` | 全部通过 |
| 全部测试 | `nsh test .` | exit 0 |

## Scope

**In scope**：`src/engine/eq.rs`（新建）、`src/engine/audio.rs`、`src/engine/mod.rs`、`src/config/mod.rs`、`src/ui/store.rs`、`src/ui/views/settings/playback.rs`、`src/ui/views/settings/mod.rs`、`src/ui/views/playlists.rs` 或 `src/ui/components/form.rs`（仅当需要把 `select` 提升可见性）、`tests/ui.rs`、`README.md`。

**Out of scope**：参数 EQ / 自定义频点；可视化频响曲线；竖向滑块组件；保存多个自定义预设；对 `components/slider.rs` 的修改。

## Git workflow

不要 commit / push。

## Steps

### Step 1：DSP 模块 `src/engine/eq.rs`

```rust
//! 10 段图形均衡器：RBJ peaking 双二阶，实时回调里运行（不分配、不加锁）。

pub const BANDS: [f32; 10] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
pub const MAX_DB: f32 = 12.0;
const Q: f32 = 1.41;
/// 最多声道数（超过的声道不处理 EQ，原样输出）。
pub const MAX_CHANNELS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqConfig { pub enabled: bool, pub preamp_db: f32, pub bands: [f32; 10] }
// Default：enabled false，全 0。

impl EqConfig {
    /// 是否等于直通（关闭，或全部为 0）。
    pub fn is_flat(&self) -> bool
}

/// 预设（名字, 10 段增益）。
pub const PRESETS: &[(&str, [f32; 10])] = &[ ("平直", [0.0; 10]), ("流行", […]), … ];

pub struct Equalizer { /* 每段系数 b0 b1 b2 a1 a2（已除 a0）、每段每声道 z1 z2、preamp 线性值、active 标记 */ }

impl Equalizer {
    pub fn new() -> Self                                  // 旁路
    pub fn configure(&mut self, cfg: &EqConfig, rate: u32) // 重算系数；状态清零
    /// 原地处理交错样本。旁路时立即返回。
    pub fn process(&mut self, data: &mut [f32], channels: usize)
}
```

- 系数（RBJ cookbook peaking）：`A = 10^(dB/40)`，`w0 = 2π f0 / fs`，`alpha = sin(w0)/(2Q)`；`b0 = 1 + alpha*A`，`b1 = -2cos(w0)`，`b2 = 1 - alpha*A`，`a0 = 1 + alpha/A`，`a1 = -2cos(w0)`，`a2 = 1 - alpha/A`；全部除以 `a0`。
- 用 Transposed Direct Form II：`y = b0*x + z1; z1 = b1*x - a1*y + z2; z2 = b2*x - a2*y`。
- 增益为 0 的段跳过；`f0 >= 0.45 * rate` 的段跳过。
- 预设数值（dB，按频点顺序）：
  - 流行 `[-1, 1, 3, 4, 3, 0, -1, -1, 1, 2]`
  - 摇滚 `[4, 3, 2, 0, -1, -1, 1, 2, 3, 4]`
  - 爵士 `[3, 2, 1, 2, -1, -1, 0, 1, 2, 3]`
  - 古典 `[4, 3, 2, 1, -1, -1, 0, 2, 3, 4]`
  - 人声 `[-2, -2, -1, 1, 3, 4, 3, 1, 0, -1]`
  - 低音增强 `[6, 5, 4, 2, 0, 0, 0, 0, 0, 0]`
  - 高音增强 `[0, 0, 0, 0, 0, 0, 2, 4, 5, 6]`

在 `src/engine/mod.rs` 加 `pub(crate) mod eq;`。

**Verify**：`nsh check .` → exit 0。

### Step 2：DSP 单测（先写，确认正确再接回调）

`eq.rs` 末尾 `#[cfg(test)] mod tests`：

- `eq_flat_is_identity`：`configure(全 0, 48000)` 后处理一段随机样本，输出与输入逐样本相等（旁路）。
- `eq_boost_at_center_frequency`：仅 1 kHz 段 +6 dB，48 kHz 立体声 1 kHz 正弦（幅度 0.25，2 秒），丢弃前 0.2 秒（瞬态）后 RMS 比 ≈ 1.995（容差 ±3%）；同时 100 Hz 正弦的 RMS 比在 0.95…1.08 之间（远离中心几乎不变）。
- `eq_cut_reduces_level`：1 kHz −6 dB → RMS 比 ≈ 0.501（±3%）。
- `eq_skips_bands_near_nyquist`：rate 32000 时 16 kHz 段 +12 dB，处理 1 秒白噪声后无 NaN / Inf，且输出峰值 < 4。
- `eq_preamp_scales`：所有段 0、preamp −6 dB、enabled → 输出 ≈ 输入 × 0.501。
- `eq_channels_are_independent`：左声道正弦、右声道全 0 → 右声道输出全 0。

**Verify**：`cargo test eq_` → 6 passed。

### Step 3：接进输出回调

1. `Shared` 加 `pub eq_gen: AtomicU64`（初 0）与 `pub eq_config: Mutex<EqConfig>`。
2. `open_output` 建流时，闭包里放局部状态：
   ```rust
   let mut eq = Equalizer::new();
   let mut eq_seen = u64::MAX; // 强制首块读取
   move |data: &mut [f32], _| {
       let g = shared.eq_gen.load(Ordering::Acquire);
       if g != eq_seen {
           if let Ok(cfg) = shared.eq_config.try_lock() {
               eq.configure(&cfg, rate);
               eq_seen = g;
           }
       }
       output_callback(&shared, &mut eq, data, channels)
   }
   ```
   （`rate` / `channels` 是本次 `open_output` 拿到的设备格式；流重建时状态自然重置。）
3. `output_callback` 加参数 `eq: &mut Equalizer`：`ring.pop` 取完后（在应用 `rg × volume × fade` 之前）对已取出的 `data[..take]` 调 `eq.process(&mut data[..take], channels)`。为此把现有「取样即乘增益」的闭包拆成两步：先 `pop` 把原始样本写进 `data`，再 EQ，再逐帧推进淡变并乘 `rg × volume × fade_curve(gain)` 后 clamp。**淡变推进逻辑（每帧一次、`take == 0` 时也推进）保持原样**，只是挪到第二遍循环里。
4. `Player::set_eq(&self, cfg: EqConfig)`：`*shared.eq_config.lock() = cfg; shared.eq_gen.fetch_add(1, Release);`（UI 线程上执行，锁只与回调的 `try_lock` 竞争，回调不会被阻塞）。

**Verify**：`nsh check .` → exit 0；`cargo test` 里原有的 `fade_ramps_over_the_configured_time`（`audio.rs` 单测，它会直接调 `output_callback`）按新签名传 `&mut Equalizer::new()` 后仍通过；`nsh test .` → exit 0。

### Step 4：设置与界面

1. `LibraryConfig` 加 `#[serde(default)] pub eq: crate::engine::eq::EqConfig`（Default 补字段）；`LibraryStore` getter/setter；`LibraryState` 加 `eq`；`store.rs` 加 `pub fn set_eq(cfg)`：更新信号 + `player().set_eq(cfg)` 立即生效；**持久化防抖**：用一个代次计数，`spawn` 一个 400ms 的 `timer` 后若代次未变才后台保存（拖滑块时不每次写 `library.json`）。`install()` 末尾推一次。
2. `settings/playback.rs` 新增 `pub(super) fn eq_section() -> View`：
   - `setting_section("均衡器", "按频段提升或衰减音量。拖动立即生效。", Some(开关))`：右上角用 nsh-base `switch`（样式照 `pref_row` 里的 `toggle`），无障碍标签 `启用均衡器`。
   - 启用时显示（`when`）：
     - 预设下拉：当前值显示预设名，或「自定义」（当前 bands 与任何预设都不相等时）。选项为 `PRESETS` 全部；选中即把 bands 换成预设值。若 `select` 是 `playlists.rs` 的私有函数，把它移到 `src/ui/components/form.rs` 并改为 `pub fn`（同时改 `playlists.rs` 的调用处）——这是本计划允许的唯一跨文件重构。
     - 「重置」文字按钮：bands 全 0、preamp 0。
     - 11 行滑块（前级 + 10 段），每行：左侧标签（`前级`、`31`、`62`、`125`、`250`、`500`、`1k`、`2k`、`4k`、`8k`、`16k`，宽 44pt）+ `slider`（fraction = `(db + 12) / 24`，`on_move` 与 `on_release` 都按 0.5 dB 取整后 `set_eq`）+ 右侧数值（`+3.5 dB`，`tabular_nums`，宽 64pt）。每行 `view` 的无障碍标签为 `均衡器 <标签>`（如 `均衡器 1k`）。
3. `SECTIONS` 加 `{ pane: Pane::Playback, title: "均衡器", keys: "均衡器 eq 低音 高音 人声 预设 频段 音效 流行 摇滚 爵士 古典", build: playback::eq_section }`。

**Verify**：`nsh check .` → exit 0。

### Step 5：UI 测试

`tests/ui.rs` 新增 `equalizer_preset_applies_and_persists`（参照 `settings_search_filters_sections` 打开设置 → 「播放」）：按 `label:启用均衡器` 打开；选预设「低音增强」；断言界面出现文字 `+6.0 dB`；读数据目录的 `library.json`（同计划 002 的读法，轮询 2 秒等防抖保存）断言 `eq.enabled == true` 且 `eq.bands[0] == 6.0`；最后关掉开关恢复。

**Verify**：`cargo test --test ui equalizer` → 1 passed；`nsh test .` → exit 0。

### Step 6：README

「播放」一节加：`- 10 段均衡器（31 Hz–16 kHz，±12 dB）与前级增益，内置流行 / 摇滚 / 爵士 / 古典 / 人声 / 低音增强 / 高音增强预设，拖动立即生效（设置 →「播放」→「均衡器」）。`

## Test plan

- DSP 单测 6 个（Step 2）；回调签名变更后原淡变单测仍通过（Step 3）；UI 测试 1 个（Step 5）。

## Done criteria

- [ ] `cargo test eq_` 6 passed；`nsh test .` exit 0；`nsh check .` exit 0
- [ ] `rg -n "Vec::|Box::new|\.lock\(\)" src/engine/eq.rs` 在 `process` 函数体内无匹配（回调路径零分配零阻塞）
- [ ] `output_callback` 内没有 `.lock()`（只允许闭包里的 `try_lock`）
- [ ] README 已更新；`plans/README.md` 状态行更新

## STOP conditions

- 004 或 005 未完成。
- `eq_boost_at_center_frequency` 两次修正后仍不在容差内（系数公式实现有误时不要放宽容差）。
- 把淡变推进挪到第二遍循环后 `fade_ramps_over_the_configured_time` 失败。
- 需要修改 `components/slider.rs` 才能实现。

## Maintenance notes

- 回调里的状态随 `open_output` 建流而生、随流 drop 而灭；设备切换（004）后 EQ 用新采样率重算系数，这是正确行为。
- 若以后要可视化频响或参数 EQ，`Equalizer` 的系数计算可直接复用。
- 审阅重点：`process` 的 TDF-II 递推、超过 `MAX_CHANNELS` 的声道处理、`try_lock` 失败时下一块会再试（`eq_seen` 只在成功后更新）。
