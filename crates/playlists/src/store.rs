use std::path::{Path, PathBuf};

use crate::{FAVORITES_NAME, Kind, Playlist};
use serde::{Deserialize, Serialize};

// --- 持久化 -------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default)]
struct File {
    #[serde(default)]
    playlists: Vec<Playlist>,
}

fn file_path() -> PathBuf {
    zeo_types::config::data_dir().join("playlists.json")
}

/// 读出全部歌单（文件不存在或损坏时为空）。
pub fn load() -> Vec<Playlist> {
    load_from(&file_path())
}

pub(crate) fn load_from(path: &Path) -> Vec<Playlist> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<File>(&bytes).ok())
        .map(|f| f.playlists)
        .unwrap_or_default()
}

/// 保存全部歌单（先写临时文件再改名，写到一半不会留下坏文件）。
pub fn save(playlists: &[Playlist]) -> Result<(), String> {
    save_to(&file_path(), playlists)
}

pub(crate) fn save_to(path: &Path, playlists: &[Playlist]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(&File {
        playlists: playlists.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// 新歌单的 id：比现有的都大。
pub fn next_id(playlists: &[Playlist]) -> u64 {
    playlists.iter().map(|p| p.id).max().unwrap_or(0) + 1
}

/// 保证有且只有一个「我喜欢」歌单，并排在最前（没有时新建一个空的）。
pub fn ensure_favorites(playlists: &mut Vec<Playlist>) {
    let mut found = false;
    for p in playlists.iter_mut() {
        // 只认第一个；其余的（手改文件才会有）降为普通歌单。
        p.favorite = p.favorite && !found && !p.is_smart();
        found |= p.favorite;
    }
    if let Some(at) = playlists.iter().position(|p| p.favorite) {
        let favorites = playlists.remove(at);
        playlists.insert(0, favorites);
    } else {
        let id = next_id(playlists);
        playlists.insert(
            0,
            Playlist {
                id,
                name: FAVORITES_NAME.to_owned(),
                kind: Kind::Manual { tracks: Vec::new() },
                favorite: true,
            },
        );
    }
}
