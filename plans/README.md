# Issue 执行计划（索引）

- 基线：`f6e6821`（main，2026-11-02），NSH 0.3.0。
- 范围：GitHub 全部 open issue（#1 #3–#12）。#2 已关闭。
- 每份计划自包含，按下表顺序执行；执行者完成后更新本表状态列。
- 所有计划都**不提交、不推送**（AGENTS.md：commit / push 前必须征得用户同意），改动留在工作区等待审阅。

## 已确认的决策（2026-11-02）

| Issue | 决策 |
|---|---|
| #12 侧栏优化 | **待定**，本轮不出计划。下面的计划不要改侧栏 / 最小窗口宽度。 |
| #8 元数据管理 | 不做独立窗口；**把「文件夹」视图右侧改成标签表格**（多选 + 右侧编辑面板），另加全屏页的单曲编辑弹窗。 |
| #5 桌面歌词 / 迷你模式 | 先不做，等上游（NSH 多窗口 / 透明窗口）。 |
| #6 播放格式 | 放弃 APE / WMA / DSD / 独占 / **Opus**（不为单一格式引入 C 依赖）；做 symphonia 现成的 AAC / ALAC / M4A / OGG Vorbis / AIFF。 |
| #10 i18n | 简中 + 英文，默认跟随系统，设置里可钉住。放在最后做一次性迁移（含前面计划新增的字符串）。 |
| #9 快捷键 | 先不做。 |
| #11 系统音频控制 | 复现：Win11 上 zeo 播放 → 暂停 → AIMP 播放 → 暂停 → zeo 播放，系统媒体控件仍控制 AIMP。修复会话抢占；封面只做诊断。 |
| #7 主题/设置优化 | 淡入淡出改数字输入框；输出设备变化时是否暂停；均衡器；ReplayGain（**值随扫描入 tracks 表**）。 |
| #1 首页 | 首页作为默认页；播放 > 60% 或 > 30 秒计一次播放（两阈值可配置）。 |
| #4 托盘 | 只做托盘内的实时状态（无多窗口，不做信息/控制小窗）。 |

## 执行顺序与状态

| Plan | 标题 | Issue | 优先级 | 规模 | 依赖 | 状态 |
|---|---|---|---|---|---|---|
| 001 | [拆分 settings.rs（纯搬移）](001-split-settings.md) | #7 | P1 | S | — | DONE |
| 002 | [淡入淡出时长改数字输入框](002-fade-duration-input.md) | #7 | P2 | S | 001 | DONE |
| 003 | [支持 AAC/ALAC/M4A/OGG/AIFF](003-more-audio-formats.md) | #6 | P1 | M | — | DONE |
| 004 | [输出设备变化：重建输出流 + 可选暂停](004-output-device-change.md) | #7 | P1 | M | 001 | DONE |
| 005 | [ReplayGain（值入 tracks 表）](005-replaygain.md) | #7 | P2 | M | 001, 004（真实文件测试样本依赖 003） | DONE |
| 006 | [10 段均衡器](006-equalizer.md) | #7 | P2 | M | 001, 004, 005 | DONE |
| 007 | [歌词提前 / 延后（每曲 + 全局）](007-lyrics-offset.md) | #3 | P2 | S–M | 001（设置节落点）, 005（仅 db 版本顺序） | DONE |
| 008 | [Windows 系统媒体会话抢占](008-media-session-preempt.md) | #11 | P1 | M | 004 | TODO |
| 009 | [托盘实时播放状态](009-tray-live-status.md) | #4 | P3 | S | 001 | TODO |
| 010 | [播放统计 + 首页](010-home-and-play-stats.md) | #1 | P2 | L | 001, 005, 007（db 版本顺序） | TODO |
| 011 | [标签写回核心 + 单曲编辑弹窗](011-tag-write-and-edit-dialog.md) | #8 | P2 | M–L | 003 | TODO |
| 012 | [文件夹视图改为标签表格 + 批量编辑](012-folders-tag-table.md) | #8 | P2 | L | 011 | TODO |
| 013 | [i18n：简中 + 英文](013-i18n.md) | #10 | P2 | L | 全部前序计划 | TODO |

状态取值：TODO | IN PROGRESS | DONE | BLOCKED（一句原因）| REJECTED（一句理由）

## 依赖说明

- 001 先行：002/004/005/006/007/009/010 都要往设置里加节，先把 1780 行的 `settings.rs` 按分类拆开，避免每个计划都在同一个大文件里冲突。
- 004 → 005/006/008：004 把输出流的创建抽成可重建的函数，ReplayGain / EQ 要挂进输出回调，008 要在暂停时暂停输出流，都依赖这次重构。
- 005 → 006：两者都改 `output_callback` 的增益链，顺序执行避免冲突。
- 005 → 007 → 010：数据库表结构版本号按执行顺序递增（当前 `SCHEMA_VERSION = 3`；005 升到 4（ReplayGain 列），007 升到 5，010 升到 6）。若顺序改变，各计划里写的版本号改为「执行时的当前值 + 1」。
- 003 → 011：标签写回的往返测试要覆盖 003 新增的容器（m4a / ogg / aiff）。
- 013 最后：一次性把所有界面字符串（含前面计划新增的）迁到 `tr!`，免得每个计划都写两种语言。

## 通用验证命令（各计划内也已列出）

| 用途 | 命令 | 成功标准 |
|---|---|---|
| 编译 + 类型检查 | `nsh check .` | exit 0 |
| 测试 | `nsh test .` | exit 0 |
| 测试语料（动音频/歌词/播放 UI 时必需） | `sh scripts/make-test-music.sh` | 打印 `wrote N files` |
| Windows 交叉检查（动平台代码时） | `PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH" cargo xwin check --target x86_64-pc-windows-msvc` | exit 0 |

## 考虑过但不做

- 独立元数据窗口、托盘信息/控制小窗、桌面歌词、迷你模式：NSH 0.3 没有多窗口 / 透明置顶窗口接口；按用户决定等上游。
- APE / WMA / DSD 解码、WASAPI 独占 / CoreAudio hog 模式、Opus：用户决定放弃（Opus 需要外部解码适配 crate，曾评估 `symphonia-adapter-libopus`，bundled 构建依赖 CMake）。
- 全局 / 自定义快捷键（#9）：用户决定先不做。
- 媒体会话封面换高清图：按 AGENTS.md，列表封面升档到 400px 后会裁掉 96px 文件（`covers::prune_smaller`），而全屏背景与媒体会话都复用 96px 文件，需要单独设计；008 只做诊断。
