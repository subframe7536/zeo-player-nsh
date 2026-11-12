use super::*;

/// 界面状态（在 [`install`] 之后可用）。
pub fn store() -> Store {
    STORE
        .with(|s| s.get())
        .expect("store installed by the root component")
}

pub fn player() -> &'static Player {
    Player::global()
}

/// 意图的引擎往返余量（毫秒）：命令正常时远早于此兑现；没兑现
/// （没有可播曲目之类）就到期回落真实状态。
pub(crate) const INTENT_TTL_MS: u64 = 600;

/// 播放 / 暂停：先记下意图（播放键立刻形变），再发引擎命令。
/// 快速连点按意图取反——真实状态还没回传时连续点不会被旧值顶回。
pub fn toggle() {
    let s = store();
    let now = s.intent.with(|i| {
        i.map(|(v, _)| v)
            .unwrap_or_else(|| s.playback.with(|p| p.playing))
    });
    s.intent.set(Some((
        !now,
        Instant::now() + Duration::from_millis(INTENT_TTL_MS),
    )));
    player().toggle();
}

/// 播放键应显示的播放态：用户意图优先（点击立即形变），否则引擎
/// 轮询回的真实状态。
pub fn shown_playing() -> bool {
    let s = store();
    s.intent.with(|i| {
        i.map(|(v, _)| v)
            .unwrap_or_else(|| s.playback.with(|p| p.playing))
    })
}

