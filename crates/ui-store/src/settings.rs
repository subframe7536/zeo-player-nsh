use super::*;

/// 曲库配置的界面侧更新：克隆当前快照、应用修改、换新 `Arc`（界面
/// 立即看到新值；磁盘与 `LibraryStore` 侧的正式写入由调用方负责，
/// 下轮轮询把那边换好的 `Arc` 收进来）。
pub(crate) fn update_config(f: impl FnOnce(&mut zeo_types::config::LibraryConfig)) {
    store().library.update(|l| l.set_config(f));
}

/// 界面字体（`None`：系统字体）。立即生效并保存。
pub fn set_font(font: Option<String>) {
    update_config(|c| c.font = font.clone());
    std::thread::spawn(move || library_store().set_font(font));
}

/// 根视图的字体家族。
pub fn font_family() -> SharedString {
    store()
        .library
        .with(|l| l.config.font.clone())
        .map_or_else(|| SharedString::from("system-ui"), SharedString::from)
}

/// 全屏歌词的对齐方式（立即生效并保存）。
pub fn set_lyrics_align(align: zeo_types::config::LyricsAlign) {
    update_config(|c| c.lyrics_align = align);
    std::thread::spawn(move || library_store().set_lyrics_align(align));
}

/// 当前曲目的歌词偏移设为 `ms`（钳到 ±30000）并保存；没有正在播放的
/// 曲目时不做。
pub fn set_lyrics_offset(ms: i64) {
    let Some(path) = player().state().track.path.clone() else {
        return;
    };
    let v = ms.clamp(-30000, 30000);
    store().lyrics_offset.set(v);
    zeo_db::later(move || zeo_db::set_lyrics_offset(&path, v));
}

/// 当前曲目的歌词偏移加 `delta_ms`（钳到 ±30000）并保存；`None` 时清零。
pub fn nudge_lyrics_offset(delta_ms: Option<i64>) {
    match delta_ms {
        Some(d) => set_lyrics_offset(store().lyrics_offset.get() + d),
        None => set_lyrics_offset(0),
    }
}

/// 全屏歌词行悬停显示时间戳（立即生效并保存）。
pub fn set_lyrics_timestamps(on: bool) {
    update_config(|c| c.lyrics_timestamps = on);
    std::thread::spawn(move || library_store().set_lyrics_timestamps(on));
}

/// 全局歌词偏移（立即生效并保存；钳到 ±30000 毫秒）。
pub fn set_global_lyrics_offset(ms: i64) {
    let ms = ms.clamp(-30000, 30000);
    update_config(|c| c.global_lyrics_offset_ms = ms);
    std::thread::spawn(move || library_store().set_global_lyrics_offset_ms(ms));
}

/// 持久化的外观 → NSH 的 `Appearance`（`set_color_scheme` 的参数）。
pub(crate) fn color_scheme(mode: zeo_types::config::Appearance) -> Appearance {
    match mode {
        zeo_types::config::Appearance::System => Appearance::System,
        zeo_types::config::Appearance::Light => Appearance::Light,
        zeo_types::config::Appearance::Dark => Appearance::Dark,
    }
}

/// 界面外观（立即生效并保存）。
pub fn set_appearance(mode: zeo_types::config::Appearance) {
    update_config(|c| c.appearance = mode);
    set_color_scheme(color_scheme(mode));
    std::thread::spawn(move || library_store().set_appearance(mode));
}

/// 主题色的当前覆盖强调色（`None`：主题默认黑白）；`theme::primary` 读它。
pub fn accent() -> Option<AdaptiveColor> {
    store().accent.get()
}

/// 「跟随封面」的原始主色调（界面底色轻调用；非封面模式为 `None`）。
pub fn tint() -> Option<(f32, f32)> {
    store().tint.get()
}

/// 主题色：预设强调色或跟随当前曲目封面（立即生效并保存）。
pub fn set_theme_color(color: zeo_types::config::ThemeColor) {
    update_config(|c| c.theme_color = color);
    apply_accent();
    std::thread::spawn(move || library_store().set_theme_color(color));
}

/// 侧栏下半部当前显示的视图（歌单 / 标签 / 文件夹），立即生效并保存。
pub fn set_sidebar_section(section: SidebarSection) {
    let s = store();
    if s.sidebar_section.get_untracked() == section {
        return;
    }
    s.sidebar_section.set(section);
    update_config(|c| c.sidebar_section = section);
    std::thread::spawn(move || library_store().set_sidebar_section(section));
}

/// 按 `theme_color` 刷新覆盖色：预设色直接写；跟随封面时对当前曲目
/// 异步取主色调（结果由轮询的 [`drain_dominant`] 写回）。模式读 `library`
/// 信号（`set_theme_color` 先更新它，落盘在线程里，内存配置有延迟）。
pub(crate) fn apply_accent() {
    let s = store();
    match s.library.with(|l| l.config.theme_color) {
        zeo_types::config::ThemeColor::Cover => {
            let path = s.track.with(|t| t.path.clone());
            request_accent(&s, path.as_deref())
        }
        mode => {
            s.accent.set(crate::theme::preset_accent(mode));
            s.tint.set(None);
        }
    }
}

/// 跟随封面模式下取 `path` 的主色调（扫描时已写进 SQLite，同步读）；
/// 库里没有（老库 / 还没扫到）就让 covers 工作线程补算，结果经
/// [`drain_dominant`] 写回。没有曲目时回主题色。
pub(crate) fn request_accent(s: &Store, path: Option<&str>) {
    if s.library.with(|l| l.config.theme_color) != zeo_types::config::ThemeColor::Cover {
        return;
    }
    let Some(path) = path else {
        s.accent.set(None);
        s.tint.set(None);
        return;
    };
    match zeo_metadata::covers::art_hue(std::path::Path::new(path)) {
        Some((h, sat)) => {
            s.accent.set(Some(crate::theme::accent_of(h, sat)));
            s.tint.set(Some((h, sat)));
        }
        None => {
            s.accent.set(None);
            s.tint.set(None);
            zeo_metadata::covers::request(path, zeo_metadata::covers::TINY);
        }
    }
}

/// 封面主色调结果：跟随封面模式下成为强调色（只对当前曲目生效）。
pub(crate) fn drain_dominant() {
    for (path, hue) in zeo_metadata::covers::take_dominant() {
        let s = store();
        if s.library.with(|l| l.config.theme_color) == zeo_types::config::ThemeColor::Cover
            && s.track.with(|t| t.path.as_deref() == Some(path.as_str()))
        {
            s.accent
                .set(hue.map(|(h, sat)| crate::theme::accent_of(h, sat)));
            s.tint.set(hue);
        }
    }
}

/// 淡入淡出时长（立即生效并保存）。
pub fn set_fade_ms(ms: u32) {
    let ms = ms.min(5000);
    update_config(|c| c.fade_ms = ms);
    player().set_fade_ms(ms);
    std::thread::spawn(move || library_store().set_fade_ms(ms));
}

/// 切到下一个循环模式（顺序 → 单曲 → 随机），并保存。
pub fn cycle_loop_mode() {
    let mode = player().loop_mode().cycle();
    player().set_loop_mode(mode);
    std::thread::spawn(move || library_store().set_loop_mode(mode));
}

/// 本地歌词优先（保存）。歌词缓存键带开关位，下次读取自动换来源。
pub fn set_prefer_local_lyrics(on: bool) {
    update_config(|c| c.prefer_local_lyrics = on);
    std::thread::spawn(move || library_store().set_prefer_local_lyrics(on));
}
