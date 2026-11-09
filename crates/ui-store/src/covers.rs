use super::*;

// --- 封面 ---------------------------------------------------------------------

/// `path` 的列表封面（文件路径）：`pt` ≤ 48 的插槽（曲目行、队列、
/// chips）给 96px 小图，更大的（网格 ~184pt、详情页 hero）升档到
/// 400px——每个音频文件最多一份缓存封面，升档后小文件删掉。首次读取
/// 时交给后台线程池生成；每个曲目一个信号，生成后只有读它的绑定再
/// 运行。信号属于专用的 `COVER_OWNER`，列表单元格复用或销毁都不会
/// 丢掉进行中的请求，隐藏到托盘时才整个拆掉。
/// 「列表显示封面」开关只管列表，关掉时这里返回 `None`——不建信号、
/// 不发请求，列表完全不解码封面；读库信号让调用处的闭包订阅它，切换
/// 即重渲。
pub fn cover_for(path: &str, pt: f32) -> Option<SharedString> {
    if !covers_on() {
        return None;
    }
    cover_for_always(path, pt)
}

/// 「在列表中显示封面」开关的响应式读：关掉后曲目表、网格卡片、队列
/// 行等封面槽位整个移除（各处用 `when(covers_on, …)` 包着），不再画
/// 首字占位图。
pub fn covers_on() -> bool {
    store().library.with(|l| l.config.show_covers)
}

/// 与 [`cover_for`] 相同但不受「列表显示封面」开关影响：播放条的当前
/// 曲目封面一直显示（开关只管列表渲染）。
pub fn cover_for_always(path: &str, pt: f32) -> Option<SharedString> {
    if cfg!(any(debug_assertions, test)) && std::env::var_os("ZEO_PLAYER_BENCH_NO_COVERS").is_some()
    {
        return None;
    }
    let side = if pt <= 48.0 {
        zeo_metadata::covers::TINY
    } else {
        zeo_metadata::covers::THUMB
    };
    let hit = COVERS.with(|c| c.borrow().get(path).copied());
    let cover = match hit {
        Some((have, cover)) => {
            // 升档：先记下已请求的尺寸，重复调用不再排队。
            if have < side {
                COVERS.with(|c| c.borrow_mut().insert(path.to_owned(), (side, cover)));
                zeo_metadata::covers::request(path, side);
            }
            cover
        }
        None => {
            // 挂在专用 owner 下：根 owner 永不 dispose，信号建在它下面
            // 的话隐藏时清句柄也放不掉节点。
            let cover = COVER_OWNER.with(|o| {
                let mut o = o.borrow_mut();
                if o.as_ref().is_none_or(|owner| !owner.is_live()) {
                    *o = Some(nsh_core::element::create_root(|| {}).0);
                }
                o.unwrap().run(|| signal(None::<SharedString>))
            });
            let mut have = side;
            // 封面已在磁盘缓存（扫描预热 / 队尾预载 / 播过）就先填上，
            // 免得闪一次占位符；请求照旧入队（色调补算 / 指纹变更重算）。
            if let Some((s, file)) = zeo_metadata::covers::cached_list(Path::new(path)) {
                cover.set(Some(file.to_string_lossy().into_owned().into()));
                have = have.max(s);
            }
            COVERS.with(|c| c.borrow_mut().insert(path.to_owned(), (have, cover)));
            zeo_metadata::covers::request(path, side);
            cover
        }
    };
    cover.get()
}

pub(crate) fn drain_covers() {
    for (track, side, file) in zeo_metadata::covers::take_done() {
        if let Some((have, cover)) = COVERS.with(|c| c.borrow().get(&track).copied()) {
            let keep = side.max(have);
            if side >= have {
                COVERS.with(|c| c.borrow_mut().insert(track.clone(), (keep, cover)));
                // None（无封面 / 来源换掉了）也要写：清掉可能已过期的旧图。
                cover.set(file.map(SharedString::from));
            }
            // 每个音频文件最多一份列表封面：信号已换到大文件（或升档
            // 请求已记下），小文件删掉。
            zeo_metadata::covers::prune_smaller(Path::new(&track), keep);
        }
    }
}
