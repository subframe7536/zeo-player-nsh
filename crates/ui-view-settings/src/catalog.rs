use super::*;

/// 分类图标。
pub(crate) fn pane_glyph(pane: Pane) -> Icon {
    match pane {
        Pane::Library => Icon::Folder,
        Pane::Playback => Icon::Play,
        Pane::Lyrics => Icon::Note,
        Pane::Covers => Icon::Disc,
        Pane::Window => Icon::Monitor,
        Pane::Other => Icon::More,
        Pane::Appearance => Icon::Sun,
    }
}

/// 一节设置与它的搜索词：左栏搜索按 `title` / `keys` 命中（分类名命中时
/// 该分类的节全部算命中）。
pub(crate) struct Section {
    pub(crate) pane: Pane,
    pub(crate) title: &'static str,
    pub(crate) keys: &'static str,
    pub(crate) build: fn() -> View,
}

pub(crate) const SECTIONS: &[Section] = &[
    Section {
        pane: Pane::Library,
        title: "音乐文件夹",
        keys: "添加 文件夹 目录 拖放 根目录 mp3 flac wav m4a aac alac ogg aiff",
        build: library::folders_section,
    },
    Section {
        pane: Pane::Library,
        title: "曲库",
        keys: "扫描更新 重建曲库 索引 监视 文件夹变化 自动 统计 曲目 专辑 歌手 总时长 缓存",
        build: library::library_section,
    },
    Section {
        pane: Pane::Library,
        title: "音频过滤",
        keys: "过滤 阈值 时长 大小 短音频 小文件 秒 KB 采样 铃声",
        build: library::filter_section,
    },
    Section {
        pane: Pane::Library,
        title: "删除文件",
        keys: "删除 文件 回收站 永久删除 直接删除 曲目 右键 菜单",
        build: library::delete_section,
    },
    Section {
        pane: Pane::Covers,
        title: "封面来源",
        keys: "封面 图片 本地 同名 内嵌 优先 来源 cover jpg jpeg png webp 专辑",
        build: covers::covers_source_section,
    },
    Section {
        pane: Pane::Playback,
        title: "淡入淡出",
        keys: "播放 音量 渐变 切歌 暂停 毫秒 时长 无缝",
        build: playback::playback_section,
    },
    Section {
        pane: Pane::Playback,
        title: "播放统计",
        keys: "统计 播放次数 首页 排行 阈值 百分比 秒 听过",
        build: playback::stats_section,
    },
    Section {
        pane: Pane::Playback,
        title: "输出设备",
        keys: "输出 设备 耳机 拔出 暂停 默认设备 声卡 蓝牙",
        build: playback::device_section,
    },
    Section {
        pane: Pane::Playback,
        title: "ReplayGain",
        keys: "replaygain 回放增益 音量 标准化 响度 专辑 曲目 预放大 削波 峰值",
        build: playback::replaygain_section,
    },
    Section {
        pane: Pane::Playback,
        title: "均衡器",
        keys: "均衡器 eq equalizer 频段 低音 高音 预设 流行 摇滚 爵士 古典 人声",
        build: playback::eq_section,
    },
    Section {
        pane: Pane::Lyrics,
        title: "歌词对齐",
        keys: "歌词 全屏 左对齐 居中 右对齐 对齐方式",
        build: lyrics::lyrics_section,
    },
    Section {
        pane: Pane::Lyrics,
        title: "歌词行时间戳",
        keys: "歌词 时间戳 行 悬停 时间 跳转 timestamp",
        build: lyrics::lyrics_timestamps_section,
    },
    Section {
        pane: Pane::Lyrics,
        title: "全局歌词偏移",
        keys: "歌词 偏移 全局 提前 延后 同步 offset 蓝牙 延迟",
        build: lyrics::lyrics_offset_section,
    },
    Section {
        pane: Pane::Lyrics,
        title: "歌词来源",
        keys: "歌词 本地 lrc 同名 内嵌 优先 来源",
        build: lyrics::lyrics_source_section,
    },
    Section {
        pane: Pane::Window,
        title: "关闭窗口时",
        keys: "关闭 窗口 最小化 托盘 退出 询问 记住 大小 位置 恢复",
        build: window::window_section,
    },
    Section {
        pane: Pane::Other,
        title: "艺术家分隔符",
        keys: "艺术家 歌手 分隔符 分号 顿号 斜杠 拆分",
        build: other::artist_separators_section,
    },
    Section {
        pane: Pane::Other,
        title: "标签分隔符",
        keys: "流派 标签 分隔符 分号 顿号 斜杠 拆分",
        build: other::genre_separators_section,
    },
    Section {
        pane: Pane::Appearance,
        title: "外观",
        keys: "浅色 深色 系统 主题 颜色 跟随",
        build: appearance::appearance_section,
    },
    Section {
        pane: Pane::Appearance,
        title: "主题色",
        keys: "主题色 颜色 强调色 黑白 红 橙 绿 蓝 紫 封面 自动 自定义 代码",
        build: appearance::accent_section,
    },
    Section {
        pane: Pane::Appearance,
        title: "字体",
        keys: "字体 系统字体 已安装 预览 字形",
        build: appearance::font_section,
    },
];

/// 搜索词是否命中一段文本（调用方已把 `q` 转成小写并去掉两端空格）。
pub(crate) fn matches(q: &str, s: &str) -> bool {
    s.to_lowercase().contains(q)
}

/// 分类命中：名字匹配，或它任一节的标题 / 关键词匹配。
pub(crate) fn pane_hits(q: &str, p: Pane) -> bool {
    matches(q, p.title())
        || SECTIONS
            .iter()
            .any(|s| s.pane == p && (matches(q, s.title) || matches(q, s.keys)))
}

/// 一节在右栏显示：分类名命中时整节显示，否则按自身标题 / 关键词。
pub(crate) fn section_hits(q: &str, s: &Section) -> bool {
    matches(q, s.pane.title()) || matches(q, s.title) || matches(q, s.keys)
}
