# Plan 005：ReplayGain 音量标准化

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- src/engine/audio.rs src/engine/mod.rs src/metadata/mod.rs src/db.rs src/config/mod.rs src/ui/store.rs src/ui/views/settings README.md`
> 计划 001/002/004 的改动是预期内的（尤其 004 引入了 `Output` / `open_output` / `DeviceEvent`）；先确认 004 已完成，再核对下文摘录。

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED（改输出回调的增益链 + 数据库迁移）
- **Depends on**: plans/001-split-settings.md、plans/004-output-device-change.md
- **Category**: direction（issue #7）
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

不同专辑的母带响度差很多，切歌时音量忽大忽小。ReplayGain 标签（`REPLAYGAIN_TRACK_GAIN` / `_PEAK`、`REPLAYGAIN_ALBUM_GAIN` / `_PEAK`）记录了把每首（或整张专辑）拉到统一响度所需的增益，播放器读出来乘到输出上即可。issue #7 要求支持。

## 设计决定（已定，不要改）

- **RG 值随扫描进数据库**：`read_meta` 用 lofty 已打开的文件顺手读四个 RG 键（无额外文件 I/O），存进 `MetaInfo` 新字段 `replaygain`；`tracks` 表加四列 `rg_track_gain` / `rg_track_peak` / `rg_album_gain` / `rg_album_peak`（REAL，可空），`SCHEMA_VERSION` +1（预期 3→4）。文件指纹变化（重扫、011 的标签写回）会自动刷新。
- 迁移后老行四列为 NULL：`load_track` 用 `MetaInfo.replaygain`；四项全 `None` 时回退 `read_replaygain(path)` 现场读一次（覆盖还没重扫的老库；没有 RG 标签的文件每次装载会多读一次标签，几毫秒，可接受——不强制整库重扫）。
- **增益乘在输出回调里**（与音量、淡变同处），切换模式立即生效。自然切歌时引擎等缓冲播空（`drain` 到 0）才 `load_track`，所以新增益不会作用到上一首的尾巴。
- 设置项：模式（关闭 / 按曲目 / 按专辑，默认关闭）；预放大（−15…+15 dB，默认 0）；无标签曲目的增益（−15…+15 dB，默认 0）；防止削波（按峰值限制增益，默认开）。
- 模式回退：按专辑但没有专辑增益 → 用曲目增益；按曲目但没有曲目增益 → 用专辑增益；都没有 → 用「无标签曲目的增益」。

## Current state

- 输出回调 `src/engine/audio.rs`（f6e6821 时 1132 行）：
  ```rust
  fn output_callback(shared: &Shared, data: &mut [f32], channels: usize) {
      let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
      …
      let take = shared.ring.pop(data.len(), |i, sample| {
          if i % ch == 0 && gain != target { /* 推进淡变 gain */ }
          data[i] = sample * volume * fade_curve(gain);
      });
  ```
- `load_track`（f6e6821 时 877 行）先 `MetaCache::global().get(&path)` 拿到 `Arc<MetaInfo>`，`Source::open(&path)` 成功后建 `Loaded`、填 `TrackPublic`，最后 `publish_media`。
- `MetaInfo`（`src/metadata/mod.rs:21`）在 `read_meta`（约 120 行）里由 lofty 已打开的文件逐字段填充——RG 在这里读没有额外 I/O。`MetaInfo` 的字面量构造点先用 `rg -n "MetaInfo \{"` 找全（已知 `read_meta` 一处），新字段每处都要给值。
- `src/db.rs`：`SCHEMA_VERSION = 3`；`migrate` 按 `if version < N` 递进（v2 的 `ALTER TABLE tracks ADD COLUMN art_hue / art_sat` 是范例）；`COLUMNS` 常量（26 列）同时用于 `load_meta` 的 `SELECT` 与 `save_meta` 的 `INSERT OR REPLACE`；`load_meta` 按序号 `r.get(N)` 组装 `MetaInfo` 放进 `CacheEntry`。`mod tests` 尚未有——`Connection::open_in_memory()` + `migrate(&conn)` 可测迁移（计划 007 也按此模式补，谁先落地谁建 `mod tests`）。
- `Shared`（`audio.rs:209`）是引擎线程 / 回调 / 读取方共享的原子量集合；`Player`（`src/engine/mod.rs`）的 setter 直接写原子量（例 `set_fade_ms`）或发 `EngineCmd`。
- lofty 0.25 的 `ItemKey` 有 `ReplayGainTrackGain`、`ReplayGainTrackPeak`、`ReplayGainAlbumGain`、`ReplayGainAlbumPeak`（ID3v2 TXXX、Vorbis comment、APE、MP4 `----:com.apple.iTunes:replaygain_*` 都映射到它们）。值是文本，常见形如 `-6.54 dB`、`+1.20 dB`、`0.988525`。
- 标签读取写法参照 `src/metadata/mod.rs:200` 的 `read_lyrics`：
  ```rust
  pub fn read_lyrics(path: &Path) -> Option<String> {
      let tagged = lofty::read_from_path(path).ok()?;
      let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
      first(tag, ItemKey::Lyrics).or_else(|| first(tag, ItemKey::UnsyncLyrics))
  }
  ```
- 配置 / 状态 / setter 的模式：`LibraryConfig` 字段 + `#[serde(default…)]`（`src/config/mod.rs:133`），`impl Default for LibraryConfig`（约 220 行）逐字段列出，`LibraryStore` getter/setter，`LibraryState` + `library_state()` + `store::set_xxx`（`src/ui/store.rs`），`install()` 末尾把设置推给引擎。
- 数字输入写法：见计划 002 落地后的 `src/ui/views/settings/playback.rs`（草稿信号 + 提交时校验）。下拉：`src/ui/components/form.rs` 的 `select_trigger` 与 `src/ui/views/playlists.rs:591` 的 `select(...)` 用法；单选组：`components::radio::group_row()` + `radio::item(checked, label, on_select)`（`playback.rs` 改前的写法）。

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 编译 | `nsh check .` | exit 0 |
| 单测 | `cargo test replaygain` | 新用例全部通过 |
| 全部测试 | `nsh test .` | exit 0 |
| 生成带 RG 标签的样本（Step 6） | 见 Step 6 | — |

## Scope

**In scope**：`src/metadata/mod.rs`（`ReplayGain` 结构、解析函数、`read_replaygain`、`MetaInfo` 新字段、`read_meta` 填充）、`src/db.rs`（迁移 + 读写映射）、`src/engine/audio.rs`、`src/engine/mod.rs`、`src/config/mod.rs`、`src/ui/store.rs`、`src/ui/views/settings/playback.rs`、`src/ui/views/settings/mod.rs`（`SECTIONS`）、`scripts/make-test-music.sh`（仅在格式语料目录加一个样本）、`README.md`。

**Out of scope**：扫描 / 计算 ReplayGain（只读已有标签）；Opus（003 已放弃，其 `R128_TRACK_GAIN` 不在范围）；全屏页显示 RG 值。

## Git workflow

不要 commit / push。

## Steps

### Step 1：读取与解析 RG 标签，进 MetaInfo

`src/metadata/mod.rs` 新增：

```rust
/// ReplayGain 标签（dB 与线性峰值）；没有的项为 `None`。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ReplayGain {
    pub track_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<f32>,
    pub album_peak: Option<f32>,
}

impl ReplayGain {
    /// 四项全空（没读到任何 RG 标签，或迁移前的老行）。
    pub fn is_empty(&self) -> bool
}

/// 从已打开的文件读 ReplayGain（`read_meta` 扫描时调用）。
fn replaygain_of(tagged: &lofty::file::TaggedFile) -> ReplayGain

/// 独立按路径读一次（引擎对迁移前的老行回退时用）。
pub fn read_replaygain(path: &Path) -> ReplayGain

/// `-6.54 dB` / `+1.2dB` / `-6.54` → -6.54；解析失败 `None`。
fn parse_gain(text: &str) -> Option<f32>
/// `0.988525` → 0.988525；非正数或解析失败 `None`。
fn parse_peak(text: &str) -> Option<f32>
```

`replaygain_of` / `read_replaygain` 共用同一查找逻辑：先查主标签，再查其余标签（`tagged.tags()`），每个键取第一个能解析的值。`parse_gain`：去掉首尾空白、不区分大小写去掉结尾 `db`、允许前导 `+`，结果须 `is_finite()` 且在 −60…+60。

`MetaInfo` 加 `pub replaygain: ReplayGain`；`read_meta` 里 `replaygain: replaygain_of(&tagged)`；`MetaInfo` 的其它字面量构造点（`rg -n "MetaInfo \{"`）补 `replaygain: ReplayGain::default()`。

**Verify**：`nsh check .` → exit 0。

### Step 2：入库（db.rs）

`src/db.rs`（版本号取「执行时当前值 + 1」，下面按预期的 3→4 写）：

1. `SCHEMA_VERSION` 改为 4；`migrate` 加：
   ```rust
   if version < 4 {
       // ReplayGain 标签随扫描入库；迁移前的老行为 NULL，
       // 装载时由引擎回退到现场读标签（read_replaygain）。
       conn.execute_batch(
           "ALTER TABLE tracks ADD COLUMN rg_track_gain REAL;
            ALTER TABLE tracks ADD COLUMN rg_track_peak REAL;
            ALTER TABLE tracks ADD COLUMN rg_album_gain REAL;
            ALTER TABLE tracks ADD COLUMN rg_album_peak REAL;",
       )?;
   }
   ```
   （数字 4 与目标 `SCHEMA_VERSION` 一致；若落地时版本已被别的计划抬高，两处一起改。）
2. `COLUMNS` 末尾追加 `, rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak`（现在 26 列 → 30 列）。
3. `load_meta`：`meta` 组装处加
   ```rust
   replaygain: ReplayGain {
       track_gain: r.get::<_, Option<f64>>(26)?.map(|v| v as f32),
       track_peak: r.get::<_, Option<f64>>(27)?.map(|v| v as f32),
       album_gain: r.get::<_, Option<f64>>(28)?.map(|v| v as f32),
       album_peak: r.get::<_, Option<f64>>(29)?.map(|v| v as f32),
   },
   ```
4. `save_meta`：VALUES 占位符补到 `?30`，params 追加 `m.replaygain.track_gain`、`m.replaygain.track_peak`、`m.replaygain.album_gain`、`m.replaygain.album_peak`（`Option<f32>` 直接传给 rusqlite 即可）。

**Verify**：`nsh check .` → exit 0。

### Step 3：增益计算（纯函数）

`src/engine/audio.rs` 新增：

```rust
/// ReplayGain 模式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplayGainMode { #[default] Off, Track, Album }

/// ReplayGain 设置（界面 → 引擎）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ReplayGainConfig {
    pub mode: ReplayGainMode,
    pub preamp_db: f32,
    pub fallback_db: f32,
    pub clip_protect: bool,
}
// Default：Off / 0 / 0 / true

/// 一首曲目应乘的线性增益。
pub(crate) fn replaygain_factor(cfg: &ReplayGainConfig, rg: &crate::metadata::ReplayGain) -> f32
```

规则：`Off` → 1.0。否则按「设计决定」里的回退顺序取 `(gain_db, peak)`；无标签时 `gain_db = fallback_db`、`peak = None`。`db = gain_db + preamp_db`，`factor = 10^(db/20)`。`clip_protect && peak.is_some()` 时 `factor = factor.min(1.0 / peak)`。结果钳到 `0.0..=4.0`（+12 dB 上限，防止标签写错爆音）。

**Verify**：`nsh check .` → exit 0。

### Step 4：接进引擎

1. `Shared` 加 `pub rg_factor: AtomicU32`（f32 位，初值 1.0）与 `pub rg_config: Mutex<ReplayGainConfig>`（只有引擎线程与 setter 用，不在回调里锁）。
2. `Loaded` 加 `rg: crate::metadata::ReplayGain`；`load_track` 里 `meta`（`MetaCache::global().get(&path)` 已拿到）：
   ```rust
   let rg = if meta.replaygain.is_empty() {
       crate::metadata::read_replaygain(&path) // 迁移前的老行 / 缓存里没有
   } else {
       meta.replaygain
   };
   ```
   存进 `Loaded`，并 `shared.rg_factor.store(replaygain_factor(&cfg, &rg).to_bits())`（`cfg` 从 `rg_config` 取一份）。
3. `EngineCmd` 加 `SetReplayGain(ReplayGainConfig)`：存进 `rg_config`，若有 `loaded` 立即按新配置重算 `rg_factor`。它**不**属于 `interrupts`（不淡出）。
4. `output_callback`：`let rg = f32::from_bits(shared.rg_factor.load(Ordering::Relaxed));`，样本写成 `(sample * rg * volume * fade_curve(gain)).clamp(-1.0, 1.0)`。
5. `Player::set_replaygain(&self, cfg: ReplayGainConfig)` 发 `EngineCmd::SetReplayGain(cfg)`。

**Verify**：`nsh check .` → exit 0；`nsh test .` → exit 0。

### Step 5：设置

1. `LibraryConfig` 加 `#[serde(default)] pub replaygain: crate::engine::audio::ReplayGainConfig`（`ReplayGainConfig` 需要 `Default`；若 `#[serde(default)]` 对结构体字段要求 `Default` 已满足即可）。`impl Default for LibraryConfig` 补字段；`LibraryStore` 加 `replaygain()` / `set_replaygain(cfg)`。
2. `LibraryState` 加 `replaygain` 字段；`store.rs` 加 `pub fn set_replaygain(cfg)`（信号 + `player().set_replaygain(cfg)` + 后台保存）；`install()` 末尾推一次。
3. `settings/playback.rs` 新增 `pub(super) fn replaygain_section() -> View`：
   - `setting_section("ReplayGain", "按曲目或专辑里的 ReplayGain 标签调整音量，让不同专辑听起来一样响。只读取已有标签，不会扫描计算。", None)`
   - 模式：单选组 `关闭 / 按曲目 / 按专辑`。
   - 模式不是「关闭」时才显示下面三项（`when`）：
     - 「预放大」数字输入（dB，允许一位小数，−15…+15，非法回退），旁边文字 `dB`。
     - 「无标签曲目」数字输入（dB，同上）。
     - `pref_row("防止削波", "按标签里的峰值限制增益，避免爆音。", …)`。
   - 无障碍标签：`ReplayGain 预放大`、`ReplayGain 无标签增益`（供测试定位）。
4. `SECTIONS` 加 `{ pane: Pane::Playback, title: "ReplayGain", keys: "replaygain 回放增益 音量 标准化 响度 专辑 曲目 预放大 削波 峰值", build: playback::replaygain_section }`。

**Verify**：`nsh check .` → exit 0。

### Step 6：测试

1. `src/metadata/mod.rs` 单测 `replaygain_parses_common_forms`：`parse_gain("-6.54 dB") == Some(-6.54)`、`"+1.2dB"`、`" -3 "`、`"abc" → None`、`"nan" → None`、`"-100 dB" → None`；`parse_peak("0.988525")`、`"0" → None`。
2. `src/engine/audio.rs` 单测 `replaygain_factor_follows_mode_and_fallbacks`（表驱动）：Off → 1.0；Track 有曲目增益 −6.02 dB → ≈0.5；Album 无专辑增益回退曲目；都没有 → fallback；preamp +6 叠加；clip_protect 时 peak 0.8、增益 +6 dB → 1/0.8；钳到 4.0。浮点比较用 `(a-b).abs() < 1e-3`。
3. 带标签的样本：`scripts/make-test-music.sh` 在格式语料目录（计划 003 新增的 `.nsh/tmp/zeo-player-test-formats`；若 003 尚未执行，见 STOP）加 `rg.flac`：`-metadata REPLAYGAIN_TRACK_GAIN="-6.00 dB" -metadata REPLAYGAIN_TRACK_PEAK="0.5" -metadata REPLAYGAIN_ALBUM_GAIN="-3.00 dB"`。单测 `replaygain_reads_flac_tags`：语料不存在时跳过；`read_meta` 与 `read_replaygain` 都读出 track_gain −6.0、track_peak 0.5、album_gain −3.0、album_peak None。**注意**：003 的格式测试断言该目录有 5 个文件、逐个解码——加了 `rg.flac` 后，把那个测试里的数量 / 文件列表同步改成 6（只改数量与列表，不改其它断言）。
4. `src/db.rs` 的 `mod tests`（若 007 先落地已建，就往里加）：`replaygain_migrate_adds_columns`——先执行 `SCHEMA_V1` 与 v2/v3 的迁移语句、把 `user_version` 设为 3（若落地时版本不同，改成实际的前一版本），`migrate` 后 `user_version == 4` 且 `PRAGMA table_info(tracks)` 含四个 `rg_` 列、已有 `tracks` 行还在；`replaygain_db_roundtrip`——内存库 `migrate` 后 `save_meta` 写一行带 `ReplayGain{ track_gain: Some(-6.0), .. }` 的 `CacheEntry`，`load_meta` 读回相等（db.rs 的测试要 `unsafe { std::env::set_var("ZEO_PLAYER_DATA_DIR", …) }` 吗？不用——`save_meta` / `load_meta` 走 `db()`，单元测试下 `db()` 返回 None。改为在 `mod tests` 里直接对 `open_in_memory` 的连接调 `migrate`，再复用 `load_meta` / `save_meta` 的内部写法；若这两个函数写成接收 `&Connection` 的私有函数（参照 007 的模式：`pub fn` 薄壳 + `fn xxx(conn)` 实体），测试直接调实体函数。）

**Verify**：`sh scripts/make-test-music.sh`；`cargo test replaygain` → 4 个用例通过；`nsh test .` → exit 0。

### Step 7：README

「播放」一节加：`- ReplayGain：按曲目或按专辑标签调整音量，可设预放大、无标签曲目的增益与防削波（设置 →「播放」，默认关闭）。`

## Test plan

- 解析单测、增益计算单测、真实文件读取单测、数据库迁移 + 往返单测各 1 个（Step 6）。回调里的乘法由增益计算单测覆盖其输入；不为回调单独写测试。

## Done criteria

- [ ] `nsh check .`、`nsh test .` exit 0
- [ ] `cargo test replaygain` 4 个用例通过（语料存在时不跳过）
- [ ] `rg -n "rg_factor" src/engine/audio.rs` 出现在 `output_callback` 内
- [ ] `rg -n "rg_track_gain" src/db.rs src/metadata/mod.rs` 有输出；迁移测试通过
- [ ] README 已更新；`plans/README.md` 状态行更新

## STOP conditions

- 计划 004 未完成（`audio.rs` 里没有 `open_output`）。
- 计划 003 未完成（没有 `.nsh/tmp/zeo-player-test-formats` 的生成逻辑）：先完成 Step 1–5、7 与前两个单测，真实文件与数据库样本相关断言留待 003 后补，并在汇报里说明。
- `SCHEMA_VERSION` 已被改且你无法确定是哪个计划加的迁移（避免两个计划用同一个版本号）。
- lofty 读不到 ffmpeg 写进 FLAC 的 `REPLAYGAIN_*`（`read_meta` / `read_replaygain` 全为 `None`）。
- 加了 `clamp(-1.0, 1.0)` 之后现有 e2e / 淡变单测失败。

## Maintenance notes

- 006（均衡器）也在 `output_callback` 里插处理；顺序约定：取样 → EQ → `rg * volume * fade` → clamp。
- 以后若想显示「当前增益 −6.5 dB」，`rg_factor` 已在 `Shared` 里，可加进 `Snapshot`。
- 011（标签写回）改标签后 `MetaInfo` 按指纹失效重读，`rg` 字段随之刷新，无需额外联动。
- 以后若恢复 Opus，还要读 `R128_TRACK_GAIN`（Q7.8 定点，参考 −23 LUFS），与 ReplayGain 是另一套基准。
