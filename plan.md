# NSH 桌面音乐播放器 · 产品技术计划

> 可迭代计划。权威交互版：[Canvas 计划页](https://cursor.com/canvas/store-03d9d238-ae76-4de2-b3d5-f128f4081611/236c5770-c058-40c3-85b7-53585dd157fc)（canvas_id `236c5770-c058-40c3-85b7-53585dd157fc`）。  
> 导出日期：2026-10-25（Asia/Shanghai）。

**摘要：** NSH（Rust + TSX）本地优先桌面播放器：网易云式多根曲库、playwire、Opus、全屏波形+行级 LRC；含 audio-playground 实现规格（**执行时勿再解析该仓库**）。

**来源：**

- https://nsh.dev
- https://audio.subf.dev/
- https://github.com/subframe7536/audio-playground
- https://crates.io/crates/playwire

---

## 目录

1. [执行约束与拍板摘要](#执行约束与拍板摘要)
2. [目标与非目标](#目标与非目标)
3. [参考与对标](#参考与对标)
4. [audio-playground 实现规格（执行勿再解析仓库）](#audio-playground-实现规格执行勿再解析仓库)
5. [信息架构与页面](#信息架构与页面)
6. [标签/流派分隔规则](#标签流派分隔规则)
7. [系统级音频控制](#系统级音频控制)
8. [架构分层](#架构分层)
9. [技术选型表](#技术选型表)
10. [分阶段里程碑](#分阶段里程碑)
11. [已拍板决定](#已拍板决定)
12. [架构与页面流](#架构与页面流)
13. [修订约定](#修订约定)

---

## 执行约束与拍板摘要

**执行约束：** 实现全屏播放 / 歌词 / 波形时，以本文件「audio-playground 实现规格」为准。不要再去解析 https://github.com/subframe7536/audio-playground 源码，除非 Jerry 明确要求对照更新本规格。

**开放问题已拍板（2026-10-25）：** 分隔符 `;`+trim；多根目录；外置 LRC 优先；先 macOS；仅 playwire；必须 Opus；全部内置不做插件；commit/push 前先问。

**v1 范围提醒：** 不做逐字歌词 / 卡拉 OK 擦除动画。行级 LRC 高亮 + 随 timeupdate 自动滚动即可。系统媒体中心 / 媒体键 / 锁屏控制用 playwire。

---

## 目标与非目标

为 Jerry 做一款桌面本地音乐播放器：宿主用 NSH（https://nsh.dev），Rust 负责解码/标签/输出/系统媒体会话，TSX 负责主库与播放 UI。先交付 macOS；功能全部内置，不做插件皮肤。主库对标网易云桌面布局习惯，全屏播放对标 audio-playground（行为以本规格为准）。

### 目标

- 本地曲库四视图：文件夹树 / 歌手 / 专辑 / 标签（genre 按 `;` 拆分并 trim）
- 支持多个音乐根目录并持久化
- 典型桌面布局：左侧导航 + 内容列表 + 底部迷你播放条
- 全屏 Now Playing：波形、元数据、行级 LRC（外置 `.lrc` 优先于内嵌）
- 系统级音频控制：playwire → 锁屏 / 控制中心 / 媒体键
- 基线格式含 Opus（另含 MP3 / FLAC / WAV / OGG Vorbis）
- 标签与封面用 lofty；解码用 symphonia；输出用 cpal 或 rodio；重采样按需 rubato

### 非目标（v1）

- 逐字歌词 / 软渐变卡拉 OK wipe
- 插件、签名 `.nshpkg` 皮肤、第三方可加载视图（全部内置）
- souvlaki 回退路径（现阶段只用 playwire）
- 首发 Windows / Linux（先 macOS）
- 完整克隆网易云账号、云同步、社交、评论
- 控制其它 App 的媒体会话
- MVP 阶段的 ICY / HLS / 远端流式（可记入更后阶段）

---

## 参考与对标

| 名称 | 链接 |
|------|------|
| Native Shell 文档 | https://nsh.dev |
| audio-playground 在线演示 | https://audio.subf.dev/ |
| audio-playground 源码（已摘录进本计划） | https://github.com/subframe7536/audio-playground |
| playwire | https://crates.io/crates/playwire |

### 网易云式主体布局（要学的点，不是功能全集）

- 左侧边栏：库入口（文件夹 / 歌手 / 专辑 / 标签）与固定导航
- 主区：列表或网格（曲目、专辑封面墙等），可滚动、可多列
- 底部常驻迷你播放条：封面缩略、曲名/歌手、播放控制、进度、音量入口
- 从迷你条一键进入全屏播放 / 歌词页

全屏播放手感以「audio-playground 实现规格」为唯一执行依据。

---

## audio-playground 实现规格（执行勿再解析仓库）

摘录自 https://github.com/subframe7536/audio-playground（SolidJS + audio0 + node-taglib-sharp-extend）。NSH 版用 lofty / symphonia / playwire / cpal|rodio 替换浏览器库，但状态机、歌词算法与 UI 行为应对齐本节。

### 栈与入口

| 项 | 说明 |
|----|------|
| 框架 | SolidJS；状态 createStore；PlayerProvider 单例 |
| 播放 | audio0：ZAudio，mediaSession 开启，默认音量 0.8 |
| 标签 | node-taglib-sharp-extend（NSH 改 lofty） |
| 波形 | `createWaveformGenerator(buffer)` 再 `calc(48)`，得到 length=48 的 `number[]` |
| Demo | 右上 circle-play：fetch 内置 test.ogg 成 File 后 setAudioFile；upload 选本地文件 |
| 全屏布局 | 左列约 w-96：封面 + 元数据 + 控制条；右列约 w-180：歌词；背景用封面模糊 |

### PlayerState 字段

| 字段 | 含义 |
|------|------|
| isPlaying | 是否播放中 |
| currentTime | 秒；timeupdate 时四舍五入到 0.01 |
| duration | 秒；来自元数据或 load |
| volume | 0 到 1；默认 0.8 |
| activeLyricIndex | 当前歌词行；无则 -1 |
| lyrics | LrcObj 数组 |
| metadata | AudioMetadata 或 null |
| waveform | number 数组或 null；条数 48 |
| isAudioReady | load 成功才可 play |
| isLoading / error / currentFile | 资源与错误 |

### AudioMetadata 字段（对齐 lofty）

- 必填展示：title（缺省用文件名去扩展名）、artist（缺省 Unknown Artist）、album（缺省 Unknown Album）、duration
- 可选：artwork、lyrics（内嵌原文）、bitRate、bitDepth、sampleRate、channels、codecs
- 标签扩展：genres、year、track、trackTotal、disk、diskTotal、albumArtists、composers、quality
- 参考站 artist 来自 tag.artists 用分号空格拼接；本产品库标签切分用分号加 trim，与此一致

### 加载流水线（setAudioFile）

1. **清空** — file 为空：audio.stop → cleanup → 重置 initialState  
2. **读字节** — `file.arrayBuffer()`  
3. **元数据** — `parseMetadata(name, buffer)`；若有 lyrics 则 parseLyric，选项 `ignoreShortEmptyLines=true`、`minEmptyLineDuration=3`  
4. **波形占位** — 先设 waveform 为 48 个 0.1，再异步 generate 替换为真实峰值  
5. **解码加载** — parseTrack 后 `audio.load(track)`；记录 cleanup；置 isAudioReady  

### ZAudio 事件到状态

- play → isPlaying=true；pause 与 ended → false
- volume → 写 volume
- load → 若 audio.duration 大于 0 则写 duration
- error → 打日志并 isPlaying=false
- timeupdate(time) → currentTime 四舍五入到百分之一秒，并更新 activeLyricIndex

**当前行算法：** 遍历 lyrics，取最后一个满足「lyric.time ≤ currentTime + lookahead」的下标。lookahead：currentTime 大于 1 秒时用 0.5 秒，否则用 0（避免开头跳行）。

### LRC 解析（parseLyric）— 必须复刻

| 项 | 说明 |
|----|------|
| 时间戳 | 行首 `[mm:ss.xx]` 或 `[mm:ss.xxx]`；可多个行首时间戳共享同一正文 |
| 清理 | 去掉正文中残留的尖括号或方括号卡拉 OK 碎片时间标签 |
| 排序 | 按 time 升序 |
| 元数据头 | 若前至少 3 条同一 time：合并为一条 rawContent（换行拼接），视为 leading metadata |
| 翻译行 | 相邻两条 time 完全相同且都有 content：前者 rawContent、后者 transContent |
| 空行 | ignoreShortEmptyLines + minEmptyLineDuration（参考站加载用 3 秒）：短空行跳过，连续空行合并 |
| 无时间戳回退 | 单行 → time=0；多行 → time=-1、index=-1（仅展示、不跟唱） |
| LrcObj | index、time、rawContent、transContent |

**NSH 增量：** 外置 `.lrc` 优先于内嵌 lyrics；解析算法与上述相同。空行策略：`ignoreShortEmptyLines=true`，`minEmptyLineDuration=3`。

### 歌词 UI（LyricsDisplay）

- 有 time ≥ 0 的行才进入跟唱模式；否则纯文本列表「No timing information」
- 无歌词：显示 No lyrics available
- 自动滚动：activeLyricIndex 变化且开启时，scrollTo 把当前行放到容器高度约 35% 处，behavior 为 smooth
- 容器上下大 padding（约 pt-40vh pb-60vh）便于居中
- 用户滚动打断：wheel / touchstart / mousedown / keydown → 关闭自动滚动；3 秒无操作后恢复并滚回当前行；点歌词 seek 则立即恢复
- 点击行：time ≥ 0 则 seek(lyric.time)
- 视觉：当前行高亮；非滚动时按与当前行距离加 blur（上限约 8px）；hover 显示 formatTime；空内容显示省略号脉冲
- 翻译：transContent 小号半透明跟在原文下

### 波形与进度（AudioControls）

- 48 根竖条，高度 = amp × 100%；未就绪禁用指针事件
- 进度用 CSS mask：已播段不透明，未播段约 0.4 透明（硬切百分比，不是扫光歌词）
- 无波形时退化为细进度条；hover 加高
- Seek：PointerCapture；拖动只更新 previewTime，抬起再 seek；展示时间用 preview 优先于 currentTime
- 时间格式 formatTime：分:秒两位（无小时）
- 中间小字音频信息：bitDepth-bit / sampleRate kHz / bitRate kbps / Mono|Stereo|Nch
- 音量：默认条；点击图标在静音与 lastVolume 间切换；方向键步进 0.05
- 上一首/下一首在参考站为 TODO stub；NSH 版接真实队列

### NSH 移植对照（替换层，行为不变）

| 参考站 | NSH / Rust | 阶段 |
|--------|------------|------|
| ZAudio + mediaSession | symphonia + cpal\|rodio + playwire | MVP |
| node-taglib-sharp-extend | lofty | MVP |
| createWaveformGenerator(48) | PCM 峰值或 rustfft → 48 bars | 详情页 |
| parseLyric.ts | 同算法移植到 Rust 或共享 TS | 详情页 |
| Solid store / timeupdate | NSH signal + 宿主推送进度 | MVP |
| 封面 blob URL | lofty 图 → 临时文件或字节给 Image | 详情页 |

---

## 信息架构与页面

三层壳：主库浏览 → 底部迷你条 → 全屏播放+歌词。可配置多个音乐根；四切面共用同一索引。系统媒体会话与应用内播放状态双向同步。全屏页行为见「audio-playground 实现规格」。

| # | 页面 | 说明 |
|---|------|------|
| 0 | 音乐根目录 | 多个根路径，设置里增删，持久化后参与扫盘 |
| 1 | 主库 · 文件夹 | 跨根文件树；点目录展开，点文件加入队列或立即播放 |
| 2 | 主库 · 歌手 | 按 artist 标签聚合；进入歌手后列专辑/曲目 |
| 3 | 主库 · 专辑 | 按 album 聚合；封面优先 lofty 内嵌图 |
| 4 | 主库 · 标签 | genre 按 `;` 切开并 trim，再索引 |
| 5 | 底部迷你播放条 | 播放/暂停、上一首/下一首、进度、曲名元数据、进全屏 |
| 6 | 全屏播放 + 歌词 | 对齐 ap-spec：大封面/48 条波形、元数据、行级 LRC（外置优先） |
| 7 | 系统媒体面 | macOS Now Playing / 媒体键：曲目、封面、播放态、seek（playwire） |

---

## 标签/流派分隔规则

标签视图来自 genre 字段。默认规则已拍板：只用分号分隔，并去掉每段前后空格。

| 项 | 值 |
|----|-----|
| 默认分隔符 | `;`（仅此） |
| 处理步骤 | 按 `;` 切开 → trim 每段 → 去空段 → 写入标签索引 |
| 示例 | `Rock; Indie; Dream Pop` → Rock、Indie、Dream Pop |
| 配置面 | 设置里仍可允许用户改分隔符（实现阶段）；默认出厂为 `;` |

---

## 系统级音频控制

播放器作为会话发布方。参考站 ZAudio 开了 mediaSession；NSH 用 playwire 做同等能力，不做 souvlaki 回退。

### playwire（唯一）

- 统一 API：发布 PlaybackState，回调 Event（play / pause / next / previous / seek 等）
- macOS（首发）：MPNowPlayingInfoCenter / MPRemoteCommandCenter；需 NSApplication
- 后续平台：Windows SMTC（需 HWND）、Linux MPRIS（zbus）
- crates.io：https://crates.io/crates/playwire

### 与 NSH 宿主对接（macOS 优先）

| 项 | 说明 |
|----|------|
| macOS | NSH AppKit 宿主已有 NSApplication；Rust 侧挂 MediaControls，切曲/进度 set_state |
| 封面 | lofty 抽出封面写临时文件或可访问 URL，填 artwork；勿阻塞音频回调线程 |
| 事件线程 | OS 回调经通道转回播放引擎，再同步底栏与全屏 UI |

---

## 架构分层

### Rust 宿主（核心引擎）

- 多根目录扫盘建库
- lofty：时长、标签、封面
- symphonia：解码（含 Opus；OGG Vorbis 开对应 features）
- cpal / rodio 输出；rubato 按需重采样
- playwire：系统媒体会话与媒体键
- 波形：对齐 ap-spec，输出 48 bars
- LRC：外置优先；parseLyric 算法见 ap-spec
- 向 UI 暴露与 PlayerState 等价的字段与 play/pause/seek/setVolume

### NSH TSX UI（全部内置）

- 主壳布局（侧栏 + 内容 + 底栏）
- 四库视图与设置（多根目录、分隔符）
- 迷你条与全屏播放/歌词页（行为对齐 ap-spec）
- 通过 NSH 桥调用 Rust API；不做 `.nshpkg` 插件加载

**实现偏好：** 优先在 Jerry 的 Mac mini（machineId `cba08ecf-28a8-474c-bc02-f437b13b6be4`）本地编码。每次 git commit / push 前先问 Jerry。

---

## 技术选型表

| Crate / 层 | 用途 | 阶段 |
|------------|------|------|
| NSH (Rust + TSX) | 宿主壳与全部内置 UI（先 macOS） | MVP |
| lofty | 标签、时长、封面（替 taglib-sharp） | MVP |
| symphonia | 解码；含 Opus；OGG 需 ogg+vorbis | MVP |
| opus / audiopus（若需） | Opus 解码补强 | MVP 按需 |
| cpal 或 rodio | 音频输出（替 ZAudio） | MVP |
| rubato | 重采样 | MVP 按需 |
| playwire | 系统媒体（替 mediaSession） | MVP |
| rustfft / realfft 或峰值采样 | 48-bar 波形 | 播放详情 |
| reqwest + 缓冲 MediaSource | 流式播放 | 更后可选 |

---

## 分阶段里程碑

清单状态为计划记录（导出时均为未完成）；推进后应同步改 Canvas 与本文件。

- [x] **Phase 0** — MVP：Rust 打开本地文件、解码、输出；TSX 最小播放壳（macOS）✅ 2026-10-25（Opus/OGG 按 Jerry 指示暂缓）
- [x] **Phase 0–1** — 系统媒体：playwire 发布曲目/封面/进度；媒体键与锁屏可控制播放 ✅ 2026-10-25
- [x] **Phase 1** — 多根目录持久化 + 扫盘索引 + 文件夹/歌手/专辑/标签四视图（虚拟化列表）✅ 2026-10-25
- [x] **Phase 1** — 主壳布局：侧栏 + 列表 + 底部迷你播放条 ✅ 2026-10-25
- [x] **Phase 1** — 流派按 `;` trim 拆分 ✅ 2026-10-25（含测试）
- [x] **Phase 2** — 播放详情：严格按 ap-spec（48 波形、LRC 算法与滚动、元数据）✅ 2026-10-25
- [ ] **可选** — 流式（更后）

### 推进步骤

1. **MVP 播放通路（macOS）** — 选文件 → lofty → symphonia（含 Opus）→ cpal/rodio → UI 进度  
2. **系统媒体会话** — playwire 挂上宿主；切曲/进度 set_state；媒体键回灌引擎  
3. **多根曲库四切面** — 持久化多根；索引；侧栏四视图；底栏常驻  
4. **全屏按 ap-spec** — 勿再翻 audio-playground 仓库；按本规格实现  

---

## 已拍板决定

2026-10-25 Jerry 拍板。输出栈一项仍可在实现时定。

| 类别 | 标题 | 决定 | 状态 |
|------|------|------|------|
| 产品 | 默认流派分隔符 | 仅用 `;`，切开后 trim；默认不再预置 `/` `\|` | 已拍板：`;` + trim |
| 产品 | 本地库根目录 | 支持多个音乐根目录并持久化 | 已拍板：多根目录 |
| 产品 | 外置 .lrc 匹配规则 | 外置歌词优先于内嵌（同名同目录等细节实现时再钉） | 已拍板：外置优先 |
| 产品 | 皮肤/插件时机 | 不做插件与 .nshpkg 皮肤；功能全部内置 | 已拍板：全部内置 |
| 技术 | 首发平台范围 | 先做 macOS（Mac mini 开发） | 已拍板：先 macOS |
| 技术 | 音频输出栈 | 实现时在 cpal 与 rodio 间择一；重采样按需 rubato | 待实现时再定 |
| 技术 | 系统媒体控制 crate | 只用 playwire；不做 souvlaki 回退 | 已拍板：仅 playwire |
| 技术 | Opus / 稀有格式 | 必须支持 Opus（与 MP3/FLAC/WAV/OGG Vorbis 同属基线） | 已拍板：要 Opus |
| 流程 | 提交流程确认 | Mac mini 上每次 git commit / push 前必须先问 Jerry | 已拍板：要问 |

---

## 架构与页面流

### 宿主分层

```text
多根本地文件（持久化根目录）
        │ 扫盘 / 读标签
        ▼
Rust 引擎（lofty · symphonia+Opus · cpal · playwire）
   │                         │
   │ 发布状态 / 收媒体键      │ 状态与 PCM 事件
   ▼                         ▼
macOS 媒体面            NSH 桥（API / 事件）
（Now Playing / 媒体键）        │
                         渲染与操作 ↕ 播放命令
                                ▼
                     内置 TSX UI（主库 · 迷你条 · 全屏）
```

### 页面流

```text
主库（四切面） ──选曲播放──► 迷你播放条（底栏常驻） ──展开──► 全屏播放（波形 · LRC）
                                      ▲                        │
                                      └──────── 收起 ───────────┘
```

---

## 修订约定

1. 本文件与 Canvas 计划页均可迭代；决定变更后应两边同步（或先改 Canvas 再导出）
2. 聊天里只留短链与摘要；全文以计划页 / 本文件为准
3. 全屏播放实现以「audio-playground 实现规格」为准，执行时勿再解析该仓库
4. 写代码：优先 Mac mini；每次 commit / push 前先问 Jerry
