use super::*;

/// 「删除文件」不经过回收站（保存）。
pub fn set_delete_permanently(on: bool) {
    update_config(|c| c.delete_permanently = on);
    std::thread::spawn(move || library_store().set_delete_permanently(on));
}

/// 输出设备变化时暂停播放（立即生效并保存）。
pub fn set_pause_on_device_change(on: bool) {
    update_config(|c| c.pause_on_device_change = on);
    player().set_pause_on_device_change(on);
    std::thread::spawn(move || library_store().set_pause_on_device_change(on));
}

/// 播放统计阈值：听过多少算一次播放（立即生效并保存；两个条件任一
/// 满足即计，填 0 表示不用该条件）。
pub fn set_stats_thresholds(percent: u32, secs: u32) {
    let (percent, secs) = (percent.min(100), secs.min(3600));
    update_config(|c| {
        c.stats_min_percent = percent;
        c.stats_min_secs = secs;
    });
    player().set_stats_thresholds(percent, secs);
    std::thread::spawn(move || library_store().set_stats_thresholds(percent, secs));
}

/// ReplayGain 设置（立即生效并保存）。
pub fn set_replaygain(cfg: zeo_engine::audio::ReplayGainConfig) {
    update_config(|c| c.replaygain = cfg);
    player().set_replaygain(cfg);
    std::thread::spawn(move || library_store().set_replaygain(cfg));
}

/// 均衡器设置（立即生效；持久化防抖 400ms——拖滑块不每动一下都写盘）。
pub fn set_eq(cfg: zeo_engine::eq::EqConfig) {
    update_config(|c| c.eq = cfg);
    player().set_eq(cfg);
    // 内存快照立即更新（120ms 轮询不会把界面弹回旧档）。
    library_store().set_eq(cfg);
    // 落盘防抖：GEN>0 表示已有守候线程在等安静——拖动推子一秒能产生
    // 上百次调用，不能每动一下都起一个线程。
    static GEN: AtomicU64 = AtomicU64::new(0);
    if GEN.fetch_add(1, Ordering::Relaxed) == 0 {
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(Duration::from_millis(400));
                // 400ms 内没人再调（GEN 仍是 0）：落盘并收工。
                if GEN.swap(0, Ordering::Relaxed) == 0 {
                    library_store().save();
                    return;
                }
            }
        });
    }
}

/// 「删除文件」确认弹窗的请求：记下路径与当前的删除方式（回收站 / 直接删）。
#[derive(Debug, Clone, PartialEq)]
pub struct DeleteAsk {
    pub paths: Vec<String>,
    pub permanent: bool,
}

/// 从曲库移除 `path` 的记录：索引、元数据缓存与数据库、封面缓存文件、
/// 各手动歌单（含「我喜欢」）都清掉；文件不碰（还在时下次扫描会重新
/// 入库）。正在播放这首就停掉。
pub fn remove_record(path: &str) {
    let s = store();
    if s.track.with_untracked(|t| t.path.as_deref() == Some(path)) {
        player().stop();
    }
    let fs_path = Path::new(path);
    zeo_metadata::covers::evict(fs_path);
    MetaCache::global().remove(fs_path);
    let db_path = path.to_owned();
    zeo_db::later(move || zeo_db::remove_meta(&db_path));
    change_playlists(|lists| {
        for p in lists.iter_mut() {
            if let Kind::Manual { tracks } = &mut p.kind {
                tracks.retain(|t| t != path);
            }
        }
    });
    library_store().remove_track(path);
    s.tracks.set(Same(library_store().tracks()));
}

/// `src` 移 / 改名到 `dest`：手动歌单里等于它或以 `src/` 为前缀的路
/// 径换成新前缀，曲目留在歌单里。
pub fn move_playlist_paths(src: &str, dest: &str) {
    let sep = std::path::MAIN_SEPARATOR;
    let (src, dest) = (
        src.trim_end_matches(sep).to_owned(),
        dest.trim_end_matches(sep).to_owned(),
    );
    if src == dest {
        return;
    }
    let under = format!("{src}{sep}");
    change_playlists(|lists| {
        for p in lists.iter_mut() {
            if let Kind::Manual { tracks } = &mut p.kind {
                for t in tracks.iter_mut() {
                    if *t == src {
                        *t = dest.clone();
                    } else if let Some(rest) = t.strip_prefix(&under) {
                        *t = format!("{dest}{sep}{rest}");
                    }
                }
            }
        }
    });
}

/// 删掉 `path` 下（含自身）的手动歌单条目（文件夹已删 / 记录被清，
/// 留着是死条目）。
pub fn remove_playlist_paths_under(path: &str) {
    let sep = std::path::MAIN_SEPARATOR;
    let trimmed = path.trim_end_matches(sep);
    let under = format!("{trimmed}{sep}");
    change_playlists(|lists| {
        for p in lists.iter_mut() {
            if let Kind::Manual { tracks } = &mut p.kind {
                tracks.retain(|t| t != trimmed && !t.starts_with(&under));
            }
        }
    });
}

