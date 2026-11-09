use super::*;

// --- 命令 ---------------------------------------------------------------------

/// 播放 `tracks[index]`，以 `tracks` 为队列。
pub fn play_list(tracks: &[TrackEntry], index: usize) {
    if let Some(track) = tracks.get(index) {
        let queue = tracks.iter().map(|t| t.path.clone()).collect();
        player().open(track.path.clone(), queue);
    }
}

/// 把曲目追加到当前队尾（当前曲目与循环模式不变）。
pub fn queue_add(tracks: &[TrackEntry]) {
    player().queue_add(tracks.iter().map(|t| t.path.clone()).collect());
}

/// 拖动/点击进度的提交。
pub fn seek_to(seconds: f64) {
    store().seek_preview.set(Some(seconds));
    player().seek(seconds);
}

/// 设置音量：界面立即跟手，不等下一次轮询。
pub fn set_volume(volume: f64) {
    let volume = volume.clamp(0.0, 1.0);
    store().playback.update(|p| p.volume = volume);
    player().set_volume(volume);
}

/// 静音切换（记住静音前音量）。
pub fn toggle_mute() {
    let s = store();
    let volume = s.playback.with_untracked(|p| p.volume);
    if volume > 0.0 {
        s.last_volume.set(volume);
        set_volume(0.0);
    } else {
        let last = s.last_volume.get_untracked();
        set_volume(if last > 0.0 { last } else { 0.8 });
    }
}

/// 扫描所有根目录（后台线程）。
pub fn scan() {
    if library_store().is_scanning() {
        return;
    }
    std::thread::spawn(|| library_store().scan());
}

/// 重建曲库：清掉标签与封面缓存后重新读取（后台线程）。
pub fn rebuild() {
    if library_store().is_scanning() {
        return;
    }
    // 已显示的封面保留；重建完成后重新生成，信号再收到新文件。
    let known: Vec<(String, u32)> = COVERS.with(|c| {
        c.borrow()
            .iter()
            .map(|(p, (side, _))| (p.clone(), *side))
            .collect()
    });
    std::thread::spawn(move || {
        library_store().rebuild();
        for (path, side) in known {
            zeo_metadata::covers::request(&path, side);
        }
    });
}

/// 改流派标签分隔符（立即重新拆分，后台线程）。
pub fn set_separators(separators: Vec<String>) {
    std::thread::spawn(move || library_store().set_separators(&separators));
}

/// 改艺术家分隔符（立即保存配置，后台线程）。
pub fn set_artist_separators(separators: Vec<String>) {
    std::thread::spawn(move || library_store().set_artist_separators(&separators));
}

/// 过滤阈值：短于 `secs` 的音频不进曲库（0 不过滤）；保存后重新扫描。
pub fn set_min_duration_secs(secs: u32) {
    std::thread::spawn(move || {
        library_store().set_min_duration_secs(secs);
        rescan_when_idle();
    });
}

/// 过滤阈值：小于 `kb` KB 的音频不进曲库（0 不过滤）；保存后重新扫描。
pub fn set_min_size_kb(kb: u32) {
    std::thread::spawn(move || {
        library_store().set_min_size_kb(kb);
        rescan_when_idle();
    });
}

/// 改完阈值重新扫描；进行中的扫描先等它结束，不然用的是旧阈值。
pub(crate) fn rescan_when_idle() {
    while library_store().is_scanning() {
        std::thread::sleep(Duration::from_millis(200));
    }
    library_store().scan();
}

/// 弹出系统文件夹选择器；选中的文件夹加入曲库并扫描。返回结果说明。
pub async fn pick_folders() -> Option<String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let picked = blocking(|| {
            let start = dirs::audio_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_default();
            rfd::FileDialog::new()
                .set_title("添加音乐文件夹")
                .set_directory(start)
                .pick_folders()
        })
        .await
        .ok()
        .flatten()?;
        let lib = library_store();
        let mut added = Vec::new();
        let mut errors = Vec::new();
        for path in picked {
            match lib.add_root(&path.to_string_lossy()) {
                Ok(_) => added.push(path.to_string_lossy().into_owned()),
                Err(error) => errors.push(error),
            }
        }
        if !added.is_empty() {
            scan();
        }
        let mut lines: Vec<String> = added.iter().map(|p| format!("已添加 {p}")).collect();
        lines.extend(errors);
        Some(lines.join("\n"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Some("此平台暂不支持文件夹选择器，请把文件夹拖进来".to_owned())
    }
}
