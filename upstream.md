# 待上游（NSH）

Workarounds live in the app; don't patch NSH crates locally. When a limitation is fixed upstream, remove the workaround and the entry. New limitations go here (not `AGENTS.md`); code comments point here as `upstream.md`.

下列条目已于 2026-11-10 对照上游源码复核，在 pin 的 nightly `36b9168`（`nsh-*` 0.4.0）仍然成立；标注「待实机复核」的条目需在 Windows 真机上确认。

## 测试驱动（nsh-testing）

- **无头驱动缓存的 `environment.window` 不从 `config.viewport` 起种。** `Headless::start` 把 `config.environment` 原样存进驱动侧副本（`window` 留默认 0×0），引擎侧启动时才 `environment.window = config.viewport`；此后每次主窗口状态上报（恢复位置 / 隐藏 / 显示 / 焦点等 `Report::Main`）都整份回发，把引擎的 `env.window` 盖成 0——`window().width` 的消费方（列宽断点等）全当窄窗口且不再恢复。测试侧绕法：`Headless::start` 后先按 viewport 调一次 `set_viewport` 回种（`tests/ui.rs`、`tests/layout_probe.rs`），并在测试 `library.json` 里关掉 `remember_frame`（曲库配置进程共享，窗口位置会互相写 / 恢复而触发上报）。希望上游把驱动侧副本的 `window` 起种为 `config.viewport`。
- **滑块的 `label:` 要落在响应件本身，且零尺寸视图不上屏。** 无头测试的 `drag` / `press` 把手势送进「命中视图自身 + 祖先」的链——`label:` 标在滑块外面的包裹视图时，盖在里面的 `nsh_base::slider` 是**子级**不在链里、收不到手势（EQ 推子曾因此拖不动）；`accessibility_label` 要标给原生件（落在 thumb 上）。另外零尺寸视图不进场景树，`thumb` 不能 `size(0)`——隐形也要留命中区（24pt），否则无障碍角色与 `label:` 匹配一起没。

## 引擎与控件（nsh-core / nsh-base）

