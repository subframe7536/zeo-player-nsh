use std::sync::Arc;

use super::*;

impl LibraryStore {
    /// 配置的共享快照（`Arc` 克隆只加引用计数；设置写入才换新）。
    pub fn config(&self) -> Arc<LibraryConfig> {
        self.config.lock().unwrap().clone()
    }

    pub fn roots(&self) -> Vec<String> {
        self.config().roots.clone()
    }

    /// 克隆当前配置、用 `f` 改、写盘并换新 `Arc`（返回新快照）。
    pub(crate) fn update_config(&self, f: impl FnOnce(&mut LibraryConfig)) -> Arc<LibraryConfig> {
        let mut guard = self.config.lock().unwrap();
        let mut config = (**guard).clone();
        f(&mut config);
        self.save_config(&config);
        let config = Arc::new(config);
        *guard = config.clone();
        config
    }

    pub(crate) fn save_config(&self, config: &LibraryConfig) {
        if let Ok(bytes) = serde_json::to_vec_pretty(config) {
            let path = config_path(&self.dir);
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, &bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    /// 新增音乐根目录（必须存在且为目录）；返回新的根列表。
    pub fn add_root(&self, path: &str) -> Result<Vec<String>, String> {
        // dunce：Windows 上不返回 `\\?\C:\…` 这种逐字路径，和扫描出的曲目路径一致。
        let canonical = dunce::canonicalize(path).map_err(|e| format!("无法访问 {path}: {e}"))?;
        if !canonical.is_dir() {
            return Err(format!("{path} 不是目录"));
        }
        let as_string = canonical.to_string_lossy().into_owned();
        let config = self.update_config(|c| {
            if !c.roots.iter().any(|r| r == &as_string) {
                c.roots.push(as_string.clone());
            }
        });
        crate::provider::local::watcher::sync();
        Ok(config.roots.clone())
    }

    /// 移除音乐根目录。
    pub fn remove_root(&self, path: &str) -> Vec<String> {
        let config = self.update_config(|c| c.roots.retain(|r| r != path));
        crate::provider::local::watcher::sync();
        config.roots.clone()
    }

    /// 设置界面字体（`None`：系统字体），保存。
    pub fn set_font(&self, font: Option<String>) {
        self.update_config(|c| c.font = font.filter(|f| !f.trim().is_empty()));
    }

    pub fn fade_ms(&self) -> u32 {
        self.config().fade_ms
    }

    pub fn set_fade_ms(&self, ms: u32) {
        self.update_config(|c| c.fade_ms = ms);
    }

    pub fn set_lyrics_align(&self, align: LyricsAlign) {
        self.update_config(|c| c.lyrics_align = align);
    }

    pub fn appearance(&self) -> Appearance {
        self.config().appearance
    }

    /// 设置界面外观（保存）。
    pub fn set_appearance(&self, appearance: Appearance) {
        self.update_config(|c| c.appearance = appearance);
    }

    #[allow(dead_code)]
    pub fn sidebar_section(&self) -> SidebarSection {
        self.config().sidebar_section
    }

    /// 设置侧栏下半部显示的视图（保存）。
    pub fn set_sidebar_section(&self, section: SidebarSection) {
        self.update_config(|c| c.sidebar_section = section);
    }

    pub fn loop_mode(&self) -> zeo_types::audio::LoopMode {
        self.config().loop_mode
    }

    pub fn set_loop_mode(&self, mode: zeo_types::audio::LoopMode) {
        self.update_config(|c| c.loop_mode = mode);
    }

    /// 本地同名歌词文件是否优先于内嵌歌词。
    pub fn prefer_local_lyrics(&self) -> bool {
        self.config().prefer_local_lyrics
    }

    pub fn set_prefer_local_lyrics(&self, on: bool) {
        self.update_config(|c| c.prefer_local_lyrics = on);
    }

    /// 设置全局歌词偏移（保存）。
    pub fn set_global_lyrics_offset_ms(&self, ms: i64) {
        self.update_config(|c| c.global_lyrics_offset_ms = ms);
    }

    /// 全屏歌词行时间戳（悬停显示）开关。
    pub fn set_lyrics_timestamps(&self, on: bool) {
        self.update_config(|c| c.lyrics_timestamps = on);
    }

    /// 本地同名图片文件是否优先于内嵌封面。
    pub fn prefer_local_covers(&self) -> bool {
        self.config().prefer_local_covers
    }

    pub fn set_prefer_local_covers(&self, on: bool) {
        self.update_config(|c| c.prefer_local_covers = on);
        zeo_types::prefs::sync_prefer_local_covers(on);
    }

    /// 设置列表是否显示封面（关闭后行 / 网格 / 队列只画占位图）。
    pub fn set_show_covers(&self, on: bool) {
        self.update_config(|c| c.show_covers = on);
    }

    /// macOS 菜单栏是否在托盘图标旁显示当前曲名。

    /// 「删除文件」是否不经过回收站、直接删除。
    pub fn delete_permanently(&self) -> bool {
        self.config().delete_permanently
    }

    pub fn set_delete_permanently(&self, on: bool) {
        self.update_config(|c| c.delete_permanently = on);
    }

    /// 输出设备变化时是否暂停播放。
    pub fn pause_on_device_change(&self) -> bool {
        self.config().pause_on_device_change
    }

    pub fn set_pause_on_device_change(&self, on: bool) {
        self.update_config(|c| c.pause_on_device_change = on);
    }

    /// 播放统计阈值（听过多少算一次播放；0 = 不用该条件）。
    pub fn stats_min_percent(&self) -> u32 {
        self.config().stats_min_percent
    }

    pub fn stats_min_secs(&self) -> u32 {
        self.config().stats_min_secs
    }

    /// 两个阈值一起写（保存）。
    pub fn set_stats_thresholds(&self, percent: u32, secs: u32) {
        self.update_config(|c| {
            c.stats_min_percent = percent.min(100);
            c.stats_min_secs = secs.min(3600);
        });
    }

    /// ReplayGain 设置。
    pub fn replaygain(&self) -> zeo_types::audio::ReplayGainConfig {
        self.config().replaygain
    }

    pub fn set_replaygain(&self, cfg: zeo_types::audio::ReplayGainConfig) {
        self.update_config(|c| c.replaygain = cfg);
    }

    /// 均衡器设置。
    pub fn eq(&self) -> zeo_types::audio::EqConfig {
        self.config().eq
    }

    /// 只更新内存里的配置快照、不落盘：`library.json` 的写盘由调用方
    /// （`ui::store::set_eq`，400ms 防抖）调 [`LibraryStore::save`] 收尾。
    /// 内存立即更新是为了不让 120ms 轮询在防抖窗口内把界面值弹回旧档。
    pub fn set_eq(&self, cfg: zeo_types::audio::EqConfig) {
        let mut guard = self.config.lock().unwrap();
        let mut config = (**guard).clone();
        config.eq = cfg;
        *guard = Arc::new(config);
    }

    /// 把当前内存配置写回 `library.json`（防抖写盘的收尾用）。
    pub fn save(&self) {
        self.save_config(&self.config());
    }

    pub fn close_action(&self) -> CloseAction {
        self.config().close_action
    }

    pub fn set_close_action(&self, action: CloseAction) {
        self.update_config(|c| c.close_action = action);
        window_apply(WindowApply::CloseAction(action));
    }

    /// 重新打开时是否恢复上次的窗口大小与位置。
    pub fn remember_frame(&self) -> bool {
        self.config().remember_frame
    }

    pub fn set_remember_frame(&self, on: bool) {
        self.update_config(|c| {
            c.remember_frame = on;
            if !on {
                c.window_placement = None;
            }
        });
        window_apply(WindowApply::RememberFrame(on));
    }

    /// 上次的窗口位置（`remember_frame` 开着时才有意义）。
    pub fn window_placement(&self) -> Option<nsh_core::desktop::WindowPlacement> {
        self.config().window_placement
    }

    /// 记录当前窗口位置（界面侧的窗口状态效果在上报变化时调用）。
    pub fn set_window_placement(&self, placement: nsh_core::desktop::WindowPlacement) {
        let mut guard = self.config.lock().unwrap();
        if !guard.remember_frame {
            return;
        }
        let mut config = (**guard).clone();
        config.window_placement = Some(placement);
        self.save_config(&config);
        *guard = Arc::new(config);
    }

    pub fn watching(&self) -> bool {
        self.config().watch
    }

    /// 设置主题色（保存）。
    pub fn set_theme_color(&self, color: ThemeColor) {
        self.update_config(|c| c.theme_color = color);
    }

    /// 设置时长过滤阈值（保存；0 为不过滤）。
    pub fn set_min_duration_secs(&self, secs: u32) {
        self.update_config(|c| c.min_duration_secs = secs);
    }

    /// 设置大小过滤阈值（保存；0 为不过滤）。
    pub fn set_min_size_kb(&self, kb: u32) {
        self.update_config(|c| c.min_size_kb = kb);
    }

    /// 打开/关闭文件夹监视（保存设置）。
    pub fn set_watch(&self, on: bool) {
        self.update_config(|c| c.watch = on);
        crate::provider::local::watcher::sync();
    }
}
