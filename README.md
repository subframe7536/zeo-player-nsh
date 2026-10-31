# Zeo Player · NSH 桌面音乐播放器

本地优先的桌面音乐播放器，纯 Rust 的 [Native Shell（NSH）](https://nsh.dev) 应用：界面、状态与引擎都编译为原生代码，不带 JavaScript 运行时。支持 macOS（AppKit）与 Windows（Win32 / Direct2D）。设计与算法依据见 [plan.md](./plan.md)。

支持格式：MP3、FLAC、WAV（Opus / OGG 暂缓）。

## 功能

- **曲库**
  - 多个根文件夹（持久化到数据目录的 `library.json`：macOS `~/Library/Application Support/Zeo Player`，Windows `%APPDATA%\Zeo Player`）。
  - 用系统文件夹选择器或拖入添加文件夹；扫描时显示实时进度（已读 / 总数、当前文件），可「扫描更新」或「重建曲库」（清空标签与封面缓存后重读）。
  - 文件夹监视（可开关）：增删改名音频文件后，变化停止 1.5 秒自动重扫；文件按 修改时间 + 大小 + inode + ctime 识别，内容一变就重读。
  - 流派标签分隔符可自定义（多个分隔符，修改后立即重新拆分，不必重扫），带预览。
  - 曲目元数据（封面之外的全部：标签、技术参数、内嵌歌词）存在数据目录的 SQLite 数据库 `zeo-player.db`（`tracks` 表，按文件指纹失效）。启动时先用数据库里上次的曲库显示列表，随后的扫描只重读变了的文件，扫描结束删掉已不存在的文件；「重建曲库」同时清空这张表。数据库打不开时照常工作，只是不保存。
- **搜索**：侧栏顶部的搜索框匹配曲目的标题、歌手、专辑和文件名；点框外任意位置即失焦（Space 等快捷键恢复可用）。
- **四个视图**（列表虚拟化，大库不卡）
  - 文件夹：顶层列出所有根文件夹，逐级进入；列包括封面、标题 / 歌手、专辑、文件大小、时长，文件夹行显示总大小与总时长；窗口变窄时依次收起专辑、大小、序号列。
  - 歌手、专辑（封面网格）、标签（流派）。
  - 封面在后台生成 400 px JPEG 缩略图并缓存（macOS `~/Library/Caches/Zeo Player/covers`，Windows `%LOCALAPPDATA%\Zeo Player\covers`）。
- **歌单**（侧栏「歌单」区直接列出，存到数据目录的 `playlists.json`）
  - 手动歌单：在任意曲目上点右键（或全屏页「⋯」）→「添加到歌单」，可选已有歌单或「新建歌单…」；歌单页里右键「从歌单中移除」。文件被移走后条目保留，回到曲库时重新出现。
  - 智能歌单：按条件从曲库自动聚合，曲库变化时跟着更新。条件可选标签（标题、歌手、专辑、专辑歌手、流派、年份）、音频（格式、时长、采样率、比特率、声道数）、文件（文件夹、文件名、大小、修改时间）；文本比较不分大小写，流派有多个值时任一满足即可；组合方式为「全部」或「任一」，可排序并限制数量，编辑时实时显示匹配数。
  - 侧栏「+」新建；右键歌单可重命名 / 编辑条件、删除（删除前确认，不动文件）。
  - 固定的「我喜欢」排在歌单最前，不能改名或删除；播放条与全屏页上的心形按钮把当前曲目加入（最新的在最前）或移出。`playlists.json` 里它是带 `"favorite": true` 的手动歌单，旧文件读入时自动补上。
- **播放**：symphonia 解码 → 声道转换 → rubato 重采样 → cpal 输出；精确 seek；队列上一首 / 下一首；playwire 接入系统 Now Playing、媒体键与锁屏控制。
  - 淡入淡出：播放、暂停、跳转、切歌时音量渐变（设置里选 关闭 / 0.1–1.5 秒，默认 0.2 秒）；在输出回调里逐帧推进，一首自然播完接下一首时不淡变。
  - 记住上次播放：曲目、位置和队列存在 `zeo-player.db` 的 `state` 表（换曲、播放 / 暂停、位置变了 2 秒以上、队列变了时写入，退出时最多丢 2 秒进度）。下次启动装载这首并暂停在上次的位置；文件已不在时跳过。
  - 播放条与全屏页的控制按钮一致：循环模式、上一首、播放 / 暂停、下一首、喜欢（红色心形描边，喜欢后变成红色实心）；播放键最大，上一首 / 下一首次之，两侧最小。上一首 / 下一首是线框图标；播放键没有底色，图标在播放三角与暂停竖条之间形变（两块四边形的顶点在 320 ms 内移动过去，绘制在 canvas 上、顶点绑定动画值；加粗的圆角描边让角更圆，外轮廓大小不变；「减弱动态效果」时直接切换）。
- **全屏播放**（打开时淡入并从下方滑入，收起时淡出并向下滑出；收起途中再打开会接着展开；系统「减弱动态效果」时不动画）
  - 模糊封面作背景（切歌时旧背景留到新封面生成好，新图在上面淡入，不会闪黑；新歌没有封面时旧图淡出）；左列封面、曲名与「⋯」菜单（在访达中显示、拷贝路径）、歌手 / 专辑、质量与流派标签。
  - 56 根波形进度：按 ap-spec `createWaveformGenerator` 计算（第一声道约 2000 块 RMS，以 95 分位为参考峰值，映射到 0.1–0.9）；播放头所在的那根只亮到播放头；可点击或拖动 seek。
  - 行级歌词（外置同名 `.lrc` 优先，其次内嵌）：当前行停在歌词区垂直居中处，打开全屏时直接定位到这一行（不从开头滚过去）；自动滚动、手动滚动后 3 秒恢复跟随、点击行 seek（直接滚到点的那一行，不先回到原高亮行）；对齐方式可在设置 →「歌词对齐」选左对齐 / 居中 / 右对齐（默认左对齐，存在 `library.json` 的 `lyrics_align`）；原文 30 pt 粗体，译文是原文的 2/3（20 pt）、颜色始终不变；当前行原文比译文亮，其余行原文比译文淡；靠近歌词区顶部和底部的行逐渐淡出；当前行与前后一句清晰，再远每行多模糊 1.5 pt，最多 8 pt（鼠标移入歌词区时 300 ms 内逐渐变清晰，移出时逐渐恢复）；歌词区上方让出标题栏、下方留 32 pt，滚动的歌词不会盖住窗口按钮与收起按钮；悬停时文字有高亮，行最右侧浮出时间戳（右对齐时在最左侧）。
  - 收起按钮在标题栏这一行靠右：Windows 上紧挨窗口按钮，macOS 上在窗口右边缘。
  - 键盘：Space 播放 / 暂停，← → 快退 / 快进 5 秒，↑ ↓ 音量，Esc 收起。
- **字体**：界面字体可选系统字体或任一已安装字体（font-enumeration 向系统字体服务查询：macOS CoreText，Windows DirectWrite；懒加载，离开设置页即释放）。
- **托盘**（macOS 菜单栏 / Windows 通知区域）：第一次点窗口的关闭按钮时在系统警告框里选择「最小化到托盘」或「退出」，之后记住（设置 →「关闭窗口时」可改）。最小化后界面整个拆掉，只留播放引擎与队列；托盘菜单显示当前曲目，可播放 / 暂停、上一首 / 下一首、打开窗口、退出（Windows 上左键单击图标打开窗口）。
- **播放队列**：播放条右侧的队列按钮在上方弹出面板（nsh-base `popover`），列出当前队列，点一首跳过去；点外面或 Esc 关闭。
- **LRC 解析**：多时间戳、卡拉 OK 碎片清理、元数据头合并、翻译行、短空行跳过、连续空行合并、无时间戳回退。
- **组件**：界面控件建在 nsh-base 0.3.0 上，外观按 shadcn/ui（布局、间距、圆角、字号同 shadcn，颜色取自 `theme.json`，浅色 / 深色自动切换）：
  - 按钮（`src/ui/button.rs`）：`button` 加 shadcn 的变体（default、secondary、outline、ghost、destructive）与尺寸（xs / sm / default / lg 与 icon 系列），有聚焦环；播放条与全屏页的图标按钮悬停 500 ms 出现提示（`tooltip`）。
  - 设置页的选项（淡入淡出、歌词对齐、关闭窗口时）是 `radio_group` / `radio`（`src/ui/radio.rs`）。
  - 歌单编辑弹窗是 `dialog`（Esc 或点遮罩关闭），里面的输入框、下拉触发器、「全部 / 任一」分段（`tabs` / `tab`）在 `src/ui/form.rs`；删除歌单与关闭窗口的确认用 `alert_dialog`，右键菜单与下拉列表用 `menu`。
  - 仍是自己组合的：进度条与音量条（0.3.0 没有 slider）、下拉列表的选项（0.3.0 没有 select，用 menu 代替）、播放 / 暂停形变图标、波形、虚拟化列表与封面网格、歌词滚动、Windows 窗口按钮、全屏页背景与开合动画。
- **图标与滑块**：基础图标用 [Lucide](https://lucide.dev)（lucide-static 1.66.0，ISC 许可，路径数据在 `src/ui/icons.rs`），播放 / 暂停键是自己画的形变图标，Windows 窗口按钮按系统按钮的尺寸画。进度条与音量条是同一个滑块（`parts::slider`）：鼠标移入或拖动时在当前位置出现圆形 thumb，轨道不缩放。

## 结构

```
nsh.json           应用配置："rust": "."、"root": "App"、"theme": "theme.json"
theme.json         主题（shadcn cssVars 格式，浅色 / 深色）
src/lib.rs         crate 入口，导出根组件 App
src/ui/            界面：mod（外壳、侧栏）、store（信号与命令）、views（四视图与设置）、
                   playlists（歌单区、歌单页、右键菜单、编辑弹窗）、
                   bar（底部播放条）、queue（播放队列）、full（全屏播放与歌词）、
                   button / radio / form（nsh-base 组件的 shadcn 外观）、parts、icons、theme
src/engine.rs      引擎门面：Player / Library / Meta
src/audio.rs       解码线程与 cpal 输出        src/codec.rs     symphonia 封装
src/library.rs     曲库、扫描进度、流派拆分    src/watcher.rs   文件夹监视（notify / FSEvents）
src/metadata.rs    lofty 标签与缓存（存 SQLite） src/covers.rs    封面缩略图
src/lyrics.rs      LRC 解析                    src/waveform.rs  波形
src/playlists.rs   歌单模型、智能规则、持久化  src/db.rs        SQLite：元数据与播放状态
src/media.rs       playwire 系统媒体会话        src/e2e.rs       端到端测试
src/fonts.rs       已安装字体列表            src/window.rs    关闭拦截、托盘
tests/ui.rs        界面测试（nsh_testing::Headless 无头运行根组件）
scripts/           make-test-music.sh：端到端测试语料
.github/            CI：推送 / PR 在 macOS 与 Windows 上 check + test；手动运行打包并建草稿 Release
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

测量方法：`ZEO_PLAYER_BENCH=1` 启动时自动依次打开各视图、滚动专辑网格、播放并切歌（`src/ui/mod.rs` 的 `bench`；`ZEO_PLAYER_BENCH_NO_SCROLL=1` 跳过滚动），步骤打印到 stderr，期间用 `footprint <pid>` 取样。

## 待上游（NSH）

- **解码图片缓存上限不可配置。** `nsh-platform-macos` / `nsh-platform-windows` 0.3.0 的 `src/image.rs` 里 `CACHE_BYTES` 固定为 128 MB，按最近使用淘汰。封面多时它会被填满，macOS 上 Core Animation 还持有同一批图片的副本，所以滚完整个专辑网格后 footprint 约 300 MB。本地实验把它改成 24 MB 后，同一场景峰值为 91 MB，滚动时没有可见的重新加载。希望 NSH 提供配置项（例如 `AppOptions` 或 `nsh.json` 里的图片缓存上限），或在内存压力时清理该缓存。
- **没有托盘与关闭拦截的接口。** NSH 0.3.0 的 `AppOptions::keep_alive_on_close` 只在 macOS 有、生成的 runner 也不让应用设置，Windows 上没有对应项，也没有托盘 API。Zeo Player 用 `tray-icon` 做托盘，关闭拦截直接改系统层：macOS 替换 NSH 窗口代理的 `windowShouldClose:`，Windows 子类化 `NshWindow` 拦截 `WM_CLOSE`。希望 NSH 提供「关闭前询问应用」的事件与托盘 / 菜单栏图标接口。
- **Windows 关窗后进程可能不退。** `nsh-platform-windows` 0.3.0 在 `nsh-ui` 线程退出时由线程局部析构释放 `Gfx`（D3D11 / D2D 设备），而 Windows 在持有加载器锁时运行这些析构。Intel 驱动（`igd10umt64xe`）释放设备时等自己的工作线程退出，那个线程退出又要加载器锁，于是死锁：窗口已关，进程留在后台，从终端启动时终端也不返回。Zeo Player 在主窗口 `WM_DESTROY` 后移除托盘图标并 `TerminateProcess` 绕过。希望 NSH 在线程退出前（`run_loop` 返回后）显式释放图形设备。
- **Windows 标题栏按钮画得发虚。** 隐藏标题栏时，最小化 / 最大化 / 关闭按钮由 `nsh-platform-windows` 0.3.0 `src/chrome.rs` 的 `draw()` 用 Direct2D `DrawLine` / `DrawRectangle` 画成 1 设备像素宽的线，叠在应用内容之上，应用无法替换或隐藏。实际效果：最大化方框的右边和下边比左上两边浅，关闭的 × 是发灰的抗锯齿斜线。Zeo Player 改为自己画（`src/ui/caption.rs`，与其它图标一样用 SVG）：窗口子类化后接管 `WM_NCCALCSIZE`（去掉系统标题栏，算法同 NSH 的隐藏标题栏）、`WM_NCHITTEST`（顶边缩放）与 `WM_NCACTIVATE`，再把 NSH 的标题栏样式切到 Default，它就不画按钮了。代价是没有 Snap 布局（鼠标停在最大化按钮上弹出的贴靠面板，要对按钮区返回 `HTMAXBUTTON` 并自己处理非客户区鼠标消息），Win + Z 仍可用；窗口失去焦点时按钮也不变淡。希望 NSH 改用系统图标字体绘制（Windows 11 的 Segoe Fluent Icons、Windows 10 的 Segoe MDL2 Assets：`E921` 最小化、`E922` 最大化、`E923` 还原、`E8BB` 关闭）或允许应用自己画按钮并保留 Snap 布局。
- **`maskImage` 不绘制。** nsh-core 0.3.0 能解析 `maskImage` 的渐变（`Effects::mask_images`），但 `nsh-platform-macos` / `nsh-platform-windows` 0.3.0 都不画它。歌词区上下边缘的淡出因此不能用一个渐变遮罩完成，Zeo Player 按每行在视口里的位置算透明度（`src/ui/full.rs` 的 `edge_fade`），只能整行淡，做不到一行内部的渐隐。希望 NSH 在原生后端实现 `maskImage`（macOS 用 `CALayer.mask` 的渐变层，Windows 用 Direct2D 不透明度蒙版）。
- **没有 slider / select 组件。** nsh-base 0.3.0（crates.io 与 GitHub release 上的最新版）只有 progress、switch、checkbox 等，没有滑块和下拉选择；nsh.dev 文档里的 Slider、Select 在 NSH 主分支上，还没发布。下拉选择暂用 `menu` 加自画的触发器。Zeo Player 用 `parts::slider` 自己组合（轨道 + 填充 + 悬停时出现的 thumb，拖动用 responder 事件），没有键盘调节和无障碍的「可调节」角色。等发布了包含 Slider / Select 的版本，随 CLI 一起升级后换上。
- **组件元素不能挂 tooltip。** nsh-base 0.3.0 的 `ButtonElement` 等组件元素没有实现 `Interactive`，没有 `.tooltip(..)`；Zeo Player 在外面包一层 view 挂提示（`button::tipped`）。另外 `popover` 的触发器若是 nsh-base `button`，按压被按钮吃掉、弹层打不开，队列按钮因此用普通 view 做触发器。

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
nsh test .                         # 单元测试 + tests/ui.rs 界面测试（等同 cargo test）
nsh package . --target macos       # 生成 `.nsh/out/Zeo Player.app`（在 macOS 上）
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

`src/e2e.rs` 的播放端到端测试和 `tests/ui.rs` 里依赖曲目的用例需要仓库内 `.nsh/tmp/zeo-player-test-music` 测试语料（`sh scripts/make-test-music.sh` 用 ffmpeg 生成），没有时跳过。

## 平台差异

- 标题栏：自绘、透明（40 pt），浮在侧栏与内容区顶部，图标与名称相对整个窗口居中；macOS 交通灯在左上。Windows 的最小化 / 最大化（还原）/ 关闭按钮在右上，由应用画（SVG 图标，悬停变色，关闭为红底白字；全屏播放页上为白色），没有 Snap 布局；顶边可拖动改变高度，全屏页的收起按钮排在它们左边。全屏播放盖住整个窗口，只留窗口按钮。
- 路径：文件夹树按系统分隔符（Windows `\`）拆分；根目录用 `dunce::canonicalize` 规范化，避免 `\\?\` 逐字路径；拖入的 `file:///C:/…` 转为 `C:\…`。
- 文件识别：修改时间 + 大小，再加 Unix 的 inode / ctime 或 Windows 的创建时间。
- 「在访达中显示」在 Windows 上为在资源管理器中选中文件；文件夹选择器用 rfd（NSOpenPanel / IFileDialog）。
- 文件夹监视：macOS 用 FSEvents，Windows 用 ReadDirectoryChangesW（notify 默认后端）。

## 打包分发

`nsh package . --target macos` 生成 `.nsh/out/Zeo Player.app`（arm64，id `dev.subf.zeo-player`，版本见 `nsh.json`），为 ad-hoc 签名，本机可直接打开。分发给其它 Mac 需用自己的证书重签并公证：

```sh
codesign --force --deep --sign "Developer ID Application: YOUR NAME (TEAMID)" ".nsh/out/Zeo Player.app"
xcrun notarytool submit ZeoPlayer.zip --apple-id … --team-id … --password … && xcrun stapler staple "Zeo Player.app"
```

## 暂缓

- Opus / OGG Vorbis（symphonia 0.6 无 Opus 解码）。
- 逐字歌词、空行省略号脉冲动画。
- Linux（GTK 4）尚未适配：文件夹选择器、标题栏未验证。
