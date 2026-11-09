use std::path::Path;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::ItemKey;
use lofty::tag::Tag;

use crate::{MetaInfo, ReplayGain};

/// `-6.54 dB` / `+1.2dB` / `-6.54` → -6.54；解析失败 `None`。
pub fn parse_gain(text: &str) -> Option<f32> {
    let lower = text.trim().to_lowercase();
    let digits = lower.strip_suffix("db").unwrap_or(&lower).trim();
    let value: f32 = digits.parse().ok()?;
    (value.is_finite() && (-60.0..=60.0).contains(&value)).then_some(value)
}

/// `0.988525` → 0.988525；非正数或解析失败 `None`。
pub fn parse_peak(text: &str) -> Option<f32> {
    let value: f32 = text.trim().parse().ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

/// 先查主标签再查其余标签，取第一个能解析的值。
pub(crate) fn rg_from_tagged(tagged: &lofty::file::TaggedFile) -> ReplayGain {
    let find = |key: ItemKey, parse: fn(&str) -> Option<f32>| {
        tagged
            .primary_tag()
            .into_iter()
            .chain(tagged.tags().iter())
            .find_map(|tag| first(tag, key).and_then(|text| parse(&text)))
    };
    ReplayGain {
        track_gain: find(ItemKey::ReplayGainTrackGain, parse_gain),
        track_peak: find(ItemKey::ReplayGainTrackPeak, parse_peak),
        album_gain: find(ItemKey::ReplayGainAlbumGain, parse_gain),
        album_peak: find(ItemKey::ReplayGainAlbumPeak, parse_peak),
    }
}

/// 独立按路径读一次 ReplayGain（引擎对迁移前的老行回退时用）。
pub fn read_replaygain(path: &Path) -> ReplayGain {
    lofty::read_from_path(path)
        .map(|tagged| rg_from_tagged(&tagged))
        .unwrap_or_default()
}

pub(crate) fn strings(tag: &Tag, key: ItemKey) -> Vec<String> {
    tag.get_strings(key).map(|s| s.to_owned()).collect()
}

pub(crate) fn first(tag: &Tag, key: ItemKey) -> Option<String> {
    tag.get_string(key).map(|s| s.to_owned())
}

/// 多键合并去重后用 `; ` 拼接（参考站 tag.artists 拼接方式）。
pub(crate) fn joined(tag: &Tag, keys: &[ItemKey]) -> Option<String> {
    let mut all: Vec<String> = Vec::new();
    for key in keys {
        for value in strings(tag, *key) {
            if !all.contains(&value) {
                all.push(value);
            }
        }
    }
    if all.is_empty() {
        None
    } else {
        Some(all.join("; "))
    }
}

pub(crate) fn number(tag: &Tag, key: ItemKey) -> Option<u32> {
    tag.get_string(key)?.trim().parse().ok()
}

/// 解析 `n/n` 形式的音轨/碟号。
pub(crate) fn pair(tag: &Tag, key: ItemKey) -> (Option<u32>, Option<u32>) {
    match tag.get_string(key) {
        Some(raw) => {
            let raw = raw.trim();
            match raw.split_once('/') {
                Some((left, right)) => (left.trim().parse().ok(), right.trim().parse().ok()),
                None => (raw.parse().ok(), None),
            }
        }
        None => (None, None),
    }
}

pub(crate) fn codec_of(file_type: lofty::file::FileType) -> Option<&'static str> {
    use lofty::file::FileType::*;
    Some(match file_type {
        Mpeg => "MP3",
        Flac => "FLAC",
        Wav => "WAV",
        Ape => "APE",
        Opus => "OPUS",
        Vorbis => "Vorbis",
        Aac => "AAC",
        Mp4 => "MP4",
        WavPack => "WavPack",
        Aiff => "AIFF",
        _ => return None,
    })
}

