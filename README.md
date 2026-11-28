# Zeo Player

纯 Rust 构建的本地桌面音乐播放器，基于 [NSH](https://nsh.dev) 原生渲染（macOS AppKit / Windows Win32 + Direct2D），无内嵌浏览器，无需登录账号。

[官网](https://subframe7536.github.io/zeo-player-nsh/) · [下载最新版本](https://github.com/subframe7536/zeo-player-nsh/releases/latest)

支持格式：MP3、FLAC、WAV、AIFF、M4A（AAC / ALAC）、AAC、OGG Vorbis。

## 主要功能

### 曲库与元数据
- **24 字段元数据大表**：全库标签虚拟滚动浏览，支持筛选、排序、列显隐与拖宽，单选/批量编辑原子写盘
- **在线元数据自动匹配**：内置 QM、NCM、KGM、KWM、MusicBrainz 与多源智能聚合，支持单曲搜索回填及批量自动匹配（含封面与歌词）
- **批量治理工具**：按模板批量重命名（如 `{track:02d} - {artist} - {title}`，联动同名 `.lrc` 与封面）、自动修复编码乱码（GBK / Big5 / Shift-JIS / EUC-KR 等）、批量导出歌词/封面、外部导入 `.lrc`/`.txt` 歌词
- **标签历史回滚**：保留最近 30 批写回快照（含封面），随时一键还原
- **实时同步与智能歌单**：自动监听文件夹增删与移动（移动后保留播放记录与歌单）；支持多条件组合的智能歌单

### 播放与音质
- **曲间无缝播放**：提前预载下一首，自然切歌零停顿
- **10 段均衡器**：31 Hz–16 kHz，拖动平滑无爆音，带防削波保护
- **听感细节**：自定义淡入淡出、黄金比例随机播放（避免同歌手扎堆）、ReplayGain 响度标准化、耳机拔出 / 蓝牙断开自动暂停

### 界面与系统
- **全屏播放页**：封面主色动态背景、波形进度条、逐行歌词聚焦与单曲偏移微调
- **键盘优先**：`⌘K` / `Ctrl+K` 全局命令面板；应用内与全局快捷键全部可重绑并检测冲突
- **外观定制**：浅色 / 深色跟随系统，主题色支持黑白、跟随封面或自定义取色，支持自由切换界面字体
- **系统集成**：菜单栏 / 系统托盘常驻，macOS Now Playing / Windows SMTC 媒体控制，应用内一键检查并原地自更新

## 下载

前往 [GitHub Releases](https://github.com/subframe7536/zeo-player-nsh/releases/latest) 获取预编译安装包：

- **macOS**：Apple Silicon，macOS 12+（`.dmg`）
- **Windows**：x64，Windows 10 / 11（安装程序 `.exe` 或便携 `.zip`）

## 尚未支持

Opus 解码、逐字歌词、Linux 版本。

## 本地开发

需要最新稳定版 Rust（2024 edition）与 [NSH](https://nsh.dev) CLI（stable 频道）：

```sh
# macOS
curl -fsSL https://nsh.dev/install.sh | sh

# Windows (PowerShell)
irm https://nsh.dev/install.ps1 | iex
```

常用命令：

```sh
nsh run                                                          # 开发运行
nsh test                                                         # 运行测试
nsh package . --target macos                                     # 打包 macOS .app
nsh package . --target windows --icon resources/icon-windows.png # 打包 Windows 安装包
```

修改代码前请先阅读 [AGENTS.md](AGENTS.md) 了解架构分层与开发约定。