/// 建立状态并启动轮询；在根组件里调用一次。
pub fn install() -> Store {
    let snap = player().state();
    let lib = library_store();
    let playback = signal(Playback {
        playing: snap.is_playing,
        loading: snap.is_loading,
        time: snap.current_time,
        duration: snap.track.duration,
        volume: snap.volume,
        loop_mode: snap.loop_mode,
    });
    let track = signal(now_playing(&snap));
    let library = signal(library_state());
    let tracks = signal(Same(lib.tracks()));
    let dirs = signal(Same(lib.dirs()));
    let lyrics = signal(None::<Lyrics>);
    let lyrics_offset = signal(0_i64);
    let active_line = computed(move || {
        let time = playback.with(|p| p.time);
        // 生效偏移 = 每曲手动偏移 + 全局偏移（正值 = 提前）。
        let offset = lyrics_offset.get() + library.with(|l| l.config.global_lyrics_offset_ms);
        lyrics.with(|l| match l {
            Some(l) if l.output.timed => active_index(&l.output, time + offset as f64 / 1000.0),
            _ => -1,
        })
    });
    let s = Store {
        playback,
        intent: signal(None),
        track,
        library,
        tracks,
        dirs,
        meta: signal(None),
        lyrics,
        lyrics_offset,
        waveform: signal(None),
        seek_preview: signal(None),
        owner: Owner::current(),
        last_volume: signal(0.8),
        active_line,
        queue: signal(Same(snap.track.queue.clone())),
        hidden: signal(false),
        close_prompt: signal(false),
        close_remember: signal(true),
        search: signal(String::new()),
        playlists: signal(Same(Arc::new({
            let mut lists = zeo_playlists::load();
            zeo_playlists::ensure_favorites(&mut lists);
            lists
        }))),
        editor: signal(None),
        search_open: signal(false),
        settings: signal(false),
        full_screen: signal(false),
        delete_ask: signal(None),
        edit_info: signal(None),
        accent: signal(None),
        tint: signal(None),
        artist_sel: signal(None),
        sidebar_section: signal(lib.config().sidebar_section),
        genre_sel: signal(None),
        folder_sel: signal(None),
        folder_expanded: signal(BTreeSet::new()),
        folder_extra: signal(BTreeSet::new()),
        folder_message: signal(String::new()),
        folder_op: signal(None),
        update: signal(UpdateState::Idle),
        keybinds: signal(Same(Arc::new(keybinds_map()))),
        global_keybinds: signal(Same(Arc::new(global_keybinds_map()))),
        key_capture: signal(None),
    };
    STORE.with(|cell| cell.set(Some(s)));
    let (tx, posted) = mpsc::channel::<Box<dyn FnOnce() + Send>>();
    POSTER.with(|p| *p.borrow_mut() = Some(Post(tx)));
    apply_accent();
    auto_check_updates();

    // 引擎状态：100 ms 轮询共享状态（信号只在值变化时通知）。
    spawn(async move {
        // 跟随封面的强调色：上次请求主色调的曲目。
        let mut accent_track: Option<String> = None;
        // 上次见到的曲目快照：引擎侧的冷字段整块换新 `Arc`，指针一样
        // 就说明曲目 / 封面 / 队列 / 错误都没动，跳过整条冷路径。
        let mut last_track = snap.track;
        loop {
            timer(Duration::from_millis(100)).await;
            let snap = player().state();
            s.playback.set(Playback {
                playing: snap.is_playing,
                loading: snap.is_loading,
                time: snap.current_time,
                duration: snap.track.duration,
                volume: snap.volume,
                loop_mode: snap.loop_mode,
            });
            // 意图兑现（引擎已跟上）或等太久（命令被忽略，比如没有
            // 可播曲目）就丢弃，图标回落到真实状态。注意只能 `set` 不能
            // `update`：`update` 无条件通知订阅者，每轮都触发图标重算
            // （`timing` 重定向会一直占着动画器，无头测试的 settle 永远
            // 等不到 quiet）。
            if s.intent.with_untracked(|i| {
                i.is_some_and(|(want, dl)| want == snap.is_playing || Instant::now() >= dl)
            }) {
                s.intent.set(None);
            }
            if !Arc::ptr_eq(&snap.track, &last_track) {
                last_track = snap.track.clone();
                s.track.set(now_playing(&snap));
                s.queue.set(Same(snap.track.queue.clone()));
            }
            player().remember(&snap);
            if snap.track.path != accent_track {
                accent_track = snap.track.path.clone();
                request_accent(&s, snap.track.path.as_deref());
            }
            drain_covers();
            drain_dominant();
        }
    });

    // 窗口与托盘：等窗口事件（关闭请求、托盘操作、显示 / 隐藏）再处理。
    // 先绑实例：关闭请求 / 隐藏状态按实例归属，并行宿主（界面测试）互不串。
    zeo_shell::bind_instance();
    let listener = zeo_shell::listener();
    spawn(async move {
        loop {
            listener.notified().await;
            if zeo_shell::take_close_request() {
                s.close_prompt.set(true);
            }
            let hidden = zeo_shell::is_hidden();
            if hidden && !s.hidden.get_untracked() {
                // 刚进托盘：封面信号的 owner 整个 dispose（缩略图在磁盘
                // 缓存，显示时重新请求）；大图 LRU、歌词缓存与 SQLite 页
                // 缓存一并放掉（当前曲目的大图 Arc 由 NowPlaying 持着，
                // 不受影响）；延迟 1 秒等界面拆完（NSH 视图放掉图片
                // lease），清掉进程共享的解码图片预算（封面 / SVG / 变体
                // 的未出租条目会留到 budget 上限，隐藏期不会再被逐出），
                // 再把空闲堆页还给系统。期间重新显示则全跳过。
                COVERS.with(|c| c.borrow_mut().clear());
                COVER_STAMPS.with(|c| c.borrow_mut().clear());
                COVER_OWNER.with(|o| {
                    if let Some(owner) = o.borrow_mut().take() {
                        owner.dispose();
                    }
                });
                zeo_metadata::covers::clear_artwork();
                zeo_engine::Meta::clear_lyrics();
                zeo_metadata::waveform::clear();
                // 全库元数据内存表也丢（数据库行保留，按需单行回填）——
                // 曲库大时这是最重的一块常驻堆。
                zeo_metadata::MetaCache::global().drop_in_memory();
                zeo_db::shrink_memory();
                spawn(async {
                    timer(Duration::from_secs(1)).await;
                    if !zeo_shell::is_hidden() {
                        return;
                    }
                    nsh_core::image::budget().clear();
                    // 首剪之后每 30 秒再剪一次：隐藏期间封面池 / 队尾
                    // 预载仍在解码（大封面一条几十 MB 的临时缓冲），释放
                    // 后留在 malloc 分区里不还，要再调一次裁剪才还给
                    // 系统。重新显示即退出循环。
                    loop {
                        zeo_shell::trim_memory();
                        timer(Duration::from_secs(30)).await;
                        if !zeo_shell::is_hidden() {
                            break;
                        }
                    }
                });
            }
            s.hidden.set(hidden);
            for action in zeo_shell::take_tray_actions() {
                match action {
                    zeo_shell::TrayAction::Show => zeo_shell::show(),
                    zeo_shell::TrayAction::Toggle => toggle(),
                    zeo_shell::TrayAction::Previous => player().previous(),
                    zeo_shell::TrayAction::Next => player().next(),
                    zeo_shell::TrayAction::Quit => zeo_shell::quit(),
                }
            }
        }
    });

    // 「记住窗口大小和位置」：placement 走 NSH——启动后第一次拿到窗口状态
    // 时恢复上次存的（`set_placement` 会按现在的显示器校正）；之后平台
    // 每次上报的位置变化写回配置。`state` 在焦点变化等不动位置的时候也
    // 重报，与已存值比对去重（`window_placement` 是非信号的直读，本效果
    // 只订阅窗口状态 memo）。
    let win = nsh_core::element::desktop::main_window();
    let restored = stored(false);
    effect(move || {
        let Some(state) = win.state() else {
            return;
        };
        if !restored.replace(true) {
            if zeo_shell::remembered()
                && let Some(saved) = library_store().window_placement()
            {
                win.set_placement(saved);
            }
            return;
        }
        if zeo_shell::remembered() && library_store().window_placement() != Some(state.placement) {
            library_store().set_window_placement(state.placement);
        }
    });

    // 托盘图标与菜单跟着当前曲目与播放状态（memo 拦掉进度刷新：订阅整个
    // playback 信号会让 effect 每 100ms 都重设整个托盘）。上游 `set_tray`
    // 整体替换：图标 / 提示 / 菜单随状态重建。
    let tray_key = computed(move || {
        let (title, artist, has_track) = s
            .track
            .with(|t| (t.title.clone(), t.artist.clone(), t.path.is_some()));
        (title, artist, shown_playing(), has_track)
    });
    effect(move || {
        let (title, artist, playing, has_track) = tray_key.get();
        nsh_core::element::desktop::set_tray(zeo_shell::tray_for(&zeo_shell::TrayState {
            title,
            artist,
            playing,
            has_track,
        }));
    });

    // 全局快捷键（`register_global_shortcut`，窗口外 / 隐藏到托盘也生效）：
    // 生效表或捕获态变动时换 owner 全撤重绑——捕获中先清空，按下的候选键
    // 才不会被自己已注册的全局键吃掉。媒体键上游的 `Key` 表达不了：
    // Windows 走低级键盘钩子（`zeo-shell` 的 media_hook_thread），macOS
    // 走系统媒体会话，不在此列。
    effect(move || {
        if s.key_capture.get().is_some() {
            return;
        }
        let conflicted = global_conflicted_actions();
        for (action, key) in s.global_keybinds.get().iter() {
            if key.is_empty() || conflicted.contains(action) {
                continue;
            }
            let action = *action;
            let _ = nsh_core::element::desktop::register_global_shortcut(key, move || {
                run_keybind(action)
            });
        }
    });

    // 曲库：扫盘状态、根目录与索引。主题色走全局配置——别处（多窗口 /
    // 其它实例）改了它时重取强调色，不然换曲才刷新的强调色会停在旧值。
    let mut last_color = s.library.with(|l| l.config.theme_color);
    spawn(async move {
        loop {
            timer(Duration::from_millis(120)).await;
            // 后台线程经 poster() 排来的事（重命名 / 删除的收尾等）在这跑；
            // 单个出事只丢它自己，别拖垮轮询。
            while let Ok(f) = posted.try_recv() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            }
            let state = library_state();
            let changed = state.config.theme_color != last_color;
            last_color = state.config.theme_color;
            s.library.set(state);
            if changed {
                apply_accent();
                auto_check_updates();
            }
            s.tracks.set(Same(library_store().tracks()));
            s.dirs.set(Same(library_store().dirs()));
        }
    });

    // 拖动预览：真实进度追上后清除（消除 目标→原始→目标 的跳变）。
    effect(move || {
        let Some(pending) = s.seek_preview.get() else {
            return;
        };
        if (s.playback.with(|p| p.time) - pending).abs() < 0.35 {
            s.seek_preview.set(None);
        }
    });

    // 换曲：读元数据、歌词、波形（后台线程）。
    let path = computed(move || s.track.with(|t| t.path.clone()));
    effect(move || {
        let current = path.get();
        s.meta.set(None);
        s.lyrics.set(None);
        s.lyrics_offset.set(0);
        let Some(stamp) = current else {
            s.waveform.set(None);
            return;
        };
        let floor = zeo_metadata::waveform::WaveformOptions::default().min;
        s.waveform.set(Some(Same(Arc::new(vec![
            floor;
            zeo_metadata::waveform::BARS
        ]))));
        let still = move |stamp: &str| untracked(|| path.with(|p| p.as_deref() == Some(stamp)));
        let p = stamp.clone();
        spawn(async move {
            let meta = blocking({
                let p = p.clone();
                move || MetaCache::global().get(Path::new(&p))
            })
            .await;
            if still(&p)
                && let Ok(Ok(meta)) = meta
            {
                s.meta.set(Some(Same(meta)));
            }
            let lyrics = blocking({
                let p = p.clone();
                move || Meta::lyrics(&p)
            })
            .await;
            if still(&p)
                && let Ok((output, source)) = lyrics
            {
                s.lyrics.set(Some(Lyrics {
                    output: Same(Arc::new(output)),
                    source,
                }));
            }
            let offset = blocking({
                let p = p.clone();
                move || zeo_db::lyrics_offset(&p)
            })
            .await;
            if still(&p)
                && let Ok(offset) = offset
            {
                s.lyrics_offset.set(offset);
            }
            let bars = blocking({
                let p = p.clone();
                move || zeo_metadata::waveform::waveform(Path::new(&p))
            })
            .await;
            if still(&p) {
                s.waveform.set(match bars {
                    Ok(Ok(bars)) => Some(Same(bars)),
                    _ => None,
                });
            }
        });
    });

    // 队尾预载：当前曲目的后三首提前生成全部封面尺寸、解析好歌词
    // （各自的有界缓存；歌词不进全局元数据缓存）。换曲或队列变化时
    // 重算；重复请求在封面池 / 歌词缓存里去重，成本约等于零。
    effect(move || {
        path.get();
        s.queue.get();
        let snap = player().state();
        let Ok(index) = usize::try_from(snap.track.queue_index) else {
            return;
        };
        let next: Vec<String> = snap
            .track
            .queue
            .iter()
            .skip(index + 1)
            .take(3)
            .cloned()
            .collect();
        if next.is_empty() {
            return;
        }
        std::thread::spawn(move || {
            for p in next {
                zeo_metadata::covers::request_warm(&p);
                Meta::lyrics(&p);
            }
        });
    });

    // 文件内容变了（监视或手动扫描后指纹不同）：重新生成它的封面。
    effect(move || {
        let tracks = s.tracks.get();
        let stamps: HashMap<&str, u64> =
            tracks.iter().map(|t| (t.path.as_str(), t.stamp)).collect();
        let requested: Vec<(String, u32)> = COVERS.with(|c| {
            c.borrow()
                .iter()
                .map(|(p, (side, _))| (p.clone(), *side))
                .collect()
        });
        COVER_STAMPS.with(|known| {
            let mut known = known.borrow_mut();
            for (path, side) in requested {
                let Some(&stamp) = stamps.get(path.as_str()) else {
                    continue;
                };
                match known.insert(path.clone(), stamp) {
                    Some(old) if old != stamp => {
                        zeo_metadata::covers::request(&path, side);
                    }
                    _ => {}
                }
            }
        });
    });

    zeo_library::provider::local::watcher::sync();
    // 窗口应用钩子在 store 之下注入：之后 `library_store().set_close_action` 等
    // 直接调用也照样落到窗口上（设置页与测试都走这条）。
    zeo_library::config::set_window_apply_hook(Box::new(|apply| match apply {
        zeo_library::config::WindowApply::CloseAction(a) => zeo_shell::set_close_action(a),
        zeo_library::config::WindowApply::RememberFrame(on) => zeo_shell::set_remember_frame(on),
    }));
    zeo_shell::set_close_action(library_store().close_action());
    zeo_shell::set_remember_frame(library_store().remember_frame());
    set_color_scheme(color_scheme(library_store().appearance()));
    player().set_fade_ms(library_store().fade_ms());
    player().set_pause_on_device_change(library_store().pause_on_device_change());
    player().set_stats_thresholds(
        library_store().stats_min_percent(),
        library_store().stats_min_secs(),
    );
    player().set_replaygain(library_store().replaygain());
    player().set_eq(library_store().eq());
    player().set_loop_mode(library_store().loop_mode());
    // 升级前的老收藏补进 likes 表（liked_at=0：有记录但不计「新增」）；
    // 老曲目补 added 表（文件修改时间当入库时间的近似）。
    {
        let favs = s.playlists.with_untracked(|l| favorite_paths(l));
        if !favs.is_empty() {
            zeo_db::later(move || zeo_db::backfill_likes(&favs));
        }
        let entries: Vec<(String, u64)> = s
            .tracks
            .with_untracked(|l| l.iter().map(|t| (t.path.clone(), t.modified)).collect());
        if !entries.is_empty() {
            zeo_db::later(move || zeo_db::backfill_added(&entries));
        }
    }
    // 在设好循环模式之后：随机模式下恢复的队列按它重新洗牌。
    player().restore_last();
    zeo_shell::start();
    if !library_store().roots().is_empty() {
        scan();
    }
    s
}
