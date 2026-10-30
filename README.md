# Zeo · NSH 桌面音乐播放器

本地优先的桌面音乐播放器，纯 Rust 的 [Native Shell（NSH）](https://nsh.dev) 应用：界面、状态与引擎都编译为原生代码，不带 JavaScript 运行时。支持 macOS（AppKit）与 Windows（Win32 / Direct2D）。设计与算法依据见 [plan.md](./plan.md)。

支持格式：MP3、FLAC、WAV（Opus / OGG 暂缓）。

## 功能

- **曲库**
  - 多个根文件夹（持久化到数据目录的 `library.json`：macOS `~/Library/Application Support/Zeo`，Windows `%APPDATA%\Zeo`）。
  - 用系统文件夹选择器或拖入添加文件夹；扫描时显示实时进度（已读 / 总数、当前文件），可「扫描更新」或「重建曲库」（清空标签与封面缓存后重读）。
  - 文件夹监视（可开关）：增删改名音频文件后，变化停止 1.5 秒自动重扫；文件按 修改时间 + 大小 + inode + ctime 识别，内容一变就重读。
  - 流派标签分隔符可自定义（多个分隔符，修改后立即重新拆分，不必重扫），带预览。
- **四个视图**（列表虚拟化，大库不卡）
  - 文件夹：顶层列出所有根文件夹，逐级进入；列包括封面、标题 / 歌手、专辑、文件大小、时长，文件夹行显示总大小与总时长；窗口变窄时依次收起专辑、大小、序号列。
  - 歌手、专辑（封面网格）、标签（流派）。
  - 封面在后台生成 400 px JPEG 缩略图并缓存（macOS `~/Library/Caches/Zeo/covers`，Windows `%LOCALAPPDATA%\Zeo\covers`）。
- **播放**：symphonia 解码 → 声道转换 → rubato 重采样 → cpal 输出；精确 seek；队列上一首 / 下一首；playwire 接入系统 Now Playing、媒体键与锁屏控制。
- **全屏播放**
  - 模糊封面作背景；左列封面、曲名与「⋯」菜单（在访达中显示、拷贝路径）、歌手 / 专辑、质量与流派标签。
  - 56 根波形进度：按 ap-spec `createWaveformGenerator` 计算（第一声道约 2000 块 RMS，以 95 分位为参考峰值，映射到 0.1–0.9）；播放头所在的那根只亮到播放头；可点击或拖动 seek。
  - 行级歌词（外置同名 `.lrc` 优先，其次内嵌）：自动滚动、手动滚动后 3 秒恢复跟随、点击行 seek、按距离变暗变模糊、翻译行；悬停时文字有高亮，行最右侧浮出时间戳。
  - 键盘：Space 播放 / 暂停，← → 快退 / 快进 5 秒，↑ ↓ 音量，Esc 收起。
- **字体**：界面字体可选系统字体或任一已安装字体（fontdb 列出，懒加载，离开设置页即释放）；macOS 上可导入 .ttf / .otf / .ttc（只在 Zeo 内注册，Windows 见「待上游」）。
- **LRC 解析**：多时间戳、卡拉 OK 碎片清理、元数据头合并、翻译行、短空行跳过、连续空行合并、无时间戳回退。

## 结构

```
nsh.json           应用配置："rust": "."、"root": "App"、"theme": "theme.json"
theme.json         主题（shadcn cssVars 格式，浅色 / 深色）
src/lib.rs         crate 入口，导出根组件 App
src/ui/            界面：mod（外壳、侧栏）、store（信号与命令）、views（四视图与设置）、
                   bar（底部播放条）、full（全屏播放与歌词）、parts、icons、theme
src/engine.rs      引擎门面：Player / Library / Meta
src/audio.rs       解码线程与 cpal 输出        src/codec.rs     symphonia 封装
src/library.rs     曲库、扫描进度、流派拆分    src/watcher.rs   文件夹监视（notify / FSEvents）
src/metadata.rs    lofty 标签与缓存            src/covers.rs    封面缩略图
src/lyrics.rs      LRC 解析                    src/waveform.rs  波形
src/media.rs       playwire 系统媒体会话        src/e2e.rs       端到端测试
src/fonts.rs       字体列表与导入
App.test.tsx       nsh test 的界面测试（无头打开 Rust 根组件）
scripts/           make-test-music.sh：端到端测试语料
.github/workflows/ CI：macOS 与 Windows 上 check / test / package
```

## 内存

Release 包（`nsh package`）实测 footprint（macOS，949 首、全部带内嵌封面、868 张专辑）：

| 场景 | 内存 |
|---|---|
| 启动扫描完、停在文件夹页 | 23 MB |
| 打开专辑页（首屏封面） | 58 MB |
| 打开全屏播放并连续切 20 首 | 136 MB |
| 专辑封面网格从头滚到底（最坏情况） | 约 300 MB |

应用侧的做法：
- 标签缓存不存封面原图（每首约 170 KB）：只记 `has_art`，用到时 `metadata::read_artwork` 读一次即释放。
- 列表用 400 px 缩略图文件，全屏页用 720 px 文件，背景用预先模糊好的 64 px 小图（不在窗口尺寸上实时模糊）；都不用 data URI。

剩下的大头在 NSH 里，见下文「待上游」。

测量方法：`ZEO_BENCH=1` 启动时自动依次打开各视图、滚动专辑网格、播放并切歌（`src/ui/mod.rs` 的 `bench`；`ZEO_BENCH_NO_SCROLL=1` 跳过滚动），步骤打印到 stderr，期间用 `footprint <pid>` 取样。

## 待上游（NSH）

- **解码图片缓存上限不可配置。** `nsh-platform-macos` / `nsh-platform-windows` 0.3.0 的 `src/image.rs` 里 `CACHE_BYTES` 固定为 128 MB，按最近使用淘汰。封面多时它会被填满，macOS 上 Core Animation 还持有同一批图片的副本，所以滚完整个专辑网格后 footprint 约 300 MB。本地实验把它改成 24 MB 后，同一场景峰值为 91 MB，滚动时没有可见的重新加载。希望 NSH 提供配置项（例如 `AppOptions` 或 `nsh.json` 里的图片缓存上限），或在内存压力时清理该缓存。
- **Windows 无运行时注册字体的公开接口。** Windows 后端内部有 `add_font`（DirectWrite 进程内字体集合），但没有导出；Rust 应用只能在编译期通过 `assets!()` 带字体。因此 Zeo 的「导入字体」目前只在 macOS（CoreText，进程内注册）提供，Windows 上可以选择系统已安装的字体。

## 工具链

按 [nsh.dev 的安装说明](https://nsh.dev/docs/installation)：

```sh
curl -fsSL https://nsh.dev/install.sh | sh     # 安装到 ~/.nsh，链接 ~/.local/bin/nsh
nsh --version                                   # nsh 0.3.0
```

Windows 用 PowerShell：`irm https://nsh.dev/install.ps1 | iex`（装到 `%LOCALAPPDATA%\nsh`）。

另需 Rust（[rustup](https://rustup.rs)），以及各平台的构建工具：macOS 为 Xcode 命令行工具；Windows 为 Visual Studio Build Tools（“使用 C++ 的桌面开发”与 Windows SDK）和 LLVM（libclang，`nsh test` 的无头 runner 需要；不在 `C:\Program Files\LLVM` 时设 `LIBCLANG_PATH`）。`Cargo.toml` 中 `nsh-core` / `nsh-base` 的版本须与 CLI 一致（当前 `0.3.0`），升级 CLI 时一起改。

## 开发

```sh
nsh run .                          # 本机窗口（macOS AppKit / Windows Win32）；改代码重建并重启
nsh check .                        # cargo 检查与类型检查
nsh test .                         # Rust 单元测试 + App.test.tsx 界面测试
nsh package . --target macos       # 生成 .nsh/out/Zeo.app（在 macOS 上）
nsh package . --target windows     # 生成 Windows 安装包（在 Windows 上）
```

在 Mac 上可交叉检查 Windows 构建（不运行）：

```sh
brew install llvm lld && cargo install --locked cargo-xwin
PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH" cargo xwin check --target x86_64-pc-windows-msvc
```

需要真实曲库或文件系统的测试默认跳过，请分别运行（它们共享全局曲库状态，不要一起跑）：

```sh
cargo test --release -- --ignored scan_reports_progress   # ~/Music 扫描进度与重建
cargo test --release -- --ignored watcher_rescans         # 文件夹监视：增、改、删、关闭
cargo test --release -- --ignored real_track_waveform --nocapture
```

`src/e2e.rs` 的播放端到端测试需要 `/tmp/zeo-test-music` 测试语料（`sh scripts/make-test-music.sh` 用 ffmpeg 生成），没有时跳过。

## 平台差异

- 标题栏：自绘，macOS 交通灯在左上，Windows 的窗口按钮在右上（`title_bar()` 报告其占位，全屏页的收起按钮会让开）。
- 路径：文件夹树按系统分隔符（Windows `\`）拆分；根目录用 `dunce::canonicalize` 规范化，避免 `\\?\` 逐字路径；拖入的 `file:///C:/…` 转为 `C:\…`。
- 文件识别：修改时间 + 大小，再加 Unix 的 inode / ctime 或 Windows 的创建时间。
- 「在访达中显示」在 Windows 上为在资源管理器中选中文件；文件夹选择器用 rfd（NSOpenPanel / IFileDialog）。
- 文件夹监视：macOS 用 FSEvents，Windows 用 ReadDirectoryChangesW（notify 默认后端）。

## 打包分发

`nsh package . --target macos` 生成 `.nsh/out/Zeo.app`（arm64，id `dev.subf.zeo`，版本见 `nsh.json`），为 ad-hoc 签名，本机可直接打开。分发给其它 Mac 需用自己的证书重签并公证：

```sh
codesign --force --deep --sign "Developer ID Application: YOUR NAME (TEAMID)" .nsh/out/Zeo.app
xcrun notarytool submit Zeo.zip --apple-id … --team-id … --password … && xcrun stapler staple Zeo.app
```

## 暂缓

- Opus / OGG Vorbis（symphonia 0.6 无 Opus 解码）。
- 逐字歌词、空行省略号脉冲动画。
- Linux（GTK 4）尚未适配：文件夹选择器、导入字体、标题栏未验证。
