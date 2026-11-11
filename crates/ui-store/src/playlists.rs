use super::*;

// --- 歌单 ---------------------------------------------------------------------

/// 「我喜欢」歌单当前的曲目路径（没有时为空）。
pub(crate) fn favorite_paths(lists: &[Playlist]) -> Vec<String> {
    lists
        .iter()
        .find(|p| p.favorite)
        .map(|p| match &p.kind {
            Kind::Manual { tracks } => tracks.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

/// 改歌单并保存（文件只有几 KB，直接在界面线程写）。
pub(crate) fn change_playlists(f: impl FnOnce(&mut Vec<Playlist>)) {
    let s = store();
    let mut lists = s.playlists.with_untracked(|l| l.0.as_ref().clone());
    let fav_before = favorite_paths(&lists);
    f(&mut lists);
    if let Err(err) = zeo_playlists::save(&lists) {
        eprintln!("zeo-player: 歌单保存失败: {err}");
    }
    // 「我喜欢」的差集同步 likes 表（首页「新增收藏」要 liked_at）：
    // 所有改收藏的路径都会过这里——点喜欢、右键「加到我喜欢」、歌单编辑。
    let fav_after = favorite_paths(&lists);
    if fav_before != fav_after {
        let added: Vec<String> = fav_after
            .iter()
            .filter(|p| !fav_before.contains(p))
            .cloned()
            .collect();
        let removed: Vec<String> = fav_before
            .iter()
            .filter(|p| !fav_after.contains(p))
            .cloned()
            .collect();
        let now = zeo_types::config::unix_now();
        zeo_db::later(move || {
            for p in &added {
                zeo_db::record_like(p, now);
            }
            for p in &removed {
                zeo_db::remove_like(p);
            }
        });
    }
    s.playlists.set(Same(Arc::new(lists)));
}

/// 按 id 取歌单。
pub fn playlist(id: u64) -> Option<Playlist> {
    store()
        .playlists
        .with_untracked(|l| l.iter().find(|p| p.id == id).cloned())
}

/// 打开新建弹窗（`smart`：智能歌单；`add`：建好后加入的曲目）。
pub fn new_playlist(smart: bool, add: Vec<String>) {
    let name = if smart { "智能歌单" } else { "新歌单" };
    store().editor.set(Some(Draft {
        id: None,
        name: name.to_owned(),
        rule: smart.then(Rule::default),
        add,
    }));
}

/// 打开编辑弹窗（「我喜欢」不能改名）。
pub fn edit_playlist(id: u64) {
    if let Some(p) = playlist(id).filter(|p| !p.favorite) {
        let rule = match p.kind {
            Kind::Smart { rule } => Some(rule),
            Kind::Manual { .. } => None,
        };
        store().editor.set(Some(Draft {
            id: Some(id),
            name: p.name,
            rule,
            add: Vec::new(),
        }));
    }
}

/// 保存弹窗里的草稿；新建的歌单会打开。返回歌单 id。
pub fn save_draft(draft: Draft) -> u64 {
    let name = match draft.name.trim() {
        "" if draft.rule.is_some() => "智能歌单".to_owned(),
        "" => "新歌单".to_owned(),
        name => name.to_owned(),
    };
    let mut id = 0;
    change_playlists(|lists| {
        match draft
            .id
            .and_then(|id| lists.iter_mut().find(|p| p.id == id))
        {
            Some(p) => {
                id = p.id;
                p.name = name;
                if let (Kind::Smart { rule }, Some(new)) = (&mut p.kind, draft.rule) {
                    *rule = new;
                }
            }
            None => {
                id = zeo_playlists::next_id(lists);
                let kind = match draft.rule {
                    Some(rule) => Kind::Smart { rule },
                    None => Kind::Manual {
                        tracks: dedup(draft.add),
                    },
                };
                lists.push(Playlist {
                    id,
                    name,
                    kind,
                    favorite: false,
                });
            }
        }
    });
    store().editor.set(None);
    if draft.id.is_none() {
        store().search.set(String::new());
        navigate(crate::Nav::Playlist(id));
    }
    id
}

pub(crate) fn dedup(paths: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    paths
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

/// 删除歌单（正看着它时回到文件夹页）。「我喜欢」不能删除。
pub fn delete_playlist(id: u64) {
    if playlist(id).is_none_or(|p| p.favorite) {
        return;
    }
    change_playlists(|lists| lists.retain(|p| p.id != id));
    if global::<crate::Nav>().get_untracked() == crate::Nav::Playlist(id) {
        navigate(crate::Nav::Home);
    }
}

/// 把曲目加到手动歌单末尾（已在里面的不重复加）。
pub fn add_to_playlist(id: u64, paths: Vec<String>) {
    change_playlists(|lists| {
        if let Some(Playlist {
            kind: Kind::Manual { tracks },
            ..
        }) = lists.iter_mut().find(|p| p.id == id)
        {
            for path in paths {
                if !tracks.contains(&path) {
                    tracks.push(path);
                }
            }
        }
    });
}

/// 从手动歌单里移除一首。
pub fn remove_from_playlist(id: u64, path: &str) {
    change_playlists(|lists| {
        if let Some(Playlist {
            kind: Kind::Manual { tracks },
            ..
        }) = lists.iter_mut().find(|p| p.id == id)
        {
            tracks.retain(|p| p != path);
        }
    });
}

/// `path` 在「我喜欢」里。读取即订阅。
pub fn is_liked(path: &str) -> bool {
    store().playlists.with(|l| {
        l.iter().any(|p| {
            p.favorite
                && matches!(&p.kind, Kind::Manual { tracks } if tracks.iter().any(|t| t == path))
        })
    })
}

/// 喜欢 / 取消喜欢 `path`：加到「我喜欢」最前面，或从中移除。
pub fn toggle_like(path: &str) {
    change_playlists(|lists| {
        if let Some(Playlist {
            kind: Kind::Manual { tracks },
            ..
        }) = lists.iter_mut().find(|p| p.favorite)
        {
            if let Some(at) = tracks.iter().position(|t| t == path) {
                tracks.remove(at);
            } else {
                tracks.insert(0, path.to_owned());
            }
        }
    });
}

/// 当前曲目在「我喜欢」里。读取即订阅。
pub fn current_liked() -> bool {
    store()
        .track
        .with(|t| t.path.clone())
        .is_some_and(|p| is_liked(&p))
}

/// 喜欢 / 取消喜欢当前曲目（没有曲目时什么也不做）。
pub fn toggle_like_current() {
    if let Some(path) = store().track.with_untracked(|t| t.path.clone()) {
        toggle_like(&path);
    }
}
