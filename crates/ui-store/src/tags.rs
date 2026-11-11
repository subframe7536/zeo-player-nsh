use super::*;

// --- 标签写回 ------------------------------------------------------------------

/// 打开「编辑元数据」弹窗（曲目右键菜单 / 全屏「⋯」菜单）。
pub fn open_edit_info(path: &str) {
    store().edit_info.set(Some(path.to_owned()));
}

/// 「编辑元数据」→「保存」：后台线程把 `edits` 原子写回 `path`；成败都经
/// `done`（回界面侧执行）回报。正在播放的文件先释放解码器再替换、写完
/// 重载并恢复位置 / 播放态；索引与元数据缓存随后单路径刷新（曲库信号
/// 由轮询带上，不另外推）。写失败时原文件没动，照常恢复播放。
pub fn save_tags(
    path: String,
    edits: zeo_metadata::write::TagEdits,
    done: impl FnOnce(Result<(), String>) + Send + 'static,
) {
    let post = poster();
    std::thread::spawn(move || {
        let is_current = player().state().track.path.as_deref() == Some(path.as_str());
        // Release 同步等待解码器真的放下文件句柄，再开始写。
        let (position, playing) = if is_current {
            player().release_current()
        } else {
            (0.0, false)
        };
        let file = Path::new(&path);
        let result = zeo_metadata::write::write_tags(file, &edits);
        if result.is_ok() {
            // 指纹变了：MetaCache 重读并把新条目排进待写队列；索引条目
            // 原地换新并重排。
            library_store().refresh_paths(std::slice::from_ref(&path));
        }
        if is_current {
            player().reload_current(position, playing);
        }
        post.post(move || {
            // 当前曲目的详情元数据（`s.meta`）只在换曲时重读——保存后
            // 同一路径不触发，这里手动刷。
            if result.is_ok()
                && store()
                    .track
                    .with(|t| t.path.as_deref() == Some(path.as_str()))
                && let Ok(meta) = MetaCache::global().get(Path::new(&path))
            {
                store().meta.set(Some(Same(meta)));
            }
            // 写同名图片时音频指纹没变，`COVER_STAMPS` 的 effect 不会
            // 重发封面请求——按已请求过的尺寸直接重发（缓存键折了侧车
            // 指纹，必然重新生成；内嵌写走音频指纹重发，这里是去重的）。
            if result.is_ok()
                && let Some((side, _)) = COVERS.with(|c| c.borrow().get(path.as_str()).copied())
            {
                zeo_metadata::covers::request(&path, side);
            }
            done(result);
        });
    });
}

/// 「批量保存」（元数据视图）：后台线程顺序写回每条 `TagEdits`；当前曲目
/// 在其中就先释放解码器再写、全部写完后重载恢复。成功的路径一次
/// `refresh_paths`；`done`（回界面侧）收到失败列表 `(路径, 报错)`。
pub fn save_tags_batch(
    items: Vec<(String, zeo_metadata::write::TagEdits)>,
    done: impl FnOnce(Vec<(String, String)>) + Send + 'static,
) {
    let post = poster();
    std::thread::spawn(move || {
        let current = player().state().track.path.clone();
        // 同 save_tags：release 同步等解码器放下句柄再写，写完重载。
        let released = current
            .as_ref()
            .is_some_and(|cur| items.iter().any(|(p, _)| p == cur))
            .then(|| player().release_current());
        let mut ok_paths: Vec<String> = Vec::new();
        let mut errors: Vec<(String, String)> = Vec::new();
        for (path, edits) in items {
            match zeo_metadata::write::write_tags(Path::new(&path), &edits) {
                Ok(()) => ok_paths.push(path),
                Err(error) => errors.push((path, error)),
            }
        }
        if let Some((position, playing)) = released {
            player().reload_current(position, playing);
        }
        if !ok_paths.is_empty() {
            library_store().refresh_paths(&ok_paths);
        }
        post.post(move || {
            // 当前曲目写过：详情元数据手动刷（`s.meta` 只在换曲时重读）。
            if let Some(cur) = store().track.with(|t| t.path.clone())
                && ok_paths.iter().any(|p| *p == cur)
                && let Ok(meta) = MetaCache::global().get(Path::new(&cur))
            {
                store().meta.set(Some(Same(meta)));
            }
            // 已请求过的封面按原尺寸重发（同 save_tags：缓存键折了文件
            // 指纹必然重新生成）。
            for path in &ok_paths {
                if let Some((side, _)) = COVERS.with(|c| c.borrow().get(path.as_str()).copied()) {
                    zeo_metadata::covers::request(path, side);
                }
            }
            done(errors);
        });
    });
}
