use super::*;

/// 标签列：key、标题、宽（pt）、对齐、可排序（spec §一）。
pub(crate) const COLUMNS: &[(&str, &str, f32, DataTableAlign, bool)] = &[
    ("file_name", "文件名", 200.0, DataTableAlign::Start, true),
    ("title", "标题", 180.0, DataTableAlign::Start, true),
    ("artist", "艺术家", 150.0, DataTableAlign::Start, true),
    ("album", "专辑", 160.0, DataTableAlign::Start, true),
    (
        "album_artist",
        "专辑艺术家",
        140.0,
        DataTableAlign::Start,
        true,
    ),
    ("genre", "流派", 120.0, DataTableAlign::Start, true),
    ("year", "年份", 75.0, DataTableAlign::End, true),
    ("track_number", "音轨号", 70.0, DataTableAlign::End, true),
    ("track_total", "音轨总数", 70.0, DataTableAlign::End, true),
    ("disc_number", "碟号", 65.0, DataTableAlign::End, true),
    ("discs_total", "碟总数", 65.0, DataTableAlign::End, true),
    ("composer", "作曲", 130.0, DataTableAlign::Start, true),
    ("conductor", "指挥", 120.0, DataTableAlign::Start, true),
    ("lyricist", "作词", 120.0, DataTableAlign::Start, true),
    ("publisher", "发行商", 130.0, DataTableAlign::Start, true),
    ("comment", "注释", 150.0, DataTableAlign::Start, true),
    ("lyrics", "歌词", 160.0, DataTableAlign::Start, true),
    ("copyright", "版权", 140.0, DataTableAlign::Start, true),
    ("rating", "评分", 85.0, DataTableAlign::Center, true),
    ("pictures", "封面状态", 80.0, DataTableAlign::Center, true),
    (
        "track_replay_gain",
        "音轨增益",
        85.0,
        DataTableAlign::End,
        true,
    ),
    (
        "track_replay_peak",
        "音轨峰值",
        80.0,
        DataTableAlign::End,
        true,
    ),
    (
        "album_replay_gain",
        "专辑增益",
        85.0,
        DataTableAlign::End,
        true,
    ),
    (
        "album_replay_peak",
        "专辑峰值",
        80.0,
        DataTableAlign::End,
        true,
    ),
];

/// 数值比较的列（增益剥 ` dB` 尾解析，解析不出回落文本序；评分比星数）。
pub(crate) const NUMERIC: &[&str] = &[
    "year",
    "track_number",
    "track_total",
    "disc_number",
    "discs_total",
    "rating",
    "track_replay_gain",
    "track_replay_peak",
    "album_replay_gain",
    "album_replay_peak",
];

/// 一个单元格的显示文本：行已加载用标签值（增益补 ` dB`、评分画星、
/// 封面状态译名），未加载回落 `TrackEntry` 索引值（多余列为空）。
#[allow(dead_code)]
pub(crate) fn cell_text(key: &str, track: &TrackEntry, row: Option<&MetaRow>) -> String {
    cell_value(key, track, row).into_owned()
}

pub(crate) fn field_str(row: &MetaRow, f: TagField) -> &str {
    row.fields.get(&f).map(|s| s.as_str()).unwrap_or("")
}

/// 单元格零分配提取：借用已加载行字段或 `TrackEntry` 现有字串，减少渲染期
/// 堆分配与中间临时 String 产生。
pub(crate) fn cell_value<'a>(
    key: &str,
    track: &'a TrackEntry,
    row: Option<&'a MetaRow>,
) -> Cow<'a, str> {
    match row {
        Some(row) => loaded_cell_cow(key, track, row),
        None => entry_cell_cow(key, track),
    }
}

pub(crate) fn loaded_cell_cow<'a>(
    key: &str,
    track: &'a TrackEntry,
    row: &'a MetaRow,
) -> Cow<'a, str> {
    let gain = |f: TagField| {
        let v = field_str(row, f);
        if v.is_empty() {
            Cow::Borrowed("")
        } else {
            Cow::Owned(format!("{v} dB"))
        }
    };
    match key {
        "file_name" => Cow::Borrowed(track.file_name.as_str()),
        "title" => Cow::Borrowed(field_str(row, TagField::Title)),
        "artist" => Cow::Borrowed(field_str(row, TagField::Artist)),
        "album" => Cow::Borrowed(field_str(row, TagField::Album)),
        "album_artist" => Cow::Borrowed(field_str(row, TagField::AlbumArtist)),
        "genre" => Cow::Borrowed(field_str(row, TagField::Genre)),
        "year" => Cow::Borrowed(field_str(row, TagField::Year)),
        "track_number" => Cow::Borrowed(field_str(row, TagField::Track)),
        "track_total" => Cow::Borrowed(field_str(row, TagField::TrackTotal)),
        "disc_number" => Cow::Borrowed(field_str(row, TagField::Disc)),
        "discs_total" => Cow::Borrowed(field_str(row, TagField::DiscTotal)),
        "composer" => Cow::Borrowed(field_str(row, TagField::Composer)),
        "conductor" => Cow::Borrowed(field_str(row, TagField::Conductor)),
        "lyricist" => Cow::Borrowed(field_str(row, TagField::Lyricist)),
        "publisher" => Cow::Borrowed(field_str(row, TagField::Publisher)),
        "comment" => Cow::Borrowed(field_str(row, TagField::Comment)),
        "lyrics" => Cow::Borrowed(field_str(row, TagField::Lyrics)),
        "copyright" => Cow::Borrowed(field_str(row, TagField::Copyright)),
        "rating" => match field_str(row, TagField::Rating) {
            "1" => Cow::Borrowed("★"),
            "2" => Cow::Borrowed("★★"),
            "3" => Cow::Borrowed("★★★"),
            "4" => Cow::Borrowed("★★★★"),
            "5" => Cow::Borrowed("★★★★★"),
            s => match s.parse::<usize>() {
                Ok(n) if n > 0 => Cow::Owned("★".repeat(n)),
                _ => Cow::Borrowed(""),
            },
        },
        "pictures" => match row.cover {
            CoverState::Embedded => Cow::Borrowed("内嵌"),
            CoverState::File => Cow::Borrowed("文件"),
            CoverState::None => Cow::Borrowed("无"),
        },
        "track_replay_gain" => gain(TagField::TrackGain),
        "track_replay_peak" => Cow::Borrowed(field_str(row, TagField::TrackPeak)),
        "album_replay_gain" => gain(TagField::AlbumGain),
        "album_replay_peak" => Cow::Borrowed(field_str(row, TagField::AlbumPeak)),
        _ => Cow::Borrowed(""),
    }
}

