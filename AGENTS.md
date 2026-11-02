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
- Large lists are virtualized; keep them that way. Watch memory: no data URIs, no full-size artwork in caches — the tag cache stores `has_art` only and artwork is re-read on demand (`metadata::read_artwork`); thumbnails are files at 400 px (lists), 720 px (fullscreen), 64 px pre-blurred (background). Lyrics stay out of the global cache and DB too (`metadata::read_lyrics` on demand; `engine::Meta::lyrics` keeps a bounded cache near the play position, warmed for the next 3 queue tracks together with all cover sizes). Cover/lyrics sources respect `prefer_local_covers`/`prefer_local_lyrics` (same-stem `jpg/jpeg/png/webp` / `.lrc` sidecars vs embedded); the cover cache key folds in the sidecar stamp and the prefer flag, the lyrics cache key folds in the lrc stamp and the flag, so toggles or file changes invalidate automatically.
- Persisted files (`library.json`, `playlists.json`, settings) live in the data directory from `dirs`. Keep their formats backward compatible or migrate them.
- 单行 `text()` 一律配 `.line_height(px(字号))`（leading-none）：默认行高约 1.2× 字号，中文/拉丁字形墨水在行盒里偏下约 0.75 pt，居中行里能看出来。会折行的正文/提示保持默认行高。
- `store()`（含 `store::later`、`Owner`、环境）是线程局部的，只能在根组件所在线程调；后台线程收尾要回界面侧时，在界面侧先拿 `store::poster()` 的 `Post` 把手带进线程，`post(f)` 由 `install` 的轮询循环执行（每实例一条通道，并行 UI 测试不互串）。看当前曲用 `player().state()`，不用 `store().track`。
- 文件 / 文件夹移动或改名用 `LibraryStore::move_path`，删除 / 摘除用 `remove_under`：索引、`MetaCache`、SQLite 直接换 / 清路径（不重读标签、不整库重扫）；歌单路径用 `store::move_playlist_paths` / `remove_playlist_paths_under` 同步。watcher 的单路径改名事件按（大小, mtime）指纹配对后同样走 `move_path`，配不上的才重扫。macOS 回收站走 `trash` 的 `DeleteMethod::NsFileManager`（默认的 Finder/osascript 要辅助功能权限）。
- 应用图标：`resources/icon.svg` 是源文件；`resources/icon.png` 是它的 1024px 渲染（`sips -s format png -z 1024 1024 resources/icon.svg --out resources/icon.png`），`nsh.json` 的 `icon` 与托盘（`window.rs`）都指向 PNG——NSH 打包在 Windows 上用 image crate 解码、不认 SVG（macOS 收 SVG，但为了两端一致都用 PNG）。标题栏 logo 直接嵌 `icon.svg`（`ui::components::icons::logo_svg`）。换图标改 `icon.svg` 并重渲染 PNG。

## Commands

