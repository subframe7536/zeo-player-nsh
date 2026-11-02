# Plan 003：支持 AAC / ALAC（M4A）、OGG Vorbis、AIFF

> **执行说明**：按步骤执行，每步跑完验证命令、结果符合预期再进下一步。遇到「STOP 条件」立即停下汇报，不要自行发挥。完成后把 `plans/README.md` 里本计划的状态改为 DONE（若审阅者派发你并说明由其维护索引则不改）。
>
> **漂移检查（先跑）**：`git diff --stat f6e6821..HEAD -- Cargo.toml src/engine/codec.rs src/provider/local/mod.rs src/metadata/mod.rs src/ui/components/tracks.rs src/ui/views/mod.rs scripts/make-test-music.sh README.md`
> 有输出时先核对下文摘录；对不上即 STOP。设置页文案在 001 之后位于 `src/ui/views/settings/library.rs`。

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW（只开 symphonia 内置 feature，无新外部依赖）
- **Depends on**: none
- **Category**: direction（issue #6）
- **Planned at**: commit `f6e6821`, 2026-11-02

## Why this matters

现在只能播 MP3 / FLAC / WAV。issue #6 的目标格式里，用户确认保留：AAC、ALAC、M4A、OGG（Vorbis）、AIFF；放弃 APE / WMA / DSD / 独占模式，**Opus 也已放弃**（symphonia 无原生解码器，需要外部 libopus 适配依赖，用户决定不为它引入 C 依赖）。symphonia 0.6 已经内置 AAC、ALAC、MP4 容器、OGG、Vorbis、AIFF，只是 `Cargo.toml` 没开。

## Current state

- `Cargo.toml`：
  ```toml
  symphonia = { version = "0.6", features = ["mp3", "flac", "wav"] }
  ```
  symphonia 0.6.1 的 feature：`aac`、`alac`、`isomp4`（MP4/M4A 容器）、`ogg`、`vorbis`、`aiff`；`default` 里已含 `ogg`、`vorbis`、`flac`、`wav`、`pcm`、`adpcm`、`mkv`、`all-meta`（我们没关 default，所以 ogg/vorbis 其实已在编译里，但扩展名白名单挡住了）。
- `src/engine/codec.rs`：解码封装。头注释 `//! 基线 MP3 / FLAC / WAV（opus/ogg 暂缓，恢复时在此处接线）。`；解码器两处都用全局注册表：
  ```rust
  // Source::open，codec.rs:52
  let decoder = symphonia::default::get_codecs()
      .make_audio_decoder(&params, &AudioDecoderOptions::default())
  // next_block 的 ResetRequired 分支，codec.rs:100
  self.decoder = symphonia::default::get_codecs().make_audio_decoder(...)
  ```
  探测用 `symphonia::default::get_probe()`（不用改）。
- `src/provider/local/mod.rs:16`：`pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "wav"];`（扫描与 `watcher.rs` 共用，比较前已转小写）。
- `src/metadata/mod.rs`：
  - `codec_of(file_type)`（约 91 行）已映射 `Opus => "OPUS"`、`Vorbis => "Vorbis"`、`Aac => "AAC"`、`Mp4 => "MP4"`、`Aiff => "AIFF"`。
  - `read_meta` 里的 `quality`：
    ```rust
    quality: match file_type {
        lofty::file::FileType::Flac | lofty::file::FileType::Wav | lofty::file::FileType::Aiff => Some("Lossless".to_owned()),
        lofty::file::FileType::Mpeg => Some("Lossy".to_owned()),
        _ => None,
    },
    ```
- `src/metadata/waveform.rs:79` 用 `crate::engine::codec::Source::open` 解码——改完 codec.rs 后波形自动支持新格式。
- 写死了旧格式列表的界面文案：
  - `src/ui/views/mod.rs:113`：`"添加一个包含 MP3、FLAC 或 WAV 的文件夹，Zeo Player 会扫描并整理它们。"`
  - 设置「音乐文件夹」节说明（f6e6821 时 `settings.rs:203`）：`"Zeo Player 会扫描这些文件夹里的 MP3、FLAC 和 WAV，包括子文件夹。…"`，以及 `SECTIONS` 里该节 `keys: "添加 文件夹 目录 拖放 根目录 mp3 flac wav"`。
  - `src/ui/components/tracks.rs:21,70` 的注释 `MP3 / FLAC / WAV`。
  - `README.md:5` `支持格式：MP3、FLAC、WAV。`；`README.md` 「尚未支持」`- Opus / OGG 格式、逐字歌词、Linux 版本。`