- **DataTable 行内嵌套按压件在 AppKit 拿不到指针命中。** 行单元格里的内嵌 pressable 视图先被行的按压应答抢（`DataTablePress` 的 `column` 仍按位置报出）——早先元数据表复选框列在格内放 `nsh_base::checkbox`，真机鼠标点击落不到它。绕法（`ui-view-metadata` 的 `table/table.rs`）：选择列不放按压件，单元格是数据驱动的「✓」文本字形，`on_row_press` 里 `column == 0` 统一切换（顺带撤掉了拖慢横向滚动的 `render_cell`）；表头列的复选框（不在行内）用真 `nsh_base::checkbox` 正常。**另外上游 `render_header_cell` 对滚动区列表头不落渲染内容**（`title_content` 的 `defs.get(def)?` 索引域只覆盖固定区 → 标签槽空着）——「全选」因此放工具栏，表头走默认路径。
- **`DataTable` 列宽调整逐像素触发全局重排，且没有全高竖线导轨。** 上游 `data_table::controller.rs` 在 `resize_move` 时每一微小像素位移均直接 `set_width` 并 `bump_layout`，导致当前视口内全部行与列单元格在拖拽期间每帧重新计算 `left`/`w`/`app_class` 并重算文本（数万单元格密集计算导致拖拽卡顿）；此外 `classes.resize_handle` 仅作用于表头单元格内（36pt 高度），在表头溢出裁剪下无法穿透至表体呈现贯穿全表的全高竖线。ZeoPlayer 在应用侧通过 `resizable_columns(false)` 屏蔽上游实时拖拽，改由 `render_header_cell` 在表头单元格右缘挂载手势把手：拖拽期间仅响应式更新跨越全表的绝对定位竖向导轨位置（`guide_line_x`，零表体重排重绘，120 FPS 顺滑无阻），鼠标松开时才统一计算最终列宽并一次性更新 `col_widths`（支持双击把手还原默认宽度，列宽下限 50pt）。导轨位置由把手右缘反推（`e.page + (14 - e.location)`——把手 14pt 宽贴表头单元格右缘即列线，落点在把手内的坐标随抓握点变，须用 location 校正而非直接取指针 x；把手内 2pt 指示条设 `pointer_events(None)` 保证落点 target 恒为把手）。
- **Select 弹层的 cover 对齐只在首次布局时采样一次，滚动收敛后不重采。** `select` 的弹层把选中行盖在触发器上（`core/overlay.rs` 的 `covering`）：对齐位置 `at` 缓存只认弹层尺寸与 cover 行在父级的位置，而虚拟列表的 `scrollToIndex` 用估算行高分几帧收敛（`core/list.rs` 的 pin）——cover 行首次布局早于收敛时 `at` 采在中间的滚动位置上，滚动到位后弹层仍停在错位处。应用侧在 `on_open` 后让触发器宽度在 ±1pt 间抖动几帧（`ui-components` 的 `form::cover_realign`；弹层宽绑触发器实测宽，尺寸一变 `covering` 就重采），弹性触发器走 select 根的 `min_w`（`cover_realign_flex`）。nightly 的 #1236 把缓存键扩成 cover 视图在父级的整个 frame（位置 + 尺寸），但明确「滚动弹层内容不重摆」——这里错位来自滚动偏移收敛，不在其覆盖内，绕法保留。希望上游在 pin 收敛后（或 cover 的可见位置变化时）重采 `at`。
- **右键按下不产生 PressOutside。** macOS（`RightMouseDown`）与 Windows（`WM_RBUTTONDOWN`）都只发 `ContextMenu` 事件、不发 touch Start，overlay 的 `onPressOutside` 对右键不触发——开着的菜单不会被关掉，再开一个就叠两层。应用侧互斥：所有 `menu()`/`context_menu()`/`popover()` 走 `ui-store` 的 `menu::menu_control()` 拿 `MenuGate`（共享 `store().menu_open` 令牌，开新挤掉旧的；popover 用 `gate.open_with(local)`/`gate.popover_change(local)`/`effect(gate.evicted(local))` 同步本地开关信号），根 view 上的兜底 `on_context_menu` 把「点在没有菜单的区域」也清掉。希望上游让右键（或在另一 overlay 外的 ContextMenu 事件）也算 PressOutside，或提供全局「只开一个菜单」。
- **组件元素不能挂 tooltip。** nsh-base 的 `ButtonElement` 等没有实现 `Interactive`，没有 `.tooltip(..)`（nightly 的 #1299 只修了 TS `<Button tooltip>` 的宿主属性转发，Rust 面不变）；在外面包一层 view 挂提示（`ui-components` 的 `button::tipped`）。另外 `popover` 的触发器若是 nsh-base `button`，按压被按钮吃掉、弹层打不开——队列按钮与歌词偏移数值因此用普通 view 做触发器。
- **系统级全局快捷键只有 TS API。** 窗口外也生效的 `GlobalShortcut`（`DesktopCommand::RegisterGlobalShortcut`，macOS Carbon hot key / Windows `RegisterHotKey`）只在 `nsh-js` 的 `nsh/desktop` 模块露面，`element/desktop.rs` 没有对应 Rust 函数——纯 Rust 应用注册不了，退而求其次用 `on_key`（仅窗口聚焦时生效；窗口隐藏到托盘后只有系统媒体键可用）。希望上游给 Rust 组件补 `global_shortcut` 注册入口。已提 feature request：[longbridge/nsh#1489](https://github.com/longbridge/nsh/issues/1489)。

## macOS

- **普通 view 的 `filter: blur()` 不绘制。** `nsh-platform-macos`（0.4.0，nightly 36b9168 复核未变）只在 `image` 元素解码位图时烘入滤镜（`src/image.rs` 读 `style.effects.filters` 与 `ImageProp::BlurRadius` 进 `Decoding.blur`）；容器 view 的 `e.filters` 只认 `Opacity` / `Brightness` / `DropShadow`，`Filter::Blur` 被忽略。全屏播放的模糊背景因此在 macOS 用 `image().blur_radius(..)`（`crates/ui-now-playing/src/lib.rs`；层上同时挂的 `st.blur` 在那里被忽略、无副作用）。希望上游让任意 view 的内容模糊生效（或明确文档限定 image）。

## Windows

- **标题栏按钮画得发虚。** 隐藏标题栏时 NSH 用 Direct2D 画 1 设备像素宽的线（发虚）且总叠在应用之上。应用自己画（`crates/ui-frame/src/caption.rs`）：子类化后把标题栏样式切到 Default，NSH 就不再画。代价是没有 Snap 布局（Win + Z 仍可用），失焦时按钮不变淡。已提 longbridge/nsh#1469。
- **`image().blur_radius(..)` 画不满视图框。** `nsh-platform-windows`（0.4.0，nightly 36b9168 该文件只改了染色图 DPI）的 `src/image.rs` `draw_one` 走 `with_effects` 分支时把 `DrawImage` 画在缩放变换里，实测位图落成视图中央一小块自然尺寸的方块而不是铺满 `objectFit` 算出的矩形。Windows 的全屏播放背景因此不用 `blur_radius`，模糊挂回容器 view 的 `st.blur()`（该路径走 `decor::draw_layered` 离屏合成，正常）。希望上游修正效果分支的绘制矩形。
- **不画 `maskImage`。** nightly 起 macOS 会画了（#930，`visual.rs` 的 `set_mask_image`），`nsh-platform-windows` 仍不读 `mask_images`（36b9168 复核：全 crate 无 `mask_images` 引用）。歌词边缘淡出两端统一用每行按视口位置算透明度（`crates/ui-now-playing/src/lyrics/anim.rs` 的 `edge_fade`），只能整行淡、做不到行内渐隐；Windows 也画了之后可换成滚动容器上的渐变 `maskImage`。
- **局部重绘下背景模糊留下亮块。** 局部重绘只清空并重画变化区域，而 `backdropFilter` 按模糊半径多取一圈上一帧的像素（那里已盖着播放条自己的半透明底色），于是每次重绘都留下一块更亮的直角矩形。Windows 上播放条不加背景模糊，用不透明 card 色（`ui_store::theme::bar`、`ui-frame` 的 `player_bar`）。
- **`window()` 报的是物理像素、渲染却按系统缩放放大（175% DPI 实测，待实机复核）。** 本机 168 DPI（175%）下 `GetClientRect`=1486 px，`window()` 也报 1486 pt（正确逻辑宽应约 849 pt），而绘制出来的 232 pt 侧栏实测约 465 px（≈2 px/pt）——布局按物理像素算、绘制按缩放点算，两边不一致，整页向右溢出约一倍，宽度断点等 `window()` 消费方全部失准。与具体页面无关（每个界面都放大），应用侧无法绕开；先记症状与测量口径，待上游统一 `env.window` / surface scale 的口径后再复核。（36b9168 复核：`Surface::resized` 仍按 `pixels / scale` 换算 `env.window`，自 #20 起即如此——症状机制与代码表象对不上，需实机复核是 DPI 感知时序还是别的路径。）

## 第三方依赖

- **playwire 的 Windows SMTC 缩略图只收远程 URI，不收本地文件（非 NSH，playwire 2.1.0 复核未变）。** `SmtcBackend::apply_metadata` 走 `RandomAccessStreamReference::CreateFromUri`，WinRT 只收 http(s) / ms-appx / ms-appdata——`file://` 进去缩略图恒空；若 `CreateFromUri` 对 `file:` 抛错，`apply_metadata` 还会在 `Update()` 之前退出、标题 / 艺术家也不提交。应用在媒体线程绕开它（`crates/engine/src/media.rs` 的 `smtc_thumbnail`）：`GetForWindow(hwnd)` 拿同一 SMTC 单例的 `DisplayUpdater`，`StorageFile` + `CreateFromFile` 直投缓存封面并替它 `Update()`（顺带把没提交的元数据落盘）。上游若给出本地文件 / 流式缩略图入口（`CreateFromFile` 或 `CreateFromStream`），撤掉这块。

## 平台接口与打包

- **托盘接口只有 TS 面、画不了菜单栏文字。** nightly 起 NSH 有托盘（#988，`DesktopCommand::SetTray` / `nsh_core::desktop::Tray`：图标、tooltip、菜单、单击），但 Rust 组件没有入口（`element::desktop_command` 是 `pub(crate)`，`element::desktop` 里没有 `set_tray`——36b9168 复核仍未暴露），`Tray` 也没有 macOS 菜单栏图标旁的标题文字。应用继续用 `tray-icon` 做托盘（`crates/shell/src/tray.rs`）；窗口的隐藏 / 显示 / 激活已迁到上游接口（`element::desktop::main_window()` 的 `hide` / `show` / `focus`），macOS 的「激活应用 + 还原来回最前」由 `focus` 覆盖，`window/macos.rs` 只剩 reopen 挂钩、托盘创建与内存 / 驻留采样。macOS Dock 图标的「重新打开」上游也不上报，仍靠替换 `applicationShouldHandleReopen:`（`crates/shell/src/window/macos.rs` 的 `should_reopen`）。已提 feature request longbridge/nsh#1470：Rust 的 `set_tray` + macOS 菜单栏 title + Dock reopen 事件 + 可选的自绘状态栏内容（`ImageSource`，用于托盘旁显示当前曲名）。
- **没有单实例机制。** 桌面应用惯例是重复启动时激活已有实例而不是再开一份；NSH 不管这个（36b9168 复核：全 workspace 无 single-instance API），多开会各自拉起引擎与曲库（数据目录还被并发写）。应用自己在系统层做（`crates/shell/src/single_instance.rs`）：Windows 固定名互斥体 + 注册消息，macOS 临时目录文件锁（flock）+ 分布式通知。已提 feature request longbridge/nsh#1468：单实例选项 + 「已有实例被请求激活」事件（带 argv / cwd），让应用决定恢复哪个窗口。
- **应用图标不能按平台给。** `nsh.json` 的 `icon` 全平台一张方形 PNG（36b9168 复核：`nsh-cli` 的 `config.icon` 仍是单个 `PathBuf`），各平台只做转换（macOS → icns、Windows → 缩几档嵌 exe），没有按 target 分别配图标的入口；而两端的图标惯例冲突——macOS 图形占 824/1024 留边距，Windows 满幅铺满，同一张图必有一端显小或显大。应用在 `nsh package . --target windows` 时用 `--icon resources/icon-windows.png` 传满幅渲染（两张 PNG 都由 `icon.svg` 渲出，见 `AGENTS.md`「应用图标」条）。已提 feature request longbridge/nsh#1472：`"icon": {"macos": ..., "windows": ...}` 按平台给图，单字符串保留为全平台简写。