```sh
nsh check .        # cargo check + type check
nsh test .         # unit tests + tests/ui.rs (headless UI tests)
nsh run .          # run the app with rebuild on change
nsh package . --target macos    # .nsh/out/ZeoPlayer.app (ad-hoc signed)
nsh package . --target windows  # Windows installer
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
- Memory benchmark: `ZEO_PLAYER_BENCH=1 nsh run .` and sample with `footprint <pid>`.

CI (`.github/workflows/ci.yml`) runs `nsh check` and `nsh test` on macOS and Windows for every push and PR.

## 待上游（NSH）

Workarounds live in the app; don't patch NSH crates locally. When a limitation is fixed upstream, remove the workaround and this entry.

- **解码图片缓存上限不可配置。** `nsh-platform-macos` / `nsh-platform-windows` 0.3.0 的 `src/image.rs` 里 `CACHE_BYTES` 固定为 128 MB，按最近使用淘汰。封面多时它会被填满，macOS 上 Core Animation 还持有同一批图片的副本，所以滚完整个专辑网格后 footprint 约 300 MB（本地实验改成 24 MB 后峰值 91 MB）。希望 NSH 提供缓存上限配置或在内存压力时清理。
- **没有托盘与关闭拦截的接口。** `AppOptions::keep_alive_on_close` 只在 macOS 有且 runner 不让应用设置；Windows 没有对应项，也没有托盘 API。应用用 `tray-icon` 做托盘，关闭拦截直接改系统层：macOS 替换 `windowShouldClose:`，Windows 子类化 `NshWindow` 拦截 `WM_CLOSE`。希望 NSH 提供「关闭前询问」事件与托盘接口。
- **没有窗口最小尺寸的接口。** `AppOptions` 只给初始大小，没有 min size；应用在系统层设：macOS `contentMinSize`（`src/window.rs` 的 `MIN_WIDTH`），Windows 子类化里处理 `WM_GETMINMAXINFO`。
- **没有读写窗口 frame 的接口。** `WindowFrame` / `setFrame` 只在 `runtime::desktop`（`pub(crate)`，走 JS）里，Rust 组件拿不到，也没有读当前 frame 的 API。「记住窗口大小和位置」自己实现：macOS `setFrameUsingName` / `setFrameAutosaveName`，Windows `GetWindowPlacement` / `SetWindowPlacement`（`WM_EXITSIZEMOVE` 与退出时记录）。
- **Windows 关窗后进程可能不退。** `nsh-platform-windows` 在 `nsh-ui` 线程退出时由线程局部析构释放 `Gfx`（D3D11 / D2D），而 Windows 在持有加载器锁时运行析构；Intel 驱动释放设备时等自己的工作线程退出，那个线程又要加载器锁——死锁，进程留在后台。应用在 `WM_DESTROY` 后移除托盘图标并 `TerminateProcess` 绕过。
- **Windows 标题栏按钮画得发虚。** 隐藏标题栏时 NSH 用 Direct2D 画 1 设备像素宽的线（发虚）且总叠在应用之上。应用自己画（`src/ui/frame/caption.rs`）：子类化后把标题栏样式切到 Default，NSH 就不再画。代价是没有 Snap 布局（Win + Z 仍可用），失焦时按钮不变淡。
- **`maskImage` 不绘制。** nsh-core 能解析 `maskImage` 渐变，但两个原生后端都不画。歌词边缘淡出改用每行按视口位置算透明度（`src/ui/now_playing/lyrics.rs` 的 `edge_fade`），只能整行淡，做不到行内渐隐。
- **没有 slider / select 组件。** nsh-base 0.3.0 只有 progress、switch、checkbox 等；文档里的 Slider、Select 在 NSH 主分支未发布。下拉用 `menu` + 自画触发器（`form.rs` 的 `trigger_shell`/`trigger_chevron`/`select_trigger`），滑块用 `components::slider` 自组，没有键盘调节与「可调节」无障碍角色。发布对应版本后随 CLI 一起换上。
- **组件元素不能挂 tooltip。** nsh-base 的 `ButtonElement` 等没有实现 `Interactive`，没有 `.tooltip(..)`；在外面包一层 view 挂提示（`button::tipped`）。另外 `popover` 的触发器若是 nsh-base `button`，按压被按钮吃掉、弹层打不开——队列按钮与字体下拉因此用普通 view 做触发器。
- **Windows 局部重绘下背景模糊留下亮块。** 局部重绘只清空并重画变化区域，而 `backdropFilter` 按模糊半径多取一圈上一帧的像素（那里已盖着播放条自己的半透明底色），于是每次重绘都留下一块更亮的直角矩形。Windows 上播放条不加背景模糊，用不透明 card 色（`ui::theme::bar`、`ui::frame::bar::player_bar`）。
- **右键按下不产生 PressOutside。** macOS（`RightMouseDown`）与 Windows（`WM_RBUTTONDOWN`）都只发 `ContextMenu` 事件、不发 touch Start，overlay 的 `onPressOutside` 对右键不触发——开着的菜单不会被关掉，再开一个就叠两层。应用侧互斥：所有 `menu()`/`context_menu()`/`popover()` 走 `store::menu_control()` 拿 `MenuGate`（共享 `store().menu_open` 令牌，开新挤掉旧的；popover 用 `gate.open_with(local)`/`gate.popover_change(local)`/`effect(gate.evicted(local))` 同步本地开关信号），根 view 上的兜底 `on_context_menu` 把「点在没有菜单的区域」也清掉。希望上游让右键（或在另一 overlay 外的 ContextMenu 事件）也算 PressOutside，或提供全局「只开一个菜单」。
