//! 歌单：手动歌单（按路径记下曲目与顺序）与智能歌单（按元数据、标签、音频参数、
//! 文件属性的条件从曲库里自动聚合）。持久化到数据目录的 `playlists.json`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{TrackEntry, unix_now};

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

/// 智能歌单的规则。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// 全部条件都满足（`true`）还是满足任一（`false`）。
    #[serde(default = "yes")]
    pub match_all: bool,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub sort: Sort,
    /// 只取排序后的前 N 首（`None`：不限）。
    #[serde(default)]
    pub limit: Option<usize>,
}

fn yes() -> bool {
    true
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            match_all: true,
            conditions: vec![Condition::default()],
            sort: Sort::default(),
            limit: None,
        }
    }
}

/// 一个条件：字段、比较方式、值（文本；数值字段按字段的单位解析）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub field: Field,
    pub op: Op,
    #[serde(default)]
    pub value: String,
}

impl Default for Condition {
    fn default() -> Self {
        Condition {
            field: Field::Artist,
            op: Op::Contains,
            value: String::new(),
        }
    }
}

/// 条件可用的字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    // 元数据
    Title,
    Artist,
    Album,
    AlbumArtist,
    /// 标签：genre 按设置的分隔符拆开的值。文件里沿用旧名 `genre`。
    #[serde(rename = "genre")]
    Tag,
    Year,
    // 音频
    Format,
    Duration,
    SampleRate,
    BitRate,
    Channels,
    // 文件
    Folder,
    FileName,
    Size,
    Modified,
}

/// 字段的取值类型（决定可用的比较方式与值的解析）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Text,
    Number,
    /// 修改时间：按「天前」比较。
    Date,
}

impl Field {
    /// 选择器里的顺序，按组：元数据、音频、文件。
    pub const ALL: [Field; 15] = [
        Field::Title,
        Field::Artist,
        Field::Album,
        Field::AlbumArtist,
        Field::Tag,
        Field::Year,
        Field::Format,
        Field::Duration,
        Field::SampleRate,
        Field::BitRate,
        Field::Channels,
        Field::Folder,
        Field::FileName,
        Field::Size,
        Field::Modified,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::Title => "标题",
            Field::Artist => "歌手",
            Field::Album => "专辑",
            Field::AlbumArtist => "专辑歌手",
            Field::Tag => "标签",
            Field::Year => "年份",
            Field::Format => "格式",
            Field::Duration => "时长",
            Field::SampleRate => "采样率",
            Field::BitRate => "比特率",
            Field::Channels => "声道数",
            Field::Folder => "文件夹",
            Field::FileName => "文件名",
            Field::Size => "文件大小",
            Field::Modified => "修改时间",
        }
    }

    /// 分组名（选择器里的小标题）。
    pub fn group(self) -> &'static str {
        match self {
            Field::Title
            | Field::Artist
            | Field::Album
            | Field::AlbumArtist
            | Field::Tag
            | Field::Year => "元数据",
            Field::Format
            | Field::Duration
            | Field::SampleRate
            | Field::BitRate
            | Field::Channels => "音频",
            Field::Folder | Field::FileName | Field::Size | Field::Modified => "文件",
        }
    }

    pub fn kind(self) -> FieldType {
        match self {
            Field::Year
            | Field::Duration
            | Field::SampleRate
            | Field::BitRate
            | Field::Channels
            | Field::Size => FieldType::Number,
            Field::Modified => FieldType::Date,
            _ => FieldType::Text,
        }
    }

    /// 值的单位（输入框旁的提示）。
    pub fn unit(self) -> &'static str {
        match self {
            Field::Duration => "分:秒 或 秒",
            Field::SampleRate => "kHz",
            Field::BitRate => "kbps",
            Field::Size => "MB",
            Field::Modified => "天",
            _ => "",
        }
    }

    /// 可用的比较方式（第一个为默认）。
    pub fn ops(self) -> &'static [Op] {
        match self.kind() {
            // 格式与标签的值都从列表里选，只有「是 / 不是」有意义。
            FieldType::Text if matches!(self, Field::Format | Field::Tag) => {
                &[Op::Is, Op::IsNot]
            }
            FieldType::Text => &[
                Op::Contains,
                Op::NotContains,
                Op::Is,
                Op::IsNot,
                Op::StartsWith,
                Op::EndsWith,
            ],
            FieldType::Number => &[Op::Is, Op::IsNot, Op::Greater, Op::Less],
            FieldType::Date => &[Op::InLast, Op::NotInLast],
        }
    }

    /// 文本字段的取值（标签可能有多个）。
    fn texts(self, t: &TrackEntry) -> Vec<String> {
        match self {
            Field::Title => vec![t.title.clone()],
            Field::Artist => vec![t.artist.clone()],
            Field::Album => vec![t.album.clone()],
            Field::AlbumArtist => vec![t.album_artist.clone()],
            Field::Tag => t.genres.clone(),
            Field::Format => vec![extension(&t.path)],
            Field::Folder => vec![
                Path::new(&t.path)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ],
            Field::FileName => vec![t.file_name.clone()],
            _ => Vec::new(),
        }
    }

    /// 数值字段的取值，单位同 [`Field::unit`]（没有这项数据时为 `None`）。
    fn number(self, t: &TrackEntry) -> Option<f64> {
        match self {
            Field::Year => t.year.map(f64::from),
            Field::Duration => Some(t.duration),
            Field::SampleRate => t.sample_rate.map(|r| r as f64 / 1000.0),
            Field::BitRate => t.bit_rate.map(f64::from),
            Field::Channels => t.channels.map(f64::from),
            Field::Size => Some(t.size_bytes as f64 / 1_000_000.0),
            _ => None,
        }
    }
}

