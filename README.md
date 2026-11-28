# Zeo Player

纯 Rust 写成的本地音乐播放器，基于 [NSH](https://nsh.dev) 原生绘制（macOS AppKit / Windows Win32 + Direct2D），里面没有浏览器，也不用登录账号。

[官网](https://subframe7536.github.io/zeo-player-nsh/) · [下载最新版本](https://github.com/subframe7536/zeo-player-nsh/releases/latest)

支持 MP3、FLAC、WAV、AIFF、M4A（AAC / ALAC）、AAC、OGG Vorbis。

## 功能

### 曲库和标签
- **24 字段标签大表**：全库曲目摆在一张表里，几万首歌滚动也流畅；列可显隐、拖宽、排序，单选或批量修改直接写回文件，不影响正在播的歌
- **在线补全元数据**：支持 QM、NCM、KGM、KWM、MusicBrainz 与多源智能聚合；编辑单曲时可搜索回填，多选也支持批量匹配（含封面与歌词），先预览再写回
- **顺手的整理工具**：按模板批量改名（如 `{track:02d} - {artist} - {title}`，同名 `.lrc` 和封面跟着改）；自动修复 GBK / Big5 / Shift-JIS / EUC-KR 等乱码；批量导出内嵌歌词与封面，或从 `.lrc` / `.txt` 导入歌词
- **改错能撤销**：保留最近 30 次标签写回的快照（含封面），随时一键还原
- **自动同步与智能歌单**：监听音乐文件夹的增删与移动，文件挪了位置，播放记录和歌单都还在；支持按歌手、专辑、流派、采样率等条件建智能歌单

### 播放
- **曲间无缝**：一首歌自然播完前预载下一首，中间没有停顿
- **10 段均衡器**：31 Hz–16 kHz，拖动平滑不爆音，带防削波
- **听起来顺耳**：暂停和切歌时音量淡出淡入，ReplayGain 统一响度，随机播放按黄金比例摊开同一歌手，拔下耳机或断开蓝牙自动暂停

### 界面和系统
- **全屏播放**：背景跟随封面主色过渡，配波形进度条和逐行歌词，歌词偏移可按单曲微调
- **键盘顺手**：`⌘K` / `Ctrl+K` 呼出命令面板搜歌或跳设置；应用内与全局快捷键全部可重绑，撞键会有提示
- **随你喜欢的样子**：浅色、深色或跟随系统，主题色可选黑白、跟随封面或自定义取色，界面字体也能自由切换
- **融入系统**：菜单栏 / 托盘常驻，支持 macOS Now Playing 与 Windows SMTC 封面通知，应用内直接检查并更新

## 下载

前往 [GitHub Releases](https://github.com/subframe7536/zeo-player-nsh/releases/latest) 下载：

- **macOS**：Apple Silicon，macOS 12+（`.dmg`）
- **Windows**：x64，Windows 10 / 11（安装包 `.exe` 或便携 `.zip`）

## 尚未支持

Opus 解码、逐字歌词、Linux 版。

## 开发

需要最新稳定版 [Rust](https://rustup.rs)（2024 edition）和 [NSH CLI](https://nsh.dev)（stable 频道）。

```sh
nsh run                        # 开发运行
nsh test                       # 运行测试
nsh package . --target macos   # 打包 macOS .app
nsh package . --target windows # 打包 Windows 安装包
```

改代码前请先看 [AGENTS.md](AGENTS.md)，里面有目录结构和开发约定。
