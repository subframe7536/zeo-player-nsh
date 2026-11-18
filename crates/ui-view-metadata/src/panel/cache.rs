use super::*;

/// 单选面板异步读的完整行：全部标签字段 + 读取来源的标签类型 + 入库
/// 时间（`db::added_at`，随同一后台读捎回——SQLite 读别上界面线程）。
pub(crate) type FullRow = (HashMap<TagField, String>, Option<&'static str>, Option<u64>);

pub(crate) const FULL_CACHE_CAP: usize = 32;
pub(crate) type FullCache = Mutex<Vec<(String, FullRow)>>;

pub(crate) fn full_cache() -> &'static FullCache {
    static CACHE: OnceLock<FullCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn get_cached_full(path: &str) -> Option<FullRow> {
    let mut cache = full_cache().lock().unwrap();
    let pos = cache.iter().position(|(p, _)| p == path)?;
    let entry = cache.remove(pos);
    let row = entry.1.clone();
    cache.push(entry);
    Some(row)
}

pub(crate) fn put_cached_full(path: &str, row: FullRow) {
    let mut cache = full_cache().lock().unwrap();
    if let Some(pos) = cache.iter().position(|(p, _)| p == path) {
        cache.remove(pos);
    } else if cache.len() >= FULL_CACHE_CAP {
        cache.remove(0);
    }
    cache.push((path.to_owned(), row));
}

/// 面板（宽由 `h_resizable` 分栏管：默认随断点 280/320/380、拖把手 240-560
///（窄窗口按 `panel_max` 再收，表格保底三成）；
/// 边框，无阴影）：单选表单 / 批量修改，按选中数切换——两个
/// 分支各按「单选路径 / 批量集合」memo，集合没变不重建（草稿留得住）。
///
/// `reload`（页面级信号）：标签历史还原写盘后加一，单选面板按磁盘新值重建（输入框
/// 是非受控的）；那次重建里单选面板拿 `fresh = true`，不用索引 / 缓存的旧值。
pub(crate) fn panel(
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    close: Rc<dyn Fn()>,
    reload: Signal<u32>,
) -> View {
    let seen_reload = std::cell::Cell::new(0u32);
    let single = computed(move || {
        selected.with(|set| (set.len() == 1).then(|| set.iter().next().unwrap().clone()))
    });
    let batch = computed(move || {
        let set = selected.get();
        (set.len() >= 2).then(|| Rc::new(set.iter().cloned().collect::<Vec<String>>()))
    });
    view()
        .size_full()
        .rounded(px(12.0))
        .border_1()
        .border_color(border())
        .bg(nsh_base::theme().card)
        .overflow_hidden()
        .child(dynamic(move || {
            // 这次重建是不是 `reload` 引起的（只有那一次要无视旧缓存）。
            let generation = reload.get();
            let fresh = generation != seen_reload.replace(generation);
            if let Some(path) = single.get() {
                return single_panel(path, displayed, rows, selected, close.clone(), fresh)
                    .into_element();
            }
            if let Some(paths) = batch.get() {
                return batch_panel(paths, displayed, rows, selected, close.clone()).into_element();
            }
            view().into_element()
        }))
}

/// 固定顶栏（滚动区外）：高 36pt 与表格表头等高对齐 + 底部分隔线，
/// 标题件横排后接一颗关闭钮（清空选中集，面板随之收掉）。
pub(crate) fn panel_header(parts: Vec<AnyElement>, close: Rc<dyn Fn()>) -> View {
    view()
        .h(px(36.0))
        .px(px(16.0))
        .border_b_1()
        .border_b_color(border())
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .children(parts)
        .child(icon_button(
            || Icon::Close,
            12.0,
            "关闭",
            muted_fg,
            move || {
                close();
            },
        ))
}

// --- 单选编辑 ------------------------------------------------------------------