/// 读取一个音频文件的元数据。
pub fn read_meta(path: &Path) -> Result<MetaInfo, String> {
    let tagged = lofty::read_from_path(path).map_err(|e| format!("lofty: {e}"))?;
    let props = tagged.properties();
    let file_type = tagged.file_type();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let mut codec = codec_of(file_type).map(|s| s.to_owned());
    let quality = match file_type {
        lofty::file::FileType::Flac | lofty::file::FileType::Wav | lofty::file::FileType::Aiff => {
            Some("Lossless".to_owned())
        }
        lofty::file::FileType::Mpeg
        | lofty::file::FileType::Vorbis
        | lofty::file::FileType::Aac => Some("Lossy".to_owned()),
        // M4A 可能是 AAC 也可能是 ALAC：按位深推断（ALAC 有位深，AAC 没有）。
        lofty::file::FileType::Mp4 => {
            if props.bit_depth().is_some() {
                codec = Some("ALAC".to_owned());
                Some("Lossless".to_owned())
            } else {
                codec = Some("AAC".to_owned());
                Some("Lossy".to_owned())
            }
        }
        _ => None,
    };

    let mut info = MetaInfo {
        title: String::new(),
        artist: String::new(),
        album: String::new(),
        album_artist: String::new(),
        duration: props.duration().as_secs_f64(),
        has_art: false,
        bit_rate: props.overall_bitrate().or(props.audio_bitrate()),
        bit_depth: props.bit_depth().map(u32::from),
        sample_rate: props.sample_rate(),
        channels: props.channels().map(u32::from),
        codec,
        quality,
        genres: Vec::new(),
        year: None,
        track: None,
        track_total: None,
        disk: None,
        disk_total: None,
        album_artists: Vec::new(),
        composers: Vec::new(),
        conductor: None,
        lyricist: None,
        publisher: None,
        copyright: None,
        comment: None,
        rating: None,
        lyrics: None,
        art_hue: None,
        // 文件已打开，顺手读（无额外 I/O）。
        replaygain: rg_from_tagged(&tagged),
    };

    if let Some(tag) = tag {
        if let Some(title) = first(tag, ItemKey::TrackTitle)
            && !title.trim().is_empty()
        {
            info.title = title;
        }
        if let Some(artist) = joined(tag, &[ItemKey::TrackArtist, ItemKey::TrackArtists]) {
            info.artist = artist;
        }
        if let Some(album) = first(tag, ItemKey::AlbumTitle)
            && !album.trim().is_empty()
        {
            info.album = album;
        }
        if let Some(album_artist) = joined(tag, &[ItemKey::AlbumArtist, ItemKey::AlbumArtists]) {
            info.album_artist = album_artist;
        }
        info.genres = strings(tag, ItemKey::Genre);
        info.year = number(tag, ItemKey::Year).or_else(|| number(tag, ItemKey::RecordingDate));
        let (track, track_total) = pair(tag, ItemKey::TrackNumber);
        info.track = track;
        info.track_total = track_total.or_else(|| number(tag, ItemKey::TrackTotal));
        let (disk, disk_total) = pair(tag, ItemKey::DiscNumber);
        info.disk = disk;
        info.disk_total = disk_total.or_else(|| number(tag, ItemKey::DiscTotal));
        info.album_artists = strings(tag, ItemKey::AlbumArtist);
        info.composers = strings(tag, ItemKey::Composer);
        info.conductor = first(tag, ItemKey::Conductor);
        info.lyricist = first(tag, ItemKey::Lyricist);
        info.publisher = first(tag, ItemKey::Publisher).or_else(|| first(tag, ItemKey::Label));
        info.copyright = first(tag, ItemKey::CopyrightMessage);
        info.comment = first(tag, ItemKey::Comment);
        info.rating = tag.ratings().next().map(|p| p.rating() as u32);
        info.lyrics = first(tag, ItemKey::Lyrics).or_else(|| first(tag, ItemKey::UnsyncLyrics));
        info.has_art = !tag.pictures().is_empty();
    }

    Ok(info)
}

/// 按需读取内嵌封面（原始字节 + MIME）。不进缓存：用完即释放。
pub fn read_artwork(path: &Path) -> Option<(Vec<u8>, Option<String>)> {
    let tagged = lofty::read_from_path(path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let picture = tag
        .pictures()
        .first()
        .or_else(|| tagged.tags().iter().flat_map(|t| t.pictures()).next())?;
    Some((
        picture.data().to_vec(),
        picture.mime_type().map(|m| m.as_str().to_owned()),
    ))
}

/// 按需读取内嵌歌词原文（同 [`read_artwork`]：用完即释放）。歌词不进
/// 全局元数据缓存 / 数据库——全库常驻歌词文本太占内存，只缓存播放
/// 位置附近的几首（`engine::Meta::lyrics`）。
pub fn read_lyrics(path: &Path) -> Option<String> {
    let tagged = lofty::read_from_path(path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    first(tag, ItemKey::Lyrics).or_else(|| first(tag, ItemKey::UnsyncLyrics))
}