/// 直接读取曲目索引条目（`TrackEntry` / `MetaInfo`）对应的标签值。
/// 数据库字段无回退，真实标签缺失时保持空串。
pub(crate) fn entry_cell_cow<'a>(key: &str, t: &'a TrackEntry) -> Cow<'a, str> {
    let num = |v: Option<u32>| match v {
        Some(n) => Cow::Owned(n.to_string()),
        None => Cow::Borrowed(""),
    };
    let gain = |v: Option<f32>| match v {
        Some(v) => Cow::Owned(format!("{v:.2} dB")),
        None => Cow::Borrowed(""),
    };
    let peak = |v: Option<f32>| match v {
        Some(v) => Cow::Owned(format!("{v:.6}")),
        None => Cow::Borrowed(""),
    };
    match key {
        "file_name" => Cow::Borrowed(t.file_name.as_str()),
        "title" => Cow::Borrowed(t.title.as_str()),
        "artist" => Cow::Borrowed(t.artist.as_str()),
        "album" => Cow::Borrowed(t.album.as_str()),
        "album_artist" => Cow::Borrowed(t.meta.album_artist.as_str()),
        "genre" => {
            if t.genres.is_empty() {
                Cow::Borrowed("")
            } else if t.genres.len() == 1 {
                Cow::Borrowed(t.genres[0].as_str())
            } else {
                Cow::Owned(t.genres.join("; "))
            }
        }
        "year" => num(t.year),
        "track_number" => num(t.track),
        "track_total" => num(t.track_total),
        "disc_number" => num(t.disk),
        "discs_total" => num(t.disk_total),
        "composer" => {
            if t.composers.is_empty() {
                Cow::Borrowed("")
            } else if t.composers.len() == 1 {
                Cow::Borrowed(t.composers[0].as_str())
            } else {
                Cow::Owned(t.composers.join("; "))
            }
        }
        "conductor" => Cow::Borrowed(t.conductor.as_deref().unwrap_or("")),
        "lyricist" => Cow::Borrowed(t.lyricist.as_deref().unwrap_or("")),
        "publisher" => Cow::Borrowed(t.publisher.as_deref().unwrap_or("")),
        "comment" => Cow::Borrowed(t.comment.as_deref().unwrap_or("")),
        "lyrics" => match t.lyrics.as_deref() {
            Some(lrc) => Cow::Owned(crate::lyrics_preview(lrc)),
            None => Cow::Borrowed(""),
        },
        "copyright" => Cow::Borrowed(t.copyright.as_deref().unwrap_or("")),
        "rating" => match t.rating {
            Some(1) => Cow::Borrowed("★"),
            Some(2) => Cow::Borrowed("★★"),
            Some(3) => Cow::Borrowed("★★★"),
            Some(4) => Cow::Borrowed("★★★★"),
            Some(5) => Cow::Borrowed("★★★★★"),
            Some(n) if n > 0 => Cow::Owned("★".repeat(n as usize)),
            _ => Cow::Borrowed(""),
        },
        "pictures" => {
            if zeo_metadata::covers::find_sidecar(std::path::Path::new(&t.path)).is_some() {
                Cow::Borrowed("文件")
            } else if t.has_art {
                Cow::Borrowed("内嵌")
            } else {
                Cow::Borrowed("无")
            }
        }
        "track_replay_gain" => gain(t.replaygain.track_gain),
        "track_replay_peak" => peak(t.replaygain.track_peak),
        "album_replay_gain" => gain(t.replaygain.album_gain),
        "album_replay_peak" => peak(t.replaygain.album_peak),
        _ => Cow::Borrowed(""),
    }
}

/// 单元格比较（`displayed` 排序用，纯函数）：空值恒排最后（与方向
/// 无关）；数值列按数字（增益剥 ` dB`，非数字回落文本序）；文本列
/// 不分大小写。
#[allow(dead_code)]
pub(crate) fn compare_cells(key: &str, a: &str, b: &str, ascending: bool) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    let ord = if NUMERIC.contains(&key) {
        let parse = |s: &str| s.trim_end_matches(" dB").parse::<f64>().ok();
        match (parse(a), parse(b)) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => compare_text(a, b),
        }
    } else {
        compare_text(a, b)
    };
    if ascending { ord } else { ord.reverse() }
}

pub(crate) fn compare_text(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        for (ca, cb) in a.bytes().zip(b.bytes()) {
            let ord = ca.to_ascii_lowercase().cmp(&cb.to_ascii_lowercase());
            if ord != Ordering::Equal {
                return ord;
            }
        }
        a.len().cmp(&b.len()).then_with(|| a.cmp(b))
    } else {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    }
}
