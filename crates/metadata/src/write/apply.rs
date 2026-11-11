use std::fs;
use std::path::Path;

use super::read::read_tag_type;
use super::{CoverEdit, Edit, TagEdits, TagField};
use super::{
    ensure_writable, temp_path, validate, write_cover_sidecar, write_lrc_sidecar, write_temp,
};
use crate::{pair, parse_gain, parse_peak};
use lofty::file::{TaggedFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::items::Timestamp;
use lofty::tag::items::popularimeter::{Popularimeter, StarRating};
use lofty::tag::{Accessor, ItemKey, Tag, TagItem, TagType};

/// 把 `path` 的标签改成 `edits`：原子写（复制到同目录 `.<文件名>.zeo-tmp`
/// → lofty 写标签 → `rename` 覆盖）；任何一步失败原文件都不动。
/// 数字字段在复制文件之前校验。只写 `.lrc` 侧车（`edits.lrc`）时不动音频。
pub fn write_tags(path: &Path, edits: &TagEdits) -> Result<(), String> {
    validate(edits)?;
    // 写入目标是符号链接所指的真实文件（链接本身留着不动）。
    let target = fs::canonicalize(path).map_err(|e| format!("无法访问 {}: {e}", path.display()))?;
    if edits.has_tag_changes() {
        ensure_writable(&target)?;
        let tmp = temp_path(&target)?;
        let result = write_temp(&target, &tmp, edits);
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        // 标签写失败就停，`.lrc` 也不动（重试会一起再写）。
        result?;
    }
    if let Some(lrc) = &edits.lrc {
        write_lrc_sidecar(&target, lrc)?;
    }
    if let Some(cover_file) = &edits.cover_file {
        write_cover_sidecar(&target, cover_file)?;
    }
    Ok(())
}

/// 选一张标签应用全部改动（与 music-tag-native 的 `tag_mut` 相同）：
/// 写完主标签后顺带同步已有的 ID3v1（它存不下的字段跳过），不留旧值。
pub(crate) fn apply_tag_edits(tagged: &mut TaggedFile, edits: &TagEdits) -> Result<(), String> {
    write_tag(tagged, &|tag| apply_edits(tag, edits))?;
    if tagged.primary_tag_type() != TagType::Id3v1
        && let Some(id3v1) = tagged.tag_mut(TagType::Id3v1)
    {
        let _ = apply_edits(id3v1, edits);
    }
    Ok(())
}

/// `f` 要写到的那张标签（与 music-tag-native 的 `write_tag` 相同）：
/// - 有主标签就写它；
/// - 否则先写读取用的那张（MP3 上的 APE 之类），它存不下某字段时再
///   落到一个新建的主标签——新标签先复制原标签的条目与图片；
/// - ID3v1 不直接接收写入（存不下图片、文本会被截断）；
/// - 纯删除的改动不新建标签：新建也只是换个 `tagType`，已有标签里的
///   字段照常删。
fn write_tag(
    tagged: &mut TaggedFile,
    f: &dyn Fn(&mut Tag) -> Result<(), String>,
) -> Result<(), String> {
    let primary_type = tagged.primary_tag_type();
    if let Some(tag) = tagged.tag_mut(primary_type) {
        return f(tag);
    }

    let existing_type = read_tag_type(tagged);
    let existing_error = match existing_type {
        Some(tag_type) if tag_type != TagType::Id3v1 => match tagged.tag_mut(tag_type).map(f) {
            Some(Ok(())) => return Ok(()),
            Some(Err(error)) => Some(error),
            None => None,
        },
        _ => None,
    };

    let mut candidate = Tag::new(primary_type);
    if let Some(existing) = existing_type.and_then(|tag_type| tagged.tag(tag_type)) {
        seed_tag(&mut candidate, existing);
    }
    let seed = candidate.clone();
    match f(&mut candidate) {
        Ok(()) => {
            if !only_removes(&seed, &candidate) {
                tagged.insert_tag(candidate);
            }
            Ok(())
        }
        Err(error) => Err(existing_error.unwrap_or(error)),
    }
}

/// 把 `src` 的条目与图片复制到另一类型的 `dest`：目标类型没有映射的
/// 条目跳过（`push` 保留多值条目，不像 `insert` 会替换）；歌词两种
/// 键按目标类型互换（本计划不写歌词，但复制旧标签时要带走）。
fn seed_tag(dest: &mut Tag, src: &Tag) {
    for item in src.items() {
        if dest.push(item.clone()) {
            continue;
        }
        let alternate = match item.key() {
            ItemKey::Lyrics => ItemKey::UnsyncLyrics,
            ItemKey::UnsyncLyrics => ItemKey::Lyrics,
            _ => continue,
        };
        if src.get(alternate).is_some() {
            continue;
        }
        dest.push(TagItem::new(alternate, item.value().clone()));
    }
    for picture in src.pictures() {
        dest.push_picture(picture.clone());
    }
}

/// `after` 相对 `before` 只丢了条目 / 图片（没加没改）。
fn only_removes(before: &Tag, after: &Tag) -> bool {
    after
        .items()
        .all(|item| before.items().any(|existing| existing == item))
        && after
            .pictures()
            .iter()
            .all(|picture| before.pictures().contains(picture))
}

/// 写文本条目：`Tag::insert_text` 在标签类型不支持该 `ItemKey` 时返回
/// `false` 并悄悄丢值——必须检查返回值，否则「保存成功」什么都没写。
fn insert_text_checked(
    tag: &mut Tag,
    item_key: ItemKey,
    field: &str,
    text: String,
) -> Result<(), String> {
    if tag.insert_text(item_key, text) {
        return Ok(());
    }
    Err(format!("{field} 不受 {:?} 标签支持", tag.tag_type()))
}

/// 闰年的 2 月 29 天。
fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// 换年份、保留月 / 日 / 时分；换出的日期不存在时（2 月 29 日）钳回去。
fn with_year(existing: Option<Timestamp>, year: u16) -> Timestamp {
    let mut date = existing.unwrap_or_default();
    date.year = year;
    if let (Some(month), Some(day)) = (date.month, date.day) {
        date.day = Some(day.min(days_in_month(year, month)));
    }
    date
}

fn apply_edits(tag: &mut Tag, edits: &TagEdits) -> Result<(), String> {
    for (field, edit) in &edits.fields {
        apply_field(tag, *field, edit)?;
    }
    if let Some(cover) = &edits.cover {
        apply_cover(tag, cover);
    }
    Ok(())
}

fn apply_field(tag: &mut Tag, field: TagField, edit: &Edit) -> Result<(), String> {
    match edit {
        Edit::Keep => Ok(()),
        Edit::Clear => clear_field(tag, field),
        Edit::Set(raw) => {
            let value = raw.trim();
            if value.is_empty() {
                clear_field(tag, field)
            } else {
                set_field(tag, field, value)
            }
        }
    }
}

fn clear_field(tag: &mut Tag, field: TagField) -> Result<(), String> {
    match field {
        // `remove_date` 同时清 `Year` 与 `RecordingDate`。
        TagField::Year => tag.remove_date(),
        // `read_meta` 把单数与复数键一起读：删一个留另一个还显示旧值。
        TagField::Artist => {
            tag.remove_key(ItemKey::TrackArtist);
            tag.remove_key(ItemKey::TrackArtists);
        }
        TagField::AlbumArtist => {
            tag.remove_key(ItemKey::AlbumArtist);
            tag.remove_key(ItemKey::AlbumArtists);
        }
        // TRACKNUMBER="3/12" 这类「n/m」合并写法：清号前先把总数拆
        // 到独立键（不然 /12 跟着号一起没了）。
        TagField::Track => {
            preserve_pair_total(tag, ItemKey::TrackNumber, ItemKey::TrackTotal);
            tag.remove_key(ItemKey::TrackNumber);
        }
        TagField::Disc => {
            preserve_pair_total(tag, ItemKey::DiscNumber, ItemKey::DiscTotal);
            tag.remove_key(ItemKey::DiscNumber);
        }
        // 清总数时把号里的 "/total" 尾巴也清掉（不然读回来总数还在）。
        TagField::TrackTotal => {
            strip_pair_total(tag, ItemKey::TrackNumber);
            tag.remove_key(ItemKey::TrackTotal);
        }
        TagField::DiscTotal => {
            strip_pair_total(tag, ItemKey::DiscNumber);
            tag.remove_key(ItemKey::DiscTotal);
        }
        // 歌词两键（LYRICS / UNSYNCEDLYRICS）：读时先 Lyrics 再回退，
        // 删一个留另一个还显示旧值。
        TagField::Lyrics => {
            tag.remove_key(ItemKey::Lyrics);
            tag.remove_key(ItemKey::UnsyncLyrics);
        }
        // 发行商两键（读时 Publisher 优先、Label 回退）：删一个留另一个
        // 还显示旧值。
        TagField::Publisher => {
            tag.remove_key(ItemKey::Publisher);
            tag.remove_key(ItemKey::Label);
        }
        _ => tag.remove_key(field.key()),
    }
    Ok(())
}

/// `key` 是 "n/total" 形式时把值收成 "n"（`/total` 尾巴去掉）。
fn strip_pair_total(tag: &mut Tag, key: ItemKey) {
    let (number, total) = pair(tag, key);
    if let (Some(number), Some(_)) = (number, total) {
        let _ = tag.insert_text(key, number.to_string());
    }
}

/// `key` 的 "n/total" 合并写法里把 `total` 先拆到 `total_key` 独立键：
/// 接下来改写 `key`（Set / Clear）不会连带丢掉总数。
fn preserve_pair_total(tag: &mut Tag, key: ItemKey, total_key: ItemKey) {
    if tag.get(total_key).is_some() {
        return;
    }
    let (_, total) = pair(tag, key);
    if let Some(total) = total {
        let _ = tag.insert_text(total_key, total.to_string());
    }
}

fn set_field(tag: &mut Tag, field: TagField, value: &str) -> Result<(), String> {
    match field {
        TagField::Year => set_year(tag, value),
        // "n/m" 合并写法：写号前先把 m 拆到独立总数键。
        TagField::Track => {
            preserve_pair_total(tag, ItemKey::TrackNumber, ItemKey::TrackTotal);
            insert_text_checked(tag, ItemKey::TrackNumber, field.label(), value.to_owned())
        }
        TagField::Disc => {
            preserve_pair_total(tag, ItemKey::DiscNumber, ItemKey::DiscTotal);
            insert_text_checked(tag, ItemKey::DiscNumber, field.label(), value.to_owned())
        }
        // 写总数时把号里的 "/total" 尾巴清掉：`read_meta` 优先读合并写法，
        // 留着它新写的独立键会被盖住。
        TagField::TrackTotal => {
            strip_pair_total(tag, ItemKey::TrackNumber);
            insert_text_checked(tag, ItemKey::TrackTotal, field.label(), value.to_owned())
        }
        TagField::DiscTotal => {
            strip_pair_total(tag, ItemKey::DiscNumber);
            insert_text_checked(tag, ItemKey::DiscTotal, field.label(), value.to_owned())
        }
        // 单数键写入后把复数键清掉：不然 `joined` 读回「新值; 旧值」。
        TagField::Artist => {
            insert_text_checked(tag, ItemKey::TrackArtist, field.label(), value.to_owned())?;
            tag.remove_key(ItemKey::TrackArtists);
            Ok(())
        }
        TagField::AlbumArtist => {
            insert_text_checked(tag, ItemKey::AlbumArtist, field.label(), value.to_owned())?;
            tag.remove_key(ItemKey::AlbumArtists);
            Ok(())
        }
        // 歌词：先 `Lyrics`，不支持的标签类型回退 `UnsyncLyrics`；写进
        // 哪边就把另一边清掉，旧值不能和读顺序打架。
        TagField::Lyrics => {
            let key = if tag.insert_text(ItemKey::Lyrics, value.to_owned()) {
                ItemKey::Lyrics
            } else {
                insert_text_checked(tag, ItemKey::UnsyncLyrics, field.label(), value.to_owned())?;
                ItemKey::UnsyncLyrics
            };
            tag.remove_key(if key == ItemKey::Lyrics {
                ItemKey::UnsyncLyrics
            } else {
                ItemKey::Lyrics
            });
            Ok(())
        }
        // 发行商写 Publisher 键并把 Label 别名清掉：读时 Publisher
        // 优先，旧文件里的 Label 不留下次读回还显示旧值。
        TagField::Publisher => {
            insert_text_checked(tag, ItemKey::Publisher, field.label(), value.to_owned())?;
            tag.remove_key(ItemKey::Label);
            Ok(())
        }
        // 评分写成 Popularimeter 文本（`email|星|播放次数`）：已有评分
        // 保留来源 email 与播放计数，没有用 MusicBee 格式从零写。
        TagField::Rating => {
            let star = match value.parse::<u8>() {
                Ok(1) => StarRating::One,
                Ok(2) => StarRating::Two,
                Ok(3) => StarRating::Three,
                Ok(4) => StarRating::Four,
                Ok(5) => StarRating::Five,
                _ => return Err(format!("{}必须是 1 到 5 的整数", field.label())),
            };
            let pop = match tag.ratings().next() {
                Some(p) => Popularimeter::custom(p.email().to_owned(), star, p.play_counter),
                None => Popularimeter::musicbee(star, 0),
            };
            tag.remove_key(ItemKey::Popularimeter);
            insert_text_checked(tag, ItemKey::Popularimeter, field.label(), pop.to_string())
        }
        // ReplayGain：增益统一写成 `-6.50 dB`，峰值写六位小数；读回经
        // `parse_*` 规范化，界面看到的与这里写下的同形态。
        TagField::TrackGain | TagField::AlbumGain => {
            let gain = parse_gain(value)
                .ok_or_else(|| format!("{}必须是 -60 到 60 之间的数（dB）", field.label()))?;
            insert_text_checked(tag, field.key(), field.label(), format!("{gain:.2} dB"))
        }
        TagField::TrackPeak | TagField::AlbumPeak => {
            let peak = parse_peak(value).ok_or_else(|| format!("{}必须是正数", field.label()))?;
            insert_text_checked(tag, field.key(), field.label(), format!("{peak:.6}"))
        }
        _ => insert_text_checked(tag, field.key(), field.label(), value.to_owned()),
    }
}

/// 年份：写 `RecordingDate`（保留原有月日时分），把旧的 `Year` 键清掉；
/// 标签类型不支持 `RecordingDate` 时退回 `Year`。
fn set_year(tag: &mut Tag, value: &str) -> Result<(), String> {
    let year: u16 = value.parse().map_err(|_| "年份必须是整数".to_owned())?;
    let date = with_year(tag.date(), year);
    if tag.insert_text(ItemKey::RecordingDate, date.to_string()) {
        tag.remove_key(ItemKey::Year);
        Ok(())
    } else {
        insert_text_checked(tag, ItemKey::Year, "年份", year.to_string())
    }
}

/// 封面替换 / 删除。ID3v1 存不下图片，跳过（它的同步在 `apply_tag_edits`）。
fn apply_cover(tag: &mut Tag, cover: &CoverEdit) {
    if tag.tag_type() == TagType::Id3v1 {
        return;
    }
    let clear = |tag: &mut Tag| {
        for i in (0..tag.picture_count() as usize).rev() {
            tag.remove_picture(i);
        }
    };
    match cover {
        CoverEdit::Replace { data, mime } => {
            clear(tag);
            tag.push_picture(
                Picture::unchecked(data.clone())
                    .pic_type(PictureType::CoverFront)
                    .mime_type(MimeType::from_str(mime))
                    .build(),
            );
        }
        CoverEdit::Remove => clear(tag),
    }
}
