use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use zeo_types::config::{TrackEntry, unix_now};

use crate::Rule;

/// 一个歌单。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: u64,
    pub name: String,
    #[serde(flatten)]
    pub kind: Kind,
    /// 固定的「我喜欢」歌单（手动歌单，不能改名或删除）。旧文件里没有
    /// 这个字段；旧版本读到它会当作普通手动歌单。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub favorite: bool,
}

/// 「我喜欢」歌单的名称。
pub const FAVORITES_NAME: &str = "我喜欢";

/// 歌单的种类。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Kind {
    /// 手动：曲目路径，按加入顺序（不在曲库里的路径保留，显示时跳过）。
    Manual { tracks: Vec<String> },
    /// 智能：规则求值的结果。
    Smart { rule: Rule },
}

impl Playlist {
    pub fn is_smart(&self) -> bool {
        matches!(self.kind, Kind::Smart { .. })
    }

    /// 歌单里的曲目（按显示顺序）。
    pub fn tracks(&self, library: &[TrackEntry]) -> Vec<TrackEntry> {
        match &self.kind {
            Kind::Manual { tracks } => {
                let by_path: HashMap<&str, &TrackEntry> =
                    library.iter().map(|t| (t.path.as_str(), t)).collect();
                tracks
                    .iter()
                    .filter_map(|p| by_path.get(p.as_str()).map(|t| (*t).clone()))
                    .collect()
            }
            Kind::Smart { rule } => rule.select(library, unix_now()),
        }
    }
}
