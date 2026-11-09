use super::*;

/// 草稿 → `TagEdits`（与 edit_info 同规：没动的字段省略，改空的 Clear）。
pub(crate) fn build_edits(d: &SingleDraft) -> TagEdits {
    TagEdits {
        fields: TagField::ALL
            .iter()
            .filter_map(|f| {
                let old = d.original.get(f).map(String::as_str).unwrap_or_default();
                let new = d.values.get(f).map(|s| s.trim()).unwrap_or_default();
                if new == old {
                    None
                } else if new.is_empty() {
                    Some((*f, Edit::Clear))
                } else {
                    Some((*f, Edit::Set(new.to_owned())))
                }
            })
            .collect(),
        cover: d.cover.clone(),
        cover_file: None,
        lrc: None,
    }
}

/// 本地校验（与 `write::validate` 同规；为空合法 = Clear）。
pub(crate) fn valid(values: &HashMap<TagField, String>) -> bool {
    use zeo_metadata::{parse_gain, parse_peak};
    values.iter().all(|(f, raw)| {
        let v = raw.trim();
        if v.is_empty() {
            return true;
        }
        match f {
            TagField::Year => v.parse::<u16>().is_ok(),
            TagField::Track | TagField::TrackTotal | TagField::Disc | TagField::DiscTotal => {
                v.parse::<u32>().is_ok_and(|n| n > 0)
            }
            TagField::Rating => matches!(v.parse::<u8>(), Ok(1..=5)),
            TagField::TrackGain | TagField::AlbumGain => parse_gain(v).is_some(),
            TagField::TrackPeak | TagField::AlbumPeak => parse_peak(v).is_some(),
            _ => true,
        }
    })
}

/// 「保存」：把草稿的 `TagEdits` 写回（后台线程，当前曲目先 release
/// 再 reload 由 `store::save_tags` 管）。成功后行失效，加载器按新指纹
/// 重读（`refresh_paths` 已换索引指纹）。改名与它分家，走顶栏。
pub(crate) fn save_single(
    draft: Signal<SingleDraft>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    alive: Arc<AtomicBool>,
) {
    let d = draft.get_untracked();
    let path = d.path.clone();
    let edits = build_edits(&d);
    if edits.is_empty() {
        return;
    }
    draft.update(|d| {
        d.saving = true;
        d.saved = false;
        d.error = None;
    });
    store::save_tags(path.clone(), edits, move |result| match result {
        Ok(()) => {
            full_cache().lock().unwrap().retain(|(p, _)| p != &path);
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            rows.update(|r| {
                r.remove(&path);
            });
            draft.update(|d| {
                d.saving = false;
                d.saved = true;
                d.original = d.values.clone();
            });
        }
        Err(error) => {
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            draft.update(|d| {
                d.saving = false;
                d.error = Some(error);
            });
        }
    });
}

/// 顶栏改名提交：`plan_name` 预检（Invalid / Conflict 留在 `err`，
/// Unchanged 直接退出改名态），Ok 后台执行（正播放的先停、含同名
/// `.lrc` / 封面侧车）；音频移走后索引 / 歌单引用与行图 / 选中集
/// 换到新路径（`single` memo 随即按新路径重建面板），执行报错留
/// `err` 与原名原位。
pub(crate) fn commit_rename(
    path: &str,
    state: RenameState,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
    alive: Arc<AtomicBool>,
) {
    let planned = rename::plan_name(path, &state.name_input.get_untracked());
    match &planned.status {
        Status::Invalid(m) | Status::Conflict(m) => {
            state.err.set(Some(m.clone()));
            return;
        }
        Status::Unchanged => {
            state.err.set(None);
            state.renaming.set(false);
            return;
        }
        Status::Ok => {}
    }
    state.busy.set(true);
    state.err.set(None);
    let src = planned.src.clone();
    let dest = planned.dest.clone();
    let post = poster();
    std::thread::spawn(move || {
        // 正播放的源先停（Windows 文件句柄占用），再改。
        stop_if_playing_under(&src);
        let (moves, error) = rename::apply(&planned);
        post.post(move || {
            if moves.iter().any(|(o, n)| o == &src && n == &dest) {
                // 应用级引用先换：面板没了（换页 / 换选）也不能让索引 /
                // 歌单指向旧路径。
                library_store().move_path(&src, &dest);
                store::move_playlist_paths(&src, &dest);
                if alive.load(Ordering::Relaxed) {
                    apply_move(&src, &dest, rows, selected);
                }
            }
            if let Some(error) = error {
                if alive.load(Ordering::Relaxed) {
                    state.busy.set(false);
                    state.err.set(Some(error));
                }
                return;
            }
            // 成功：选中集换键已把面板重建成新路径（本面板 `alive` 翻
            // false，这里多半跳过）；仍活着（面板没重建的边角）就把改
            // 名态收掉。
            if alive.load(Ordering::Relaxed) {
                state.busy.set(false);
                state.renaming.set(false);
            }
        });
    });
}

/// 单路径改名后的界面侧换键：行图与选中集从 `src` 挪到 `dest`（`single`
/// memo 随之按新路径重建面板）。与重命名弹窗同一套挪法。
pub(crate) fn apply_move(
    src: &str,
    dest: &str,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
) {
    let mut cache = full_cache().lock().unwrap();
    if let Some(pos) = cache.iter().position(|(p, _)| p == src) {
        let mut entry = cache.remove(pos);
        entry.0 = dest.to_owned();
        cache.push(entry);
    }
    rows.update(|r| {
        if let Some(row) = r.remove(src) {
            r.insert(dest.to_owned(), row);
        }
    });
    selected.update(|set| {
        if set.remove(src) {
            set.insert(dest.to_owned());
        }
    });
}

// --- 批量修改 ------------------------------------------------------------------