- 测试语料：`scripts/make-test-music.sh` 生成 `.nsh/tmp/zeo-player-test-music`（6 个文件）。**`src/e2e.rs` 断言扫描数量 `== 6`，UI 测试也依赖这批文件的数量**——新格式样本不能放进这个目录。
- e2e 测试的语料探测写法（`src/e2e.rs:16`）：
  ```rust
  fn corpus() -> Option<std::path::PathBuf> {
      let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".nsh/tmp/zeo-player-test-music");
      dir.is_dir().then_some(dir)
  }
  ```
  没有语料时打印「跳过」后 `return`。
- CI（`.github/workflows/ci.yml`）：macOS 与 Windows 都跑 `nsh check` / `nsh test`；只有 macOS 用 brew 装 ffmpeg 生成语料。

## Commands you will need

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 生成测试语料 | `sh scripts/make-test-music.sh` | 打印两行 `wrote N files to …` |
| 编译 | `nsh check .` | exit 0 |
| 测试 | `nsh test .` | exit 0 |
| 只跑格式测试 | `cargo test decodes_every_supported_format -- --nocapture` | 1 passed，且日志里没有「跳过」 |
| Windows 交叉检查 | `PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH" cargo xwin check --target x86_64-pc-windows-msvc` | exit 0 |

## Scope

**In scope**：`Cargo.toml`、`Cargo.lock`、`src/engine/codec.rs`、`src/provider/local/mod.rs`、`src/metadata/mod.rs`、`src/ui/views/mod.rs`、`src/ui/views/settings/library.rs`、`src/ui/views/settings/mod.rs`（仅 keys）、`src/ui/components/tracks.rs`（仅注释）、`scripts/make-test-music.sh`、`README.md`、`AGENTS.md`（测试语料说明一句）。

**Out of scope**：APE / WMA / DSD；**Opus**（用户决定放弃；若以后要做，用 `symphonia-adapter-libopus` 之类的外部解码适配器另行立项）；`.mp4` 扩展名（可能是视频，不加）；`caf`、`mkv`；改智能歌单的格式条件（它直接用扩展名，自动支持）。

## Git workflow

不要 commit / push。

## Steps

### Step 1：开启 symphonia 内置格式

```toml
symphonia = { version = "0.6", features = ["mp3", "flac", "wav", "aac", "alac", "isomp4", "ogg", "vorbis", "aiff"] }
```

**Verify**：`nsh check .` → exit 0。

### Step 2：扩展名白名单与音质标注

1. `AUDIO_EXTENSIONS` 改为 `&["mp3", "flac", "wav", "m4a", "aac", "ogg", "oga", "aif", "aiff"]`。
2. `read_meta` 的 `quality`：
   - `Aiff` 已是 Lossless。
   - `Vorbis`、`Aac` → `Lossy`。
   - `Mp4`：M4A 可能是 AAC（有损）也可能是 ALAC（无损）。lofty 的通用 `FileProperties` 对 ALAC 有 `bit_depth`、对 AAC 为 `None`，据此：`bit_depth.is_some()` → `Lossless` 且 `codec` 改为 `"ALAC"`，否则 `Lossy` 且 `codec` 为 `"AAC"`。写一行中文注释说明这是按位深推断。若实测（Step 4 的测试）发现 lofty 对 AAC 也给 `bit_depth`，改用 `lofty::mp4::Mp4File` 读 `properties().codec()` 判断。
3. `src/engine/codec.rs` 头注释改为：`//! 支持 MP3 / FLAC / WAV / AIFF / AAC / ALAC（M4A）/ OGG Vorbis。`

**Verify**：`nsh check .` → exit 0。

### Step 3：格式测试语料（单独目录）

在 `scripts/make-test-music.sh` 末尾、`echo` 之前追加第二个目录 `.nsh/tmp/zeo-player-test-formats`（与第一个目录同级，脚本已有的 `rm -rf` / `mkdir` 写法照抄），用 ffmpeg 生成每种格式 2 秒、带 `title` 标签的正弦波：

| 文件 | ffmpeg 编码参数 |
|---|---|
| `aac.m4a` | `-c:a aac -b:a 128k` |
| `alac.m4a` | `-c:a alac` |
| `raw.aac` | `-c:a aac -b:a 128k -f adts` |
| `vorbis.ogg` | `-c:a libvorbis -q:a 4` |
| `pcm.aiff` | `-c:a pcm_s16be` |

每个文件 `-metadata title="格式 <主文件名>"`（如 `格式 alac`）。`raw.aac` 是 ADTS 裸流，容器没有标签位置，标题会回退成主文件名 `raw`——测试按此断言。脚本头注释更新说明两个目录及各自用途；最后一行 `echo` 对两个目录各打印一次。

