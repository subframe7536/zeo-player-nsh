# Zeo Player

本地桌面音乐播放器，通过 [nsh](https://nsh.dev) 构建的原生应用，支持 macOS 和 Windows。

支持 MP3、FLAC、WAV、AIFF、M4A（AAC / ALAC）、AAC、OGG Vorbis。

官网：https://subframe7536.github.io/zeo-player-nsh/

## 功能

### 曲库和标签

- **元数据表格**：通过多维表格管理全库的元数据，几万首歌滚动也流畅；列可显隐、拖宽、排序，单选或批量修改直接写回文件，不影响正在播的歌
- **在线补全**：支持多数据源智能聚合；编辑单曲时可搜索回填，多选也支持批量匹配（含封面与歌词）
- **元数据整理工具**：按模板批量改名（如 `{track:02d} - {artist} - {title}`，同名 `.lrc` 和封面跟着改）；自动修复 GBK / Big5 / Shift-JIS / EUC-KR 等乱码；批量导出内嵌歌词与封面，或从 `.lrc` / `.txt` 导入歌词
- **元数据历史**：保留最近 30 次标签写回的快照（含封面），随时一键还原
- **自动同步与智能歌单**：监听音乐文件夹的增删与移动，自动同步播放记录和歌单都还在；支持按歌手、专辑、流派、采样率等条件建智能歌单
- **自动分割艺术家和流派**：把流派字段分割成多个标签，自动分组，支持自定义分隔符

### 播放
- **无缝切换**：一首歌自然播完前预载下一首，中间没有停顿
- **10 段均衡器**：31 Hz–16 kHz，拖动平滑不爆音，支持防削波
- **音量优化**：暂停和切歌时音量淡出淡入，ReplayGain 统一响度，随机播放按黄金比例摊开同一歌手，拔下耳机或断开蓝牙自动暂停

### 界面和系统
- **全屏播放**：背景跟随封面主色过渡，配波形进度条和逐行歌词，歌词偏移可按单曲微调
- **键盘支持**：`⌘K` / `Ctrl+K` 呼出命令面板搜歌或跳设置；应用内与全局快捷键全部可重绑，撞键会有提示
- **丰富主题**：浅色、深色或跟随系统，主题色可选黑白、跟随封面或自定义取色，界面字体也能自由切换
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
