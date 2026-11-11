use std::collections::HashMap;
use std::path::Path;

use super::TagField;
use crate::{first, joined, number, pair, parse_gain, parse_peak, strings};
use lofty::config::ParseOptions;
use lofty::file::{TaggedFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag, TagType};

/// 读出当前标签值作为「编辑元数据」的初值：缺失字段为空串；读取的标签
/// 与读取顺序与 `read_meta` 一致（主标签 → 第一个非 ID3v1 → ID3v1）。
pub fn read_fields(path: &Path) -> Result<HashMap<TagField, String>, String> {
    read_row(path).map(|(fields, _)| fields)
}

/// 同 [`read_fields`]，连同读取来源的标签类型显示名（元数据视图的技术
/// 参数卡片用；文件里没有标签时为 `None`）。两者共享同一次 lofty 读取。
pub fn read_row(path: &Path) -> Result<(HashMap<TagField, String>, Option<&'static str>), String> {
    let tagged = Probe::open(path)
        .map_err(|e| format!("无法打开文件: {e}"))?
        .options(
            ParseOptions::new()
                .read_properties(false)
                .read_cover_art(false),
        )
        .read()
        .map_err(|e| format!("读取标签失败: {e}"))?;
    let mut values: HashMap<TagField, String> =
        TagField::ALL.iter().map(|f| (*f, String::new())).collect();
    let tag_type = read_tag_type(&tagged).map(tag_type_display);
    let Some(tag) = read_tag(&tagged) else {
        return Ok((values, tag_type));
    };
    let mut put = |field: TagField, value: Option<String>| {
        if let Some(v) = value.filter(|s| !s.trim().is_empty()) {
            values.insert(field, v);
        }
    };
    put(TagField::Title, first(tag, ItemKey::TrackTitle));
    put(
        TagField::Artist,
        joined(tag, &[ItemKey::TrackArtist, ItemKey::TrackArtists]),
    );
    put(TagField::Album, first(tag, ItemKey::AlbumTitle));
    put(
        TagField::AlbumArtist,
        joined(tag, &[ItemKey::AlbumArtist, ItemKey::AlbumArtists]),
    );
    put(TagField::Genre, {
        let list = strings(tag, ItemKey::Genre);
        (!list.is_empty()).then(|| list.join("; "))
    });
    put(
        TagField::Year,
        number(tag, ItemKey::Year)
            .or_else(|| tag.date().map(|d| u32::from(d.year)))
            .map(|y| y.to_string()),
    );
    let (track, track_total) = pair(tag, ItemKey::TrackNumber);
    put(TagField::Track, track.map(|n| n.to_string()));
    put(
        TagField::TrackTotal,
        number(tag, ItemKey::TrackTotal)
            .or(track_total)
            .map(|n| n.to_string()),
    );
    let (disc, disc_total) = pair(tag, ItemKey::DiscNumber);
    put(TagField::Disc, disc.map(|n| n.to_string()));
    put(
        TagField::DiscTotal,
        number(tag, ItemKey::DiscTotal)
            .or(disc_total)
            .map(|n| n.to_string()),
    );
    put(TagField::Composer, {
        let list = strings(tag, ItemKey::Composer);
        (!list.is_empty()).then(|| list.join("; "))
    });
    put(TagField::Comment, first(tag, ItemKey::Comment));
    put(
        TagField::Lyrics,
        first(tag, ItemKey::Lyrics).or_else(|| first(tag, ItemKey::UnsyncLyrics)),
    );
    put(TagField::Conductor, first(tag, ItemKey::Conductor));
    put(TagField::Lyricist, first(tag, ItemKey::Lyricist));
    // Label 是 Publisher 的别名键（同一帧）：Publisher 缺失时拿它顶。
    put(
        TagField::Publisher,
        first(tag, ItemKey::Publisher).or_else(|| first(tag, ItemKey::Label)),
    );
    put(TagField::Copyright, first(tag, ItemKey::CopyrightMessage));
    put(
        TagField::Rating,
        tag.ratings().next().map(|p| (p.rating() as u8).to_string()),
    );
    // ReplayGain 读回规范形态（增益两位小数不带单位、峰值六位小数），
    // 与写入侧一致；解析不了就当没有。
    put(
        TagField::TrackGain,
        first(tag, ItemKey::ReplayGainTrackGain)
            .and_then(|s| parse_gain(&s))
            .map(|v| format!("{v:.2}")),
    );
    put(
        TagField::TrackPeak,
        first(tag, ItemKey::ReplayGainTrackPeak)
            .and_then(|s| parse_peak(&s))
            .map(|v| format!("{v:.6}")),
    );
    put(
        TagField::AlbumGain,
        first(tag, ItemKey::ReplayGainAlbumGain)
            .and_then(|s| parse_gain(&s))
            .map(|v| format!("{v:.2}")),
    );
    put(
        TagField::AlbumPeak,
        first(tag, ItemKey::ReplayGainAlbumPeak)
            .and_then(|s| parse_peak(&s))
            .map(|v| format!("{v:.6}")),
    );
    Ok((values, tag_type))
}

/// 标签类型的显示名（[`read_row`] 的第二项）。
fn tag_type_display(tag_type: TagType) -> &'static str {
    match tag_type {
        TagType::Ape => "APE",
        TagType::Id3v1 => "ID3v1",
        TagType::Id3v2 => "ID3v2",
        TagType::Mp4Ilst => "MP4 ilst",
        TagType::VorbisComments => "Vorbis Comments",
        TagType::RiffInfo => "RIFF INFO",
        TagType::AiffText => "AIFF Text",
        // TagType 是 non_exhaustive：上游新增的类型给个兜底名。
        _ => "其他",
    }
}

/// 读取来源的标签类型：主标签 → 第一个非 ID3v1 → ID3v1（ID3v1 字段短、
/// 存不下图片，排在最后）。
pub(crate) fn read_tag_type(tagged: &TaggedFile) -> Option<TagType> {
    let primary = tagged.primary_tag_type();
    let tags = tagged.tags();
    tags.iter()
        .find(|tag| tag.tag_type() == primary)
        .or_else(|| tags.iter().find(|tag| tag.tag_type() != TagType::Id3v1))
        .or_else(|| tags.first())
        .map(Tag::tag_type)
}

/// 读取用的那张标签（`read_fields` / `seed_tag` 的来源）。
pub(crate) fn read_tag(tagged: &TaggedFile) -> Option<&Tag> {
    read_tag_type(tagged).and_then(|tag_type| tagged.tag(tag_type))
}