**Verify**：`sh scripts/make-test-music.sh` → 打印两行 `wrote …`；`ls .nsh/tmp/zeo-player-test-formats | wc -l` → `5`；`find .nsh/tmp/zeo-player-test-music -type f | wc -l` 与改脚本之前跑出的数量相同（改脚本前先跑一次原脚本并记下这个数）。

### Step 4：解码 + 标签 + 波形测试

在 `src/engine/codec.rs` 末尾加 `#[cfg(test)] mod tests`，用例 `decodes_every_supported_format`：

- 语料目录 `Path::new(env!("CARGO_MANIFEST_DIR")).join(".nsh/tmp/zeo-player-test-formats")`，不存在时 `eprintln!("跳过：没有 … 测试语料")` 后 `return`（与 `src/e2e.rs` 一致）。
- 对 5 个文件逐个：`Source::open` 成功；连续 `next_block()` 解出的总帧数 ≥ `sample_rate * 1.5`（2 秒文件）；`seek(1.0)` 返回值在 `0.9..=1.05`；`crate::metadata::read_meta` 的 `title` 等于 `格式 <主文件名>`（`raw.aac` 等于 `raw`）；`quality` 对 alac.m4a、pcm.aiff 为 `Lossless`，其余为 `Lossy`。
- 例外：ADTS 裸流（`raw.aac`）没有索引，symphonia 可能不支持精确 seek。若只有它的 `seek` 返回 `Err`，允许对它跳过 seek 断言，并在测试里写中文注释说明；README 不必提。其它文件 seek 失败按 STOP 处理。
- 同一用例里对每个文件调用 `crate::metadata::waveform::waveform(path)` 返回 `Ok`。
- 断言失败信息里带文件名。

**Verify**：`cargo test decodes_every_supported_format -- --nocapture` → 1 passed，输出里没有「跳过」。

### Step 5：文案与文档

- `src/ui/views/mod.rs:113` → `"添加一个包含音乐文件（MP3、FLAC、M4A、OGG 等）的文件夹，Zeo Player 会扫描并整理它们。"`
- 设置「音乐文件夹」说明里的 `MP3、FLAC 和 WAV` → `MP3、FLAC、WAV、AIFF、M4A（AAC / ALAC）、OGG`；`keys` 追加 `m4a aac alac ogg aiff`。
- `tracks.rs` 两处注释更新格式举例。
- `README.md`：`支持格式：MP3、FLAC、WAV、AIFF、M4A（AAC / ALAC）、AAC、OGG Vorbis。`；「尚未支持」改为 `- Opus 格式、逐字歌词、Linux 版本。`
- `AGENTS.md` Commands 一节「Playback e2e tests … need `.nsh/tmp/zeo-player-test-music`」后补一句：`格式解码测试用同一脚本生成的 .nsh/tmp/zeo-player-test-formats（新格式样本放这里，不要放进 zeo-player-test-music——e2e 断言那里的文件数）。`

**Verify**：`rg -n "MP3、FLAC 或 WAV|MP3、FLAC 和 WAV|Opus / OGG" src README.md` → 无输出；`nsh test .` → exit 0。

## Test plan

- 新增 `decodes_every_supported_format`（Step 4），覆盖 5 种新容器/编码的解码、seek、标签、音质、波形。
- 现有 e2e 与 UI 测试不受影响（语料目录不变）。

## Done criteria

- [ ] `nsh check .`、`nsh test .` exit 0
- [ ] `cargo test decodes_every_supported_format -- --nocapture` 通过且未跳过
- [ ] Windows 交叉检查 exit 0（或已按 STOP 条件汇报）
- [ ] `rg -n '"mp3", "flac", "wav"\]' src` → 无输出
- [ ] README / AGENTS 已更新
- [ ] `plans/README.md` 状态行更新

## STOP conditions

- 开了 `isomp4` / `aac` / `alac` / `aiff` 任一 feature 后 `nsh check .` 编译失败（symphonia 0.6.1 的 feature 名与上文摘录不符）。
- 语料数量对比（Step 3）不一致。
- 除 `raw.aac` 外有文件 seek 失败或 `Source::open` 报「不支持的格式」。

## Maintenance notes

- 新格式要同时改三处：symphonia feature、`AUDIO_EXTENSIONS`、`read_meta` 的 `quality`；再往 `zeo-player-test-formats` 加样本。
- 计划 011（标签写回）的往返测试会复用 `zeo-player-test-formats`。
- 若以后恢复 Opus：symphonia 无原生解码器，需要外部适配 crate（曾评估过 `symphonia-adapter-libopus`，bundled 构建依赖 CMake，影响交叉编译）。