/// 比较方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Contains,
    NotContains,
    Is,
    IsNot,
    StartsWith,
    EndsWith,
    Greater,
    Less,
    /// 最近 N 天内。
    InLast,
    /// N 天之前。
    NotInLast,
}

impl Op {
    pub fn label(self) -> &'static str {
        match self {
            Op::Contains => "包含",
            Op::NotContains => "不包含",
            Op::Is => "是",
            Op::IsNot => "不是",
            Op::StartsWith => "开头是",
            Op::EndsWith => "结尾是",
            Op::Greater => "大于",
            Op::Less => "小于",
            Op::InLast => "在最近",
            Op::NotInLast => "早于最近",
        }
    }
}

/// 智能歌单的排序。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    /// 曲库顺序（歌手、专辑、碟号、音轨号）。
    #[default]
    Library,
    Title,
    Artist,
    Album,
    /// 年份，新的在前。
    YearDesc,
    /// 最近修改的在前。
    ModifiedDesc,
    /// 最长的在前。
    DurationDesc,
    /// 比特率高的在前。
    BitRateDesc,
}

impl Sort {
    pub const ALL: [Sort; 8] = [
        Sort::Library,
        Sort::Title,
        Sort::Artist,
        Sort::Album,
        Sort::YearDesc,
        Sort::ModifiedDesc,
        Sort::DurationDesc,
        Sort::BitRateDesc,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Library => "曲库顺序",
            Sort::Title => "标题",
            Sort::Artist => "歌手",
            Sort::Album => "专辑",
            Sort::YearDesc => "年份（新到旧）",
            Sort::ModifiedDesc => "最近修改",
            Sort::DurationDesc => "时长（长到短）",
            Sort::BitRateDesc => "比特率（高到低）",
        }
    }
}

fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// 解析数值条件的值：时长接受 `m:ss` / `h:mm:ss` 或秒数，其余为十进制数。
pub fn parse_number(field: Field, value: &str) -> Option<f64> {
    let value = value.trim();
    if field == Field::Duration && value.contains(':') {
        let mut total = 0.0;
        for part in value.split(':') {
            total = total * 60.0 + part.trim().parse::<f64>().ok()?;
        }
        return Some(total);
    }
    value.parse::<f64>().ok().filter(|v| v.is_finite())
}

impl Condition {
    /// 值为空或解析不了的条件不参与筛选（编辑到一半时列表不清空）。
    pub fn is_complete(&self) -> bool {
        match self.field.kind() {
            FieldType::Text => !self.value.trim().is_empty(),
            FieldType::Number | FieldType::Date => parse_number(self.field, &self.value).is_some(),
        }
    }

