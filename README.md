# Zeo Player

本地桌面音乐播放器，通过 [nsh](https://nsh.dev) 构建的原生应用，支持 macOS 和 Windows。

支持 MP3、FLAC、WAV、AIFF、M4A（AAC / ALAC）、AAC、OGG Vorbis。

官网：https://subframe7536.github.io/zeo-player-nsh/

## 功能

**界面和系统**

- 全屏播放页：背景跟随封面主色、波形进度条，歌词偏移可按单曲微调
- `⌘K` / `Ctrl+K` 命令面板，键盘搜索曲目、跳转设置
- 快捷键全部可重绑，冲突时给出提示；支持可配置的全局快捷键（窗口最小化到托盘也生效，默认不绑定）
- 菜单栏 / 系统托盘常驻，Windows 支持 SMTC 媒体通知；关闭窗口时选择最小化到托盘或退出，可勾选「记住选择」不再询问
- 根据封面自动切换主题色

**曲库和标签**

- 元数据表格：24 个元数据字段，可批量修改，列可显隐、拖宽、排序，数万首曲目滚动流畅
- 按模板批量重命名，如 `{track:02d} - {artist} - {title}`；同名 `.lrc` 歌词与封面图自动联动改名，改名前检查冲突
- 自动监听文件夹变动，外部增删、移动文件即时同步；文件移动后仍能识别，播放记录与歌单保留
- 智能歌单：按标题、艺术家、专辑、标签、采样率等条件组合筛选

**播放**

- 10 段均衡器（31 Hz–16 kHz），拖动平滑无爆音，带防削波
- 切换歌曲淡入淡出
- 更优的播放队列随机算法
- ReplayGain 音量标准化
- 拔出耳机或蓝牙断开时自动暂停

## 尚未支持

- Opus 解码
- 逐字歌词
- Linux 版

## 开发

GUI 基于 [NSH](https://nsh.dev)，需要最新 stable Rust，并安装跟踪 nightly 频道的 NSH CLI：

macOS:

```sh
curl -fsSL https://nsh.dev/install.sh | sh -s -- --nightly
```

Windows:

```sh
$env:NSH_VERSION = 'nightly'
irm https://nsh.dev/install.ps1 | iex
```

### 常用命令行

```sh
nsh run                                          # 开发运行
nsh test                                         # 测试
nsh package . --target macos                     # macOS .app
nsh package . --target windows --icon resources/icon-windows.png
```

改代码前请先读 [AGENTS.md](AGENTS.md)，包含目录结构与开发约定
