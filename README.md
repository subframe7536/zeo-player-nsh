# Zeo Player

本地优先的桌面音频工作台与音乐播放器，纯 Rust 原生构建、极低资源占用。支持 macOS 与 Windows。

支持格式：MP3、FLAC、WAV、AIFF、M4A（AAC / ALAC）、AAC、OGG Vorbis。

> **官方介绍与在线交互演示**：欢迎访问 [website/index.html](website/index.html)（或本地浏览器打开 `website/index.html`），体验 24 字段元数据矩阵、批量重命名计算器与 10 段发烧级均衡器交互演示。

---

## 核心功能概览

### 1. 全维曲库与元数据治理（核心亮点）
- **24 字段专业元数据大表**：全库曲目标签矩阵，全面支持标题、歌手、专辑、音轨、流派、年份、指挥、作词、发行商、版权、注释、评分与 ReplayGain 增益；支持动态显隐列、列宽拖拽导轨与三态即时排序。
- **十万级零掉帧虚拟化**：仅物化视口内可见行，数万首大曲库流畅 120 FPS 惯性滚动。
- **模板批量重命名引擎**：支持使用 `{track:02d} - {artist} - {title}` 等规则安全批量重命名；同名同目录 `.lrc` 歌词与封面图侧车（jpg / png / webp）自动联动改名，落盘前具备冲突预检与非法字符过滤。
- **双向系统级文件监听**：集成 macOS FSEvents 与 Windows 文件通知，外部增删或重命名毫秒级静默同步；独创 `(大小, mtime)` 指纹追踪算法，文件移动后仍保留累计播放时长、历史记录与红心歌单。
- **复合规则智能歌单**：支持「全部 / 任一」条件组合，跨元数据、音频硬件规格（采样率、位深、无损格式）与文件系统属性动态聚合。

### 2. 发烧级回放与音频工程
- **曲间无缝对接（Gapless Prefetch）**：本曲解码尾端提前预取下一首解码器与重采样器，样本边界精确对接，现场演唱会与概念专辑绝无断崖间隔与爆音。
- **10 段高精度双极均衡器（EQ）**：采用 Robert Bristow-Johnson (RBJ) peaking biquad 滤波器（31 Hz–16 kHz，±12 dB），参数按 5ms 线性渐变平滑逼近，推子拖动与预设切换无瞬态杂音；内置 Headroom 增益前级自动防削波。
- **ReplayGain 2.0 增益标准化**：自动读取曲目与专辑级增益标签，支持防削波与自定义预放大。
- **0–5000ms 自定义淡入淡出**：切歌、暂停与跳转时音量渐变（默认 200ms），自然播完无缝接续时不淡变。
- **黄金比例随机（Smart Shuffle）**：按黄金比例算法在乱序播放时均匀摊开同一歌手的曲目，避免歌曲连续扎堆。
- **输出设备热插拔保护**：断开蓝牙或拔下有线耳机时自动平滑暂停播放。

### 3. 现代交互与沉浸视觉
- **全屏沉浸伴播**：动态提取当前封面主色调并向界面底色轻调（切歌 300ms 平滑过渡），提供波形拖拽进度条、行级歌词同步、悬停行时间戳与单曲 ±30 秒独立歌词偏移微调（持久化记忆）。
- **全局快捷命令面板（Command Palette）**：支持 `⌘K`（macOS）或 `Ctrl+K`（Windows）一键呼出，全键盘模糊搜索曲目、直达设置与触发控制。
- **原生系统托盘与锁屏控制**：macOS 菜单栏实时曲名与黑白图标，Windows SMTC 系统媒体通知与缩略图注入，双击托盘快速唤起，单击即时播放/暂停。
- **单实例与状态恢复**：双端系统级单实例互斥，重新启动自动恢复曲目、播放位置、音量与窗口几何布局。

---

## 规划与尚未支持

- Opus 格式解码支持
- 逐字卡拉OK歌词
- Linux 原生版本适配

---

## 开发者与贡献指南