    /// `now`：当前 Unix 秒（修改时间条件用）。
    pub fn matches(&self, t: &TrackEntry, now: u64) -> bool {
        match self.field.kind() {
            FieldType::Text => {
                let want = self.value.trim().to_lowercase();
                let values: Vec<String> = self
                    .field
                    .texts(t)
                    .into_iter()
                    .map(|v| v.to_lowercase())
                    .collect();
                let any = |f: &dyn Fn(&str) -> bool| values.iter().any(|v| f(v));
                match self.op {
                    Op::Contains => any(&|v| v.contains(&want)),
                    Op::NotContains => !any(&|v| v.contains(&want)),
                    Op::Is => any(&|v| v == want),
                    Op::IsNot => !any(&|v| v == want),
                    Op::StartsWith => any(&|v| v.starts_with(&want)),
                    Op::EndsWith => any(&|v| v.ends_with(&want)),
                    _ => false,
                }
            }
            FieldType::Number => {
                let (Some(want), Some(have)) =
                    (parse_number(self.field, &self.value), self.field.number(t))
                else {
                    return false;
                };
                // 「是」按显示精度比较：时长差 1 秒内、其它差 0.5% 内都算相等
                // （采样率 44.1 与 44100 Hz、文件大小 3.2 MB 这类）。
                let close = match self.field {
                    Field::Duration => (have - want).abs() < 1.0,
                    Field::Year | Field::Channels => (have - want).abs() < 0.5,
                    _ => (have - want).abs() <= want.abs() * 0.005 + 1e-9,
                };
                match self.op {
                    Op::Is => close,
                    Op::IsNot => !close,
                    Op::Greater => have > want,
                    Op::Less => have < want,
                    _ => false,
                }
            }
            FieldType::Date => {
                let Some(days) = parse_number(self.field, &self.value) else {
                    return false;
                };
                let age = now.saturating_sub(t.modified) as f64 / 86_400.0;
                match self.op {
                    Op::InLast => age <= days,
                    Op::NotInLast => age > days,
                    _ => false,
                }
            }
        }
    }
}

impl Rule {
    /// 筛选、排序、截断。没有完整条件时匹配整个曲库。
    pub fn select(&self, library: &[TrackEntry], now: u64) -> Vec<TrackEntry> {
        let active: Vec<&Condition> = self.conditions.iter().filter(|c| c.is_complete()).collect();
        let mut out: Vec<TrackEntry> = library
            .iter()
            .filter(|t| {
                if active.is_empty() {
                    return true;
                }
                if self.match_all {
                    active.iter().all(|c| c.matches(t, now))
                } else {
                    active.iter().any(|c| c.matches(t, now))
                }
            })
            .cloned()
            .collect();
        let text = |a: &str, b: &str| a.to_lowercase().cmp(&b.to_lowercase());
        match self.sort {
            Sort::Library => {}
            Sort::Title => out.sort_by(|a, b| text(&a.title, &b.title)),
            Sort::Artist => out.sort_by(|a, b| text(&a.artist, &b.artist)),
            Sort::Album => out.sort_by(|a, b| {
                text(&a.album, &b.album).then((a.disk, a.track).cmp(&(b.disk, b.track)))
            }),
            Sort::YearDesc => out.sort_by_key(|t| std::cmp::Reverse(t.year)),
            Sort::ModifiedDesc => out.sort_by_key(|t| std::cmp::Reverse(t.modified)),
            Sort::DurationDesc => out.sort_by(|a, b| b.duration.total_cmp(&a.duration)),
            Sort::BitRateDesc => out.sort_by_key(|t| std::cmp::Reverse(t.bit_rate)),
        }
        if let Some(limit) = self.limit {
            out.truncate(limit);
        }
        out
    }
}

// --- 持久化 -------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default)]
struct File {
    #[serde(default)]
    playlists: Vec<Playlist>,
}

fn file_path() -> PathBuf {
    crate::config::data_dir().join("playlists.json")
}