/// 「删除文件…」菜单项：打开确认弹窗（回收站 / 直接删按当前设置）。
#[allow(dead_code)]
pub fn ask_delete_file(path: &str) {
    ask_delete_files(vec![path.to_owned()]);
}

/// 批量「删除文件…」菜单项：打开确认弹窗。
pub fn ask_delete_files(paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let permanent = library_store().delete_permanently();
    store().delete_ask.set(Some(DeleteAsk { paths, permanent }));
}

/// 确认删除：后台线程做文件操作（解码器可能还握着文件——Windows 上开着
/// 移不走，先停掉再多试几次），成功后删掉曲库记录。
pub fn confirm_delete_file() {
    let Some(ask) = store().delete_ask.get() else {
        return;
    };
    let ui = poster();
    std::thread::spawn(move || {
        // 工作线程拿不到 store()（线程局部），用播放器快照看当前曲。
        let current = player().state().track.path.clone();
        if let Some(ref cur) = current {
            if ask.paths.iter().any(|p| p == cur) {
                player().stop();
            }
        }
        let mut gone_paths = Vec::new();
        for p in &ask.paths {
            let path = Path::new(p);
            let mut gone = !path.exists();
            for _ in 0..10 {
                if gone {
                    break;
                }
                if zeo_library::provider::local::delete_file(path, ask.permanent).is_ok() {
                    gone = true;
                } else {
                    std::thread::sleep(Duration::from_millis(150));
                }
            }
            if gone {
                gone_paths.push(p.clone());
            }
        }
        ui.post(move || {
            for p in gone_paths {
                remove_record(&p);
            }
        });
    });
}

/// 本地封面优先（保存）：封面缓存键含开关位，重算已显示的封面
/// （已生成文件保留，翻回来直接命中）。
pub fn set_prefer_local_covers(on: bool) {
    update_config(|c| c.prefer_local_covers = on);
    std::thread::spawn(move || {
        library_store().set_prefer_local_covers(on);
        // 先写配置再请求：缓存键按新开关算，已显示的信号拿到新来源的图。
        let known: Vec<(String, u32)> = COVERS.with(|c| {
            c.borrow()
                .iter()
                .map(|(p, (side, _))| (p.clone(), *side))
                .collect()
        });
        for (path, side) in known {
            zeo_metadata::covers::request(&path, side);
        }
    });
}

/// 列表是否显示封面（保存）：关掉后行 / 网格 / 队列只画占位图，
/// 省掉所有列表封面的解码内存；打开即恢复（信号还在，值现成）。
pub fn set_show_covers(on: bool) {
    update_config(|c| c.show_covers = on);
    std::thread::spawn(move || library_store().set_show_covers(on));
}

/// macOS 菜单栏是否在托盘图标旁显示曲名（保存，立即生效）。

/// 关闭窗口时的行为（保存）。
pub fn set_close_action(action: zeo_types::config::CloseAction) {
    update_config(|c| c.close_action = action);
    std::thread::spawn(move || library_store().set_close_action(action));
}

/// 「记住窗口大小和位置」（立即生效并保存；关掉后已存的位置丢弃）。
pub fn set_remember_frame(on: bool) {
    update_config(|c| c.remember_frame = on);
    std::thread::spawn(move || library_store().set_remember_frame(on));
}

/// 回答关闭询问：勾了「记住选择」才记住这次的选择，然后照做。
pub fn answer_close(action: zeo_types::config::CloseAction) {
    let s = store();
    s.close_prompt.set(false);
    if s.close_remember.get_untracked() {
        set_close_action(action);
    }
    match action {
        zeo_types::config::CloseAction::Tray => zeo_shell::hide(),
        zeo_types::config::CloseAction::Quit => zeo_shell::quit(),
        zeo_types::config::CloseAction::Ask => {}
    }
}

/// 打开/关闭文件夹监视。
pub fn set_watch(on: bool) {
    update_config(|c| c.watch = on);
    std::thread::spawn(move || library_store().set_watch(on));
}

pub(crate) fn now_playing(snap: &zeo_engine::audio::Snapshot) -> NowPlaying {
    let t = &snap.track;
    NowPlaying {
        path: t.path.clone(),
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        artwork: t.artwork.clone().map(SharedString::from),
        covers_ready: t.covers_ready,
        error: t.error.clone(),
        index: t.queue_index,
    }
}

/// 当前歌词行（ap-spec：提前 0.5 s，开头 1 s 内不提前）。
pub fn active_index(output: &LyricsOutput, time: f64) -> i64 {
    let lookahead = if time > 1.0 { 0.5 } else { 0.0 };
    let mut active = -1;
    for (i, line) in output.lines.iter().enumerate() {
        if line.time >= 0.0 && line.time <= time + lookahead {
            active = i as i64;
        }
    }
    active
}