欢迎为 Zeo Player 贡献代码！本项目采用纯 Rust 编写，通过原生外壳框架 [NSH](https://nsh.dev) 直接调用操作系统底层图形 API（macOS 上为 AppKit，Windows 上为 Win32 / Direct2D），拒绝 Chromium / Webview 冗余。

### 1. 开发环境要求
- **Rust**：最新稳定版（Rust 2024 edition），通过 [rustup.rs](https://rustup.rs) 安装。
- **NSH CLI**：Native Shell 命令行工具，要求版本与 `Cargo.toml` 中 `nsh-core` 保持一致（目前为 `0.4.0`）：
  ```sh
  cargo install nsh-cli
  ```

### 2. 工程目录结构
```text
zeo-player/
├── Cargo.toml            # 依赖声明 (nsh-core, symphonia, lofty, cpal, rusqlite 等)
├── AGENTS.md             # 架构核心契约与开发详细约定手册 (深入贡献必读)
├── resources/            # 图标与图形源文件 (icon.svg, icon.png, icon-windows.png)
├── website/              # 官方介绍 Landing Page (HTML5 / CSS3 / ES6 / Assets)
├── src/
│   ├── main.rs / lib.rs  # 应用入口与 NSH 组件注册
│   ├── engine/           # 音频核心子系统
│   │   ├── audio.rs      # CPAL 输出流、环形缓冲、曲间无缝预取切换
│   │   ├── codec.rs      # Symphonia 多格式音频解码与重采样
│   │   ├── eq.rs         # 10 段 RBJ Biquad 滤波器与 5ms 线性插值平滑算法
│   │   └── media.rs      # 系统级媒体会话 (macOS NowPlaying / Windows SMTC)
│   ├── metadata/         # 标签与文件治理子系统
│   │   ├── write.rs      # 24 字段标签原子安全写回 (临时文件替换，续播不中断)
│   │   ├── rename.rs     # 模板批量重命名引擎、冲突预检与侧车同步
│   │   ├── covers.rs     # 封面按需升级 (96px→400px) 与有界 LRU 缓存
│   │   ├── lyrics.rs     # LRC 歌词解析与单曲/全局偏移叠加
│   │   └── waveform.rs   # 音频波形峰值计算与缓存
│   ├── ui/               # NSH 响应式组件与界面渲染
│   │   ├── store.rs      # 全局 Signal 状态与命令中枢
│   │   ├── views/        # 页面路由 (首页、曲库、元数据大表、文件夹、智能歌单等)
│   │   ├── components/   # 原生组件库 (Slider, Select, Combobox, Table 等)
│   │   ├── frame/        # 窗口框架 (侧栏、标题栏、浮动播放条、队列抽屉)
│   │   └── now_playing/  # 全屏播放器与波形歌词伴播
│   ├── db.rs             # 内嵌 SQLite 与曲库倒排索引驱动
│   ├── playlists.rs      # 手动歌单与智能歌单条件解析器
│   ├── single_instance.rs# macOS flock 与 Windows Mutex 单实例互斥
│   └── window.rs         # 窗口生命周期、托盘图标与全局快捷键
```

### 3. 开发、调试与测试常用命令

```sh
# 本机热重载运行（代码变动自动极速增量重编译）
nsh run .

# 代码静态检查
cargo check

# 运行全套单元测试与集成测试
cargo test

# 运行前请务必格式化代码（必须保持 rustfmt 规范清洁）
cargo fmt
```

### 4. 原生二进制打包

```sh
# macOS 平台：打包生成原生 .app 应用程序包
nsh package . --target macos

# Windows 平台：打包生成独立原生安装包
nsh package . --target windows --icon resources/icon-windows.png
```

### 5. 核心开发规范与约束
在开始修改代码之前，请务必阅读根目录下的 [AGENTS.md](AGENTS.md)。以下是重点原则：
1. **跨平台兼顾**：保持 macOS 与 Windows 双端均可编译。平台特定 API 请严格置于 `cfg(target_os = ...)` 之后。
2. **严控内存占用**：绝不滥用全局大图或 `data:` URI。列表封面严格受控（96px 智能按需晋升 400px），大图全屏伴播维持 8 项有界 LRU 缓存。
3. **响应式依赖谨慎**：在编写 UI 视图时，避免在组件顶层构建期直接读取响应式 Signal，防止引起无谓的整树重建。
4. **原子落盘安全**：修改元数据或重命名时，严格走临时文件替换与冲突预检；保证当前正在播放的曲目能原位无缝续播。
5. **代码提交前**：在提交 Git commit 之前必须运行 `cargo fmt`。

---

## 开源协议

本项目采用开源许可证发布。欢迎自由学习、使用与提交贡献！