/// 读出全部歌单（文件不存在或损坏时为空）。
pub fn load() -> Vec<Playlist> {
    load_from(&file_path())
}

fn load_from(path: &Path) -> Vec<Playlist> {
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

fn save_to(path: &Path, playlists: &[Playlist]) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn track(
        path: &str,
        artist: &str,
        genres: &[&str],
        year: Option<u32>,
        duration: f64,
    ) -> TrackEntry {
        TrackEntry {
            provider: crate::config::ProviderId::Local,
            path: path.to_owned(),
            file_name: Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            title: Path::new(path)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            artist: artist.to_owned(),
            album: "Album".to_owned(),
            album_artist: String::new(),
            genres: genres.iter().map(|g| (*g).to_owned()).collect(),
            year,
            track: None,
            disk: None,
            duration,
            bit_rate: Some(320),
            sample_rate: Some(44_100),
            channels: Some(2),
            has_art: false,
            size_bytes: 8_000_000,
            modified: 1_000_000,
            stamp: 0,
        }
    }

    fn library() -> Vec<TrackEntry> {
        vec![
            track(
                "/m/rock/a.flac",
                "Alpha",
                &["Rock", "Indie"],
                Some(1999),
                200.0,
            ),
            track("/m/pop/b.mp3", "Beta", &["Pop"], Some(2015), 185.0),
            track("/m/rock/c.mp3", "Alpha; Gamma", &["Rock"], None, 400.0),
        ]
    }

    fn cond(field: Field, op: Op, value: &str) -> Condition {
        Condition {
            field,
            op,
            value: value.to_owned(),
        }
    }

    fn names(tracks: &[TrackEntry]) -> Vec<&str> {
        tracks.iter().map(|t| t.title.as_str()).collect()
    }

    #[test]
    fn text_conditions_ignore_case_and_cover_every_tag() {
        let lib = library();
        let rule = |c: Condition| Rule {
            conditions: vec![c],
            ..Rule::default()
        };
        assert_eq!(
            names(&rule(cond(Field::Artist, Op::Contains, "alpha")).select(&lib, 0)),
            ["a", "c"]
        );
        assert_eq!(
            names(&rule(cond(Field::Tag, Op::Is, "indie")).select(&lib, 0)),
            ["a"]
        );
        assert_eq!(
            names(&rule(cond(Field::Tag, Op::IsNot, "rock")).select(&lib, 0)),
            ["b"]
        );
        assert_eq!(
            names(&rule(cond(Field::Format, Op::Is, "MP3")).select(&lib, 0)),
            ["b", "c"]
        );
        assert_eq!(
            names(&rule(cond(Field::Folder, Op::EndsWith, "/rock")).select(&lib, 0)),
            ["a", "c"]
        );
        // 值为空的条件不参与：匹配整个曲库。
        assert_eq!(
            rule(cond(Field::Title, Op::Contains, "  "))
                .select(&lib, 0)
                .len(),
            3
        );
    }

    #[test]
    fn number_and_date_conditions_use_field_units() {
        let lib = library();
        let rule = |c: Condition| Rule {
            conditions: vec![c],
            ..Rule::default()
        };
        assert_eq!(
            names(&rule(cond(Field::Duration, Op::Greater, "5:00")).select(&lib, 0)),
            ["c"]
        );
        assert_eq!(
            names(&rule(cond(Field::Duration, Op::Is, "3:05")).select(&lib, 0)),
            ["b"]
        );
        assert_eq!(
            names(&rule(cond(Field::Year, Op::Less, "2000")).select(&lib, 0)),
            ["a"],
            "没有年份的不算"
        );
        assert_eq!(
            rule(cond(Field::SampleRate, Op::Is, "44.1"))
                .select(&lib, 0)
                .len(),
            3
        );
        assert_eq!(
            rule(cond(Field::Size, Op::Greater, "7.5"))
                .select(&lib, 0)
                .len(),
            3
        );
        assert!(parse_number(Field::Duration, "1:02:03") == Some(3723.0));
        let now = 1_000_000 + 3 * 86_400;
        assert_eq!(
            rule(cond(Field::Modified, Op::InLast, "7"))
                .select(&lib, now)
                .len(),
            3
        );
        assert_eq!(
            rule(cond(Field::Modified, Op::InLast, "2"))
                .select(&lib, now)
                .len(),
            0
        );
        assert_eq!(
            rule(cond(Field::Modified, Op::NotInLast, "2"))
                .select(&lib, now)
                .len(),
            3
        );
    }

    #[test]
    fn all_or_any_then_sort_and_limit() {
        let lib = library();
        let conditions = vec![
            cond(Field::Tag, Op::Is, "rock"),
            cond(Field::Format, Op::Is, "mp3"),
        ];
        let all = Rule {
            conditions: conditions.clone(),
            ..Rule::default()
        };
        assert_eq!(names(&all.select(&lib, 0)), ["c"]);
        let any = Rule {
            match_all: false,
            conditions,
            sort: Sort::DurationDesc,
            limit: Some(2),
        };
        assert_eq!(names(&any.select(&lib, 0)), ["c", "a"]);
    }

    #[test]
    fn manual_playlists_keep_order_and_skip_missing_files() {
        let lib = library();
        let list = Playlist {
            id: 1,
            name: "x".into(),
            kind: Kind::Manual {
                tracks: vec![
                    "/m/rock/c.mp3".into(),
                    "/gone.mp3".into(),
                    "/m/rock/a.flac".into(),
                ],
            },
            favorite: false,
        };
        assert_eq!(names(&list.tracks(&lib)), ["c", "a"]);
    }

    #[test]
    fn saves_and_loads_both_kinds() {
        let dir = std::env::temp_dir().join(format!("zeo-player-playlists-{}", std::process::id()));
        let path = dir.join("playlists.json");
        let lists = vec![
            Playlist {
                id: 1,
                name: "手动".into(),
                kind: Kind::Manual {
                    tracks: vec!["/a.mp3".into()],
                },
                favorite: false,
            },
            Playlist {
                id: 2,
                name: "智能".into(),
                kind: Kind::Smart {
                    rule: Rule {
                        match_all: false,
                        conditions: vec![cond(Field::BitRate, Op::Greater, "256")],
                        sort: Sort::YearDesc,
                        limit: Some(25),
                    },
                },
                favorite: false,
            },
        ];
        save_to(&path, &lists).unwrap();
        assert_eq!(load_from(&path), lists);
        assert_eq!(next_id(&lists), 3);
        assert!(load_from(&dir.join("missing.json")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn genre_field_reads_as_tag() {
        // 旧版文件里字段叫 "genre"，读进来是标签；写回去仍是 "genre"。
        let c: Condition =
            serde_json::from_str(r#"{"field":"genre","op":"is","value":"rock"}"#).unwrap();
        assert_eq!(c.field, Field::Tag);
        assert!(serde_json::to_string(&c).unwrap().contains(r#""field":"genre""#));
    }

    #[test]
    fn favorites_are_created_once_and_kept_first() {
        let manual = |id: u64, favorite: bool| Playlist {
            id,
            name: format!("p{id}"),
            kind: Kind::Manual { tracks: Vec::new() },
            favorite,
        };
        // 旧文件：没有「我喜欢」，新建一个，id 不与现有的冲突。
        let mut lists = vec![manual(1, false), manual(4, false)];
        ensure_favorites(&mut lists);
        assert!(lists[0].favorite && lists[0].name == FAVORITES_NAME && lists[0].id == 5);
        assert_eq!(lists.iter().filter(|p| p.favorite).count(), 1);
        // 已有的移到最前；多出来的降为普通歌单。
        let mut lists = vec![manual(1, false), manual(2, true), manual(3, true)];
        ensure_favorites(&mut lists);
        assert_eq!(
            lists.iter().map(|p| (p.id, p.favorite)).collect::<Vec<_>>(),
            [(2, true), (1, false), (3, false)]
        );
        // 旧文件里没有 favorite 字段也能读；普通歌单不写这个字段。
        let old: Playlist =
            serde_json::from_str(r#"{"id":1,"name":"x","kind":"manual","tracks":[]}"#).unwrap();
        assert!(!old.favorite);
        assert!(!serde_json::to_string(&old).unwrap().contains("favorite"));
    }
}
