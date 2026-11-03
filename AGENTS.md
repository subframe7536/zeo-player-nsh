# AGENTS.md

ZeoPlayer is a local-first desktop music player written as a pure-Rust NSH (Native Shell) app for macOS (AppKit) and Windows (Win32 / Direct2D). `README.md` is user-facing — keep implementation details out of it and put them here.

## Rules

- **Ask before every `git commit` and `git push`.** Never commit or push on your own.
- `nsh-core`, `nsh-base`, `nsh-engine`, and `nsh-testing` in `Cargo.toml` must match `nsh --version` (currently 0.3.0). Bump them together with the CLI.
- Keep both platforms building. Platform code goes behind `cfg(target_os = ...)`; don't add a macOS-only path without a Windows counterpart or a documented gap.
- When behavior changes, update `README.md` (it's the user-facing feature list). Record upstream NSH limitations under "待上游（NSH）" below.
- Don't patch NSH crates locally to work around problems. Work around them in the app and document the limitation.

## Conventions

- `README.md` and code comments are in Chinese. Match the surrounding style.
- Commit messages are in English: a short summary line, then a bullet list per area (see `git log`).
- UI is in `src/ui/`; state and commands live in `src/ui/store.rs` as signals. Engine and I/O code stays out of `src/ui/` and is reached through the `crate::engine` facade (`src/engine/`).
- Domains under `src/`: `engine/` (playback facade + decode/audio threads + media session), `config/` (shared models — `TrackEntry`, `ProviderId`, global `LibraryStore`, `LibraryConfig`/`library.json`), `provider/` (track providers reached via the `provider` facade; `local/` is the only one today — new providers go in as sibling modules), `metadata/` (lofty tags, covers, lyrics, waveform caches).
- Inside `src/ui/`: `frame/` is the persistent app frame (sidebar, title bar, player bar, queue, Windows caption), `components/` is the reusable widget kit, `views/` holds the route pages and the settings dialog, `now_playing/` is the fullscreen player. Keep module-private visibility (`pub(super)` between siblings) rather than widening to `pub`.
- Large lists are virtualized; keep them that way. Watch memory: no data URIs, no full-size artwork in caches — the tag cache stores `has_art` only and artwork is re-read on demand (`metadata::read_artwork`); list covers are one file per track — 96 px for ≤48 pt slots (rows, queue, chips, bar) upgrading in place to 400 px for bigger slots (grid cells, heroes; the smaller file is pruned after the signal swaps, `covers::list_cover`/`prune_smaller`). UI asks via `store::cover_for(path, pt)`; scan pre-warm generates only 96 px and big slots upgrade lazily. Fullscreen artwork is the one data-URI exception — `covers::artwork_uri` re-reads the tags, scales to 720 px, JPEG-encodes and base64s into a `data:` URI held by an 8-entry in-memory LRU (`covers::cached_artwork`), never written to disk (both NSH backends decode `data:` off the UI thread); the fullscreen background just reuses the 96 px list-cover file, and the media session reports that same file. `show_covers` (default on) gates all list covers: `cover_for` returns `None` without creating a signal or enqueueing a job when off, so lists draw letter tiles — `components::cover::cover_initial` puts the title's first char (falling back to the filename stem, `initial_letter`) as the placeholder glyph under the image. Non-list slots bypass the gate: the player bar asks via `cover_for_always` so the current track's cover always shows, and keeps the icon-placeholder variant `cover()`. Lyrics stay out of the global cache and DB too (`metadata::read_lyrics` on demand; `engine::Meta::lyrics` keeps a bounded cache near the play position, warmed for the next 3 queue tracks together with all cover sizes). Cover/lyrics sources respect `prefer_local_covers`/`prefer_local_lyrics` (same-stem `jpg/jpeg/png/webp` / `.lrc` sidecars vs embedded); the cover cache key folds in the sidecar stamp and the prefer flag, the lyrics cache key folds in the lrc stamp and the flag, so toggles or file changes invalidate automatically.
- Persisted files (`library.json`, `playlists.json`, settings) live in the data directory from `dirs`. Keep their formats backward compatible or migrate them.
- 单行 `text()` 一律配 `.line_height(px(字号))`（leading-none）：默认行高约 1.2× 字号，中文/拉丁字形墨水在行盒里偏下约 0.75 pt，居中行里能看出来。会折行的正文/提示保持默认行高。
- `store()`（含 `store::later`、`Owner`、环境）是线程局部的，只能在根组件所在线程调；后台线程收尾要回界面侧时，在界面侧先拿 `store::poster()` 的 `Post` 把手带进线程，`post(f)` 由 `install` 的轮询循环执行（每实例一条通道，并行 UI 测试不互串）。看当前曲用 `player().state()`，不用 `store().track`。
- 单实例：`native()`（`lib.rs`，打包 runner 在 `main()` 之前调它注册组件）第一行调 `crate::single_instance::enforce()`，第二份实例在窗口创建前就退出、不会闪一下；`App()` 里也有一次幂等调用兜底。实现在 `src/single_instance.rs`：Windows 用固定名互斥体判重，第二份给第一份实例的 `NshWindow` 顶层窗口直接投递注册消息 `dev.subf.zeo-player.show`（找不到窗口退回广播；第一份在 `proc` 里经 `handle_message` 收、`show()` 自己）后 `TerminateProcess`；macOS 用临时目录 `<bundle id>.lock` 的 flock 判重（`runningApplications` 有注册延迟、同时启动互相看不见，会放行多份），拿不到锁的第二份按 pid 找更旧的实例 `activate` 它 + post 分布式通知 `dev.subf.zeo-player.show`（第一份在 `hook` 里经 `observe` 订阅、`show()` 自己——`activate` 不会把 `orderOut` 到托盘的窗口放出来）再退出（Dock / `open` 由 LaunchServices 挡，拦的是直接执行二进制或 `open -n`）。只有 debug / `cfg(test)` 不拦，设 `ZEO_PLAYER_DATA_DIR` 的正式构建也拦。
- 文件 / 文件夹移动或改名用 `LibraryStore::move_path`，删除 / 摘除用 `remove_under`：索引、`MetaCache`、SQLite 直接换 / 清路径（不重读标签、不整库重扫）；歌单路径用 `store::move_playlist_paths` / `remove_playlist_paths_under` 同步。watcher 的单路径改名事件按（大小, mtime）指纹配对后同样走 `move_path`，配不上的才重扫。macOS 回收站走 `trash` 的 `DeleteMethod::NsFileManager`（默认的 Finder/osascript 要辅助功能权限）。
- 应用图标：`resources/icon.svg` 是唯一源文件，图形满幅（Windows 惯例铺满画布）；两张 PNG 都是它的渲染产物，平台差异只在渲染时——`icon-windows.png` 直接渲 1024，`icon.png` 缩到 824 再垫透明边回 1024（macOS 惯例留边距）。`nsh.json` 的 `icon` 指 `icon.png`（macOS 打包与托盘）；Windows 打包用 `nsh package --target windows --icon resources/icon-windows.png` 覆盖，Windows 托盘按 `cfg` 嵌 `icon-windows.png`（`window.rs`）。NSH 打包在 Windows 上用 image crate 解码、不认 SVG，所以两端都出 PNG。**透明区的 RGB 必须从一张 `rx=0` 的满底渲染里并回来**：SVG 渲出的透明像素是 `00 00 00 00`（透明黑），缩到任务栏尺寸时黑 RGB 会双线性渗进边缘半透明像素，四角出灰线（NSH 打包用 image crate 按通道独立缩放、不预乘 alpha）。渲染命令（Windows 没有 sips，用 `pnpm dlx sharp-cli` 渲 SVG；ffmpeg 已用于测试音频）：
  ```sh
  sed 's/rx="[0-9.]*"/rx="0"/' resources/icon.svg > .nsh/tmp/icon-square.svg
  sips -s format png -z 1024 1024 .nsh/tmp/icon-square.svg --out .nsh/tmp/icon-square.png   # macOS
  sips -s format png -z 1024 1024 resources/icon.svg --out .nsh/tmp/icon-normal.png
  pnpm dlx sharp-cli -i .nsh/tmp/icon-square.svg -o .nsh/tmp/icon-square.png                # Windows
  pnpm dlx sharp-cli -i resources/icon.svg -o .nsh/tmp/icon-normal.png
  ffmpeg -i .nsh/tmp/icon-square.png -i .nsh/tmp/icon-normal.png -filter_complex \
    "[1:v]format=rgba,alphaextract[a];[0:v]format=rgb24[rgb];[rgb][a]alphamerge[out]" \
    -map "[out]" resources/icon-windows.png
  ffmpeg -i .nsh/tmp/icon-square.png -i resources/icon-windows.png -filter_complex \
    "[1:v]scale=824:824,format=rgba,pad=1024:1024:100:100:color=black@0,alphaextract[a];[0:v]format=rgb24[rgb];[rgb][a]alphamerge[out]" \
    -map "[out]" resources/icon.png
  ```
  标题栏 logo 直接嵌 `icon.svg`（`ui::components::icons::logo_svg`）。换图标改 `icon.svg` 并重渲染两张 PNG。

## Commands

```sh
nsh check .        # cargo check + type check
nsh test .         # unit tests + tests/ui.rs (headless UI tests)
nsh run .          # run the app with rebuild on change
nsh package . --target macos    # .nsh/out/ZeoPlayer.app (ad-hoc signed)
nsh package . --target windows --icon resources/icon-windows.png  # Windows installer（满幅图标）
```

- Dev builds (`nsh run`, debug `cargo run`) use the repo's `.nsh/dev-data` instead of the system data dir; `ZEO_PLAYER_DATA_DIR` overrides everything.
- Release `.app` is ad-hoc signed; distributing to other Macs needs a Developer ID re-sign + `xcrun notarytool` notarization + `xcrun stapler staple`.

- Run `nsh check .` and `nsh test .` before reporting a change as done.
- On Windows, the tests need libclang (QuickJS bindings in the headless test runner): install LLVM and set `$env:LIBCLANG_PATH` to its `bin` directory (the one with `libclang.dll`, e.g. `C:\Program Files\LLVM\bin`) before `nsh test .`.
- Playback e2e tests and track-dependent UI tests need `.nsh/tmp/zeo-player-test-music` in the repo (`sh scripts/make-test-music.sh`, requires ffmpeg). Without it they skip silently, so generate it when touching audio, lyrics, waveform, or the player UI.
- `#[ignore]` tests use the real library / filesystem and share global state — run them one at a time:
  `cargo test --release -- --ignored scan_reports_progress` / `watcher_rescans` / `real_track_waveform`.
- Cross-check Windows from a Mac when touching platform code:
  `brew install llvm lld && cargo install --locked cargo-xwin`, then
  `PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH" cargo xwin check --target x86_64-pc-windows-msvc`.
- Memory benchmark: `ZEO_PLAYER_BENCH=1 nsh run .` (also `scroll`/`hidden`/`close` modes; `hidden` self-reports `priv`/`ws` to stderr every 30s). Bench code compiles only under `cfg(any(debug_assertions, test))` — release builds exclude it entirely and the env vars are inert. Sample externally with `footprint <pid>` on macOS or Task Manager "Commit size" on Windows.

CI (`.github/workflows/ci.yml`) runs `nsh check` and `nsh test` on macOS and Windows for every push and PR.

## 待上游（NSH）

Workarounds live in the app; don't patch NSH crates locally. When a limitation is fixed upstream, remove the workaround and this entry.

- **解码图片缓存上限不可配置。** `nsh-platform-macos` / `nsh-platform-windows` 0.3.0 的 `src/image.rs` 里 `CACHE_BYTES` 固定为 128 MB，按最近使用淘汰，只计 CPU 像素（4B/px）；每个 `Frame` 画过一次后还持有一份 D2D 位图（再 ~128 MB），另外 `OVERSCAN=1`（渲染窗口 = 3× 视口）里的备用列表单元格各留一个 `Rc<Frame>`（位图+像素 ~1.3MB/张），这些都算在缓存的 128MB 账外。实测：1664 张封面的曲库滚完整个专辑网格后进程稳定在 ~560MB（其中图片侧 ~400MB）。希望 NSH 提供缓存上限配置、内存压力时清理，并在单元格转成 spare 时释放其图片引用。
- **没有托盘与关闭拦截的接口。** `AppOptions::keep_alive_on_close` 只在 macOS 有且 runner 不让应用设置；Windows 没有对应项，也没有托盘 API。应用用 `tray-icon` 做托盘，关闭拦截直接改系统层：macOS 替换 `windowShouldClose:`，Windows 子类化 `NshWindow` 拦截 `WM_CLOSE`。希望 NSH 提供「关闭前询问」事件与托盘接口。
- **没有窗口最小尺寸的接口。** `AppOptions` 只给初始大小，没有 min size；应用在系统层设：macOS `contentMinSize`（`src/window.rs` 的 `MIN_WIDTH`），Windows 子类化里处理 `WM_GETMINMAXINFO`。
- **没有读写窗口 frame 的接口。** `WindowFrame` / `setFrame` 只在 `runtime::desktop`（`pub(crate)`，走 JS）里，Rust 组件拿不到，也没有读当前 frame 的 API。「记住窗口大小和位置」自己实现：macOS `setFrameUsingName` / `setFrameAutosaveName`，Windows `GetWindowPlacement` / `SetWindowPlacement`（`WM_EXITSIZEMOVE` 与退出时记录）。
- **Windows 关窗后进程可能不退。** `nsh-platform-windows` 在 `nsh-ui` 线程退出时由线程局部析构释放 `Gfx`（D3D11 / D2D），而 Windows 在持有加载器锁时运行析构；Intel 驱动释放设备时等自己的工作线程退出，那个线程又要加载器锁——死锁，进程留在后台。应用在 `WM_DESTROY` 后移除托盘图标并 `TerminateProcess` 绕过。
- **Windows 标题栏按钮画得发虚。** 隐藏标题栏时 NSH 用 Direct2D 画 1 设备像素宽的线（发虚）且总叠在应用之上。应用自己画（`src/ui/frame/caption.rs`）：子类化后把标题栏样式切到 Default，NSH 就不再画。代价是没有 Snap 布局（Win + Z 仍可用），失焦时按钮不变淡。
- **`maskImage` 不绘制。** nsh-core 能解析 `maskImage` 渐变，但两个原生后端都不画。歌词边缘淡出改用每行按视口位置算透明度（`src/ui/now_playing/lyrics.rs` 的 `edge_fade`），只能整行淡，做不到行内渐隐。
- **没有 slider / select 组件。** nsh-base 0.3.0 只有 progress、switch、checkbox 等；文档里的 Slider、Select 在 NSH 主分支未发布。下拉用 `menu` + 自画触发器（`form.rs` 的 `trigger_shell`/`trigger_chevron`/`select_trigger`），滑块用 `components::slider` 自组，没有键盘调节与「可调节」无障碍角色。发布对应版本后随 CLI 一起换上。
- **组件元素不能挂 tooltip。** nsh-base 的 `ButtonElement` 等没有实现 `Interactive`，没有 `.tooltip(..)`；在外面包一层 view 挂提示（`button::tipped`）。另外 `popover` 的触发器若是 nsh-base `button`，按压被按钮吃掉、弹层打不开——队列按钮与字体下拉因此用普通 view 做触发器。
- **Windows 局部重绘下背景模糊留下亮块。** 局部重绘只清空并重画变化区域，而 `backdropFilter` 按模糊半径多取一圈上一帧的像素（那里已盖着播放条自己的半透明底色），于是每次重绘都留下一块更亮的直角矩形。Windows 上播放条不加背景模糊，用不透明 card 色（`ui::theme::bar`、`ui::frame::bar::player_bar`）。
- **没有单实例机制。** 桌面应用惯例是重复启动时激活已有实例而不是再开一份；NSH 不管这个，多开会各自拉起引擎与曲库（数据目录还被并发写）。应用自己在系统层做（`src/single_instance.rs`）：Windows 固定名互斥体 + 注册消息，macOS 临时目录文件锁（flock）+ 分布式通知。希望 NSH 提供单实例选项（如 `AppOptions::single_instance`）并把「已有实例被请求激活」暴露成事件，让应用决定恢复哪个窗口。
- **应用图标不能按平台给。** `nsh.json` 的 `icon` 全平台一张方形 PNG，各平台只做转换（macOS → icns、Windows → 缩几档嵌 exe），没有按 target 分别配图标的入口；而两端的图标惯例冲突——macOS 图形占 824/1024 留边距，Windows 满幅铺满，同一张图必有一端显小或显大。应用在 `nsh package . --target windows` 时用 `--icon resources/icon-windows.png` 传满幅渲染（两张 PNG 都由 `icon.svg` 渲出，见上面「应用图标」条）。希望 NSH 支持按平台给图标（如 `"icon": {"macos": ..., "windows": ...}`）。
- **右键按下不产生 PressOutside。** macOS（`RightMouseDown`）与 Windows（`WM_RBUTTONDOWN`）都只发 `ContextMenu` 事件、不发 touch Start，overlay 的 `onPressOutside` 对右键不触发——开着的菜单不会被关掉，再开一个就叠两层。应用侧互斥：所有 `menu()`/`context_menu()`/`popover()` 走 `store::menu_control()` 拿 `MenuGate`（共享 `store().menu_open` 令牌，开新挤掉旧的；popover 用 `gate.open_with(local)`/`gate.popover_change(local)`/`effect(gate.evicted(local))` 同步本地开关信号），根 view 上的兜底 `on_context_menu` 把「点在没有菜单的区域」也清掉。希望上游让右键（或在另一 overlay 外的 ContextMenu 事件）也算 PressOutside，或提供全局「只开一个菜单」。
