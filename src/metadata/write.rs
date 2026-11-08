//! 标签写回：先把原文件复制到同目录的临时文件 `.<文件名>.zeo-tmp`，在副本上
//! 用 lofty 改标签，成功后 `rename` 原子覆盖原文件；任何一步失败都删掉
//! 临时文件、原文件不动。
//!
//! 标签的选择与字段映射与 music-tag-native 一致：写主标签
//! （`primary_tag_type()`），没有主标签时先写读取用的那张（非 ID3v1，
//! 它存不下才落到新建的主标签——新标签先复制原标签的条目与图片）；
//! ID3v1 只同步、不直接接收写入。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, TaggedFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::items::Timestamp;
use lofty::tag::items::popularimeter::{Popularimeter, StarRating};
use lofty::tag::{Accessor, ItemKey, Tag, TagItem, TagType};

use super::{first, joined, number, pair, parse_gain, parse_peak, strings};

/// 可编辑的标签字段（「编辑元数据」对话框的每个输入对应一个）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TagField {
    Title,
    Artist,
    Album,
    AlbumArtist,
    Genre,
    Year,
    Track,
    TrackTotal,
    Disc,
    DiscTotal,
    Composer,
    Comment,
    Lyrics,
    Conductor,
    Lyricist,
    Publisher,
    Copyright,
    Rating,
    TrackGain,
    TrackPeak,
    AlbumGain,
    AlbumPeak,
}

impl TagField {
    /// 全部字段（[`read_fields`] 返回的键集合）。
    pub const ALL: [TagField; 22] = [
        TagField::Title,
        TagField::Artist,
        TagField::Album,
        TagField::AlbumArtist,
        TagField::Genre,
        TagField::Year,
        TagField::Track,
        TagField::TrackTotal,
        TagField::Disc,
        TagField::DiscTotal,
        TagField::Composer,
        TagField::Comment,
        TagField::Lyrics,
        TagField::Conductor,
        TagField::Lyricist,
        TagField::Publisher,
        TagField::Copyright,
        TagField::Rating,
        TagField::TrackGain,
        TagField::TrackPeak,
        TagField::AlbumGain,
        TagField::AlbumPeak,
    ];

    /// 界面与报错里的中文字段名。
    pub fn label(self) -> &'static str {
        match self {
            TagField::Title => "标题",
            TagField::Artist => "歌手",
            TagField::Album => "专辑",
            TagField::AlbumArtist => "专辑歌手",
            TagField::Genre => "流派",
            TagField::Year => "年份",
            TagField::Track => "音轨号",
            TagField::TrackTotal => "音轨总数",
            TagField::Disc => "碟号",
            TagField::DiscTotal => "碟总数",
            TagField::Composer => "作曲",
            TagField::Comment => "注释",
            TagField::Lyrics => "歌词",
            TagField::Conductor => "指挥",
            TagField::Lyricist => "作词",
            TagField::Publisher => "发行商",
            TagField::Copyright => "版权",
            TagField::Rating => "评分",
            TagField::TrackGain => "音轨增益",
            TagField::TrackPeak => "音轨峰值",
            TagField::AlbumGain => "专辑增益",
            TagField::AlbumPeak => "专辑峰值",
        }
    }

    /// 直接写文本的 `ItemKey`（Artist / AlbumArtist / Year 走特殊路径，
    /// 这里给 `Clear` 删除主键用）。
    fn key(self) -> ItemKey {
        match self {
            TagField::Title => ItemKey::TrackTitle,
            TagField::Artist => ItemKey::TrackArtist,
            TagField::Album => ItemKey::AlbumTitle,
            TagField::AlbumArtist => ItemKey::AlbumArtist,
            TagField::Genre => ItemKey::Genre,
            TagField::Year => ItemKey::Year,
            TagField::Track => ItemKey::TrackNumber,
            TagField::TrackTotal => ItemKey::TrackTotal,
            TagField::Disc => ItemKey::DiscNumber,
            TagField::DiscTotal => ItemKey::DiscTotal,
            TagField::Composer => ItemKey::Composer,
            TagField::Comment => ItemKey::Comment,
            TagField::Lyrics => ItemKey::Lyrics,
            TagField::Conductor => ItemKey::Conductor,
            TagField::Lyricist => ItemKey::Lyricist,
            TagField::Publisher => ItemKey::Publisher,
            TagField::Copyright => ItemKey::CopyrightMessage,
            TagField::Rating => ItemKey::Popularimeter,
            TagField::TrackGain => ItemKey::ReplayGainTrackGain,
            TagField::TrackPeak => ItemKey::ReplayGainTrackPeak,
            TagField::AlbumGain => ItemKey::ReplayGainAlbumGain,
            TagField::AlbumPeak => ItemKey::ReplayGainAlbumPeak,
        }
    }
}

/// 一个字段的改动。
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// 不动（单编辑对话框不产生；批量编辑「保持原值」的占位用它）。
    Keep,
    /// 设成新值；空白串与 `Clear` 等价。
    Set(String),
    /// 删掉该字段。
    Clear,
}

/// 封面的改动（`TagEdits::cover` 为 `None` 即不动）。
#[derive(Debug, Clone, PartialEq)]
pub enum CoverEdit {
    /// 换成新的封面（原始字节 + MIME 类型）；会替换掉文件里的全部内嵌图片——
    /// 界面只把第一张当封面，留着旧的多类型图片换完还显示旧图。
    Replace { data: Vec<u8>, mime: String },
    /// 删掉全部内嵌图片。
    Remove,
}

/// 一次写回的改动集合；批量编辑（plan 012）对每首复用同一结构。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TagEdits {
    pub fields: Vec<(TagField, Edit)>,
    /// 内嵌封面的改动（`None` 不动）。
    pub cover: Option<CoverEdit>,
    /// 同名图片侧车文件的改动（`None` 不动；它不是标签，是旁边的
    /// 图片文件，跟标签分开写——`.lrc` 同款）。`Replace` 的 `mime`
    /// 决定后缀，bmp / gif 转码成 PNG 再写。
    pub cover_file: Option<CoverEdit>,
    /// 同名 `.lrc` 侧车歌词文件的改动（`None` 不动；`.lrc` 不是标签，
    /// 是旁边的文本文件，跟标签分开写）。
    pub lrc: Option<Edit>,
}

impl TagEdits {
    /// 有没有要动音频文件的部分（没有就只写侧车，不复制原文件）。
    fn has_tag_changes(&self) -> bool {
        self.fields.iter().any(|(_, e)| *e != Edit::Keep) || self.cover.is_some()
    }

    /// 没有任何改动（批量编辑逐条跳过空改动用）。
    pub fn is_empty(&self) -> bool {
        !self.has_tag_changes() && self.cover_file.is_none() && self.lrc.is_none()
    }
}

/// 用户选的封面图片 → MIME 类型：先看扩展名，不认得的按文件头猜。
/// 认不出来的返回 `None`（调用方拒绝这张图片）。
pub fn cover_mime(path: &Path) -> Option<String> {
    let by_ext = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("bmp") => Some("image/bmp"),
        Some("webp") => Some("image/webp"),
        _ => None,
    };
    by_ext.map(str::to_owned).or_else(|| {
        use std::io::Read;
        let mut head = [0u8; 12];
        let n = fs::File::open(path).ok()?.read(&mut head).ok()?;
        let head = &head[..n];
        let mime = if head.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if head.starts_with(b"\xff\xd8\xff") {
            "image/jpeg"
        } else if head.starts_with(b"GIF8") {
            "image/gif"
        } else if head.starts_with(b"BM") {
            "image/bmp"
        } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
            "image/webp"
        } else {
            return None;
        };
        Some(mime.to_owned())
    })
}

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

/// 同名 `.lrc` 侧车歌词文件：`Set` 原子写（同目录临时文件 → `rename`；
/// 已有大写 `.LRC` 就更新它，否则新建小写 `.lrc`）；`Clear` 删除文件——
/// 空文件会遮住内嵌歌词，删了它读取才会回退到内嵌。
fn write_lrc_sidecar(audio: &Path, edit: &Edit) -> Result<(), String> {
    let Some(stem) = audio.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
        return Err(format!("音频文件没有文件名 {}", audio.display()));
    };
    let Some(dir) = audio.parent() else {
        return Err(format!("音频文件没有父目录 {}", audio.display()));
    };
    let lrc = ["lrc", "LRC"]
        .into_iter()
        .map(|ext| dir.join(format!("{stem}.{ext}")))
        .find(|p| p.is_file())
        .unwrap_or_else(|| dir.join(format!("{stem}.lrc")));
    match edit {
        Edit::Keep => Ok(()),
        Edit::Clear => match fs::remove_file(&lrc) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("删除 {} 失败: {e}", lrc.display())),
        },
        Edit::Set(text) => {
            let name = lrc
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let tmp = dir.join(format!(".{name}.zeo-tmp"));
            let result = fs::write(&tmp, text.as_bytes())
                .map_err(|e| format!("写临时歌词文件失败: {e}"))
                .and_then(|()| {
                    fs::rename(&tmp, &lrc).map_err(|e| format!("替换歌词文件失败: {e}"))
                });
            if result.is_err() {
                let _ = fs::remove_file(&tmp);
            }
            result
        }
    }
}

/// 同名图片侧车：`Replace` 原子写 `<曲名>.<后缀>`（`mime` → `jpg` /
/// `png` / `webp`；bmp / gif 读取侧车不认，转 PNG 再写），已有同后缀
/// （大小写变体）的就更新它；写完删掉其它后缀的旧侧车——jpg 在读
/// 取序最前，旧 `.jpg` 留着会遮住新写的 `.png`。`Remove` 删掉同名
/// 图片文件（没有就不动）。都不碰音频文件。
fn write_cover_sidecar(audio: &Path, edit: &CoverEdit) -> Result<(), String> {
    let Some(stem) = audio.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
        return Err(format!("音频文件没有文件名 {}", audio.display()));
    };
    let Some(dir) = audio.parent() else {
        return Err(format!("音频文件没有父目录 {}", audio.display()));
    };
    match edit {
        CoverEdit::Remove => {
            for old in crate::metadata::covers::sidecar_files(audio) {
                fs::remove_file(&old).map_err(|e| format!("删除 {} 失败: {e}", old.display()))?;
            }
            Ok(())
        }
        CoverEdit::Replace { data, mime } => {
            let (data, ext) = match mime.as_str() {
                "image/jpeg" => (data.clone(), "jpg"),
                "image/png" => (data.clone(), "png"),
                "image/webp" => (data.clone(), "webp"),
                _ => {
                    let img = image::load_from_memory(data)
                        .map_err(|e| format!("解码封面图片失败: {e}"))?;
                    let mut png = std::io::Cursor::new(Vec::new());
                    img.write_to(&mut png, image::ImageFormat::Png)
                        .map_err(|e| format!("封面转码 PNG 失败: {e}"))?;
                    (png.into_inner(), "png")
                }
            };
            // 已有同后缀（大小写变体）的侧车就更新它（`.lrc` 同款策略）。
            let target = crate::metadata::covers::sidecar_files(audio)
                .into_iter()
                .find(|p| {
                    p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
                })
                .unwrap_or_else(|| dir.join(format!("{stem}.{ext}")));
            let name = target
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let tmp = dir.join(format!(".{name}.zeo-tmp"));
            let result = fs::write(&tmp, &data)
                .map_err(|e| format!("写临时封面文件失败: {e}"))
                .and_then(|()| {
                    fs::rename(&tmp, &target).map_err(|e| format!("替换封面文件失败: {e}"))
                });
            if result.is_err() {
                let _ = fs::remove_file(&tmp);
            }
            result?;
            for old in crate::metadata::covers::sidecar_files(audio) {
                if old != target {
                    fs::remove_file(&old)
                        .map_err(|e| format!("删除旧封面 {} 失败: {e}", old.display()))?;
                }
            }
            Ok(())
        }
    }
}

/// 数字字段在复制文件之前校验：音轨 / 碟号必须是正整数，年份是
/// `u16` 范围内的整数；非法值直接报错，不碰文件。
fn validate(edits: &TagEdits) -> Result<(), String> {
    for (field, edit) in &edits.fields {
        let Edit::Set(raw) = edit else { continue };
        let value = raw.trim();
        if value.is_empty() {
            continue; // 空白串 = Clear
        }
        match field {
            TagField::Track | TagField::TrackTotal | TagField::Disc | TagField::DiscTotal => {
                match value.parse::<u32>() {
                    Ok(n) if n > 0 => {}
                    _ => return Err(format!("{}必须是正整数", field.label())),
                }
            }
            TagField::Year => {
                if value.parse::<u16>().is_err() {
                    return Err("年份必须是整数".to_owned());
                }
            }
            TagField::Rating => match value.parse::<u8>() {
                Ok(1..=5) => {}
                _ => return Err(format!("{}必须是 1 到 5 的整数", field.label())),
            },
            TagField::TrackGain | TagField::AlbumGain => {
                if parse_gain(value).is_none() {
                    return Err(format!("{}必须是 -60 到 60 之间的数（dB）", field.label()));
                }
            }
            TagField::TrackPeak | TagField::AlbumPeak => {
                if parse_peak(value).is_none() {
                    return Err(format!("{}必须是正数", field.label()));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// rename 只需要目录可写：只读文件在 Unix 上会被悄悄换掉（Windows 上则
/// 到 rename 才失败）；打开一次写柄检查真实权限，不动文件。
fn ensure_writable(path: &Path) -> Result<(), String> {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map(|_| ())
        .map_err(|e| format!("文件不可写 {}: {e}", path.display()))
}

/// 同目录的临时文件：`.<文件名>.zeo-tmp`。
fn temp_path(target: &Path) -> Result<PathBuf, String> {
    let dir = target
        .parent()
        .ok_or_else(|| "文件路径没有父目录".to_owned())?;
    let name = target
        .file_name()
        .ok_or_else(|| "文件路径没有文件名".to_owned())?;
    Ok(dir.join(format!(".{}.zeo-tmp", name.to_string_lossy())))
}

/// 只补 owner 写权限：`Permissions::set_readonly(false)` 在 Unix 上会把
/// 文件变成人人可写。
fn owner_writable(mut permissions: fs::Permissions) -> fs::Permissions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    {
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
    }
    permissions
}

/// 复制 → 在副本上改标签并保存 → 恢复权限 → rename 覆盖。中间任何一步
/// 返回 `Err`，临时文件由调用方删掉。
fn write_temp(target: &Path, tmp: &Path, edits: &TagEdits) -> Result<(), String> {
    fs::copy(target, tmp).map_err(|e| format!("复制到临时文件失败: {e}"))?;
    // fs::copy 带上源的只读属性；先让临时文件可写，写完把权限恢复成原样。
    let permissions = fs::metadata(tmp)
        .map_err(|e| format!("读取临时文件失败: {e}"))?
        .permissions();
    fs::set_permissions(tmp, owner_writable(permissions.clone()))
        .map_err(|e| format!("临时文件授权失败: {e}"))?;
    // 临时文件名改了扩展名（`.zeo-tmp`），lofty 的类型猜测会失败——
    // 用原文件探出的 FileType 强制解析。
    let file_type = Probe::open(target)
        .ok()
        .and_then(|p| p.guess_file_type().ok())
        .and_then(|p| p.file_type())
        .ok_or_else(|| format!("无法识别文件类型 {}", target.display()))?;
    let mut tagged = Probe::open(tmp)
        .map_err(|e| format!("读取标签失败: {e}"))?
        .set_file_type(file_type)
        .read()
        .map_err(|e| format!("读取标签失败: {e}"))?;
    apply_tag_edits(&mut tagged, edits)?;
    tagged
        .save_to_path(tmp, WriteOptions::default())
        .map_err(|e| format!("写入标签失败: {e}"))?;
    fs::set_permissions(tmp, permissions).map_err(|e| format!("恢复权限失败: {e}"))?;
    fs::rename(tmp, target).map_err(|e| format!("替换原文件失败: {e}"))
}

/// 选一张标签应用全部改动（与 music-tag-native 的 `tag_mut` 相同）：
/// 写完主标签后顺带同步已有的 ID3v1（它存不下的字段跳过），不留旧值。
fn apply_tag_edits(tagged: &mut TaggedFile, edits: &TagEdits) -> Result<(), String> {
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

/// 读取来源的标签类型：主标签 → 第一个非 ID3v1 → ID3v1（ID3v1 字段短、
/// 存不下图片，排在最后）。
fn read_tag_type(tagged: &TaggedFile) -> Option<TagType> {
    let primary = tagged.primary_tag_type();
    let tags = tagged.tags();
    tags.iter()
        .find(|tag| tag.tag_type() == primary)
        .or_else(|| tags.iter().find(|tag| tag.tag_type() != TagType::Id3v1))
        .or_else(|| tags.first())
        .map(Tag::tag_type)
}

/// 读取用的那张标签（`read_fields` / `seed_tag` 的来源）。
fn read_tag(tagged: &TaggedFile) -> Option<&Tag> {
    read_tag_type(tagged).and_then(|tag_type| tagged.tag(tag_type))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn corpus(name: &str) -> Option<PathBuf> {
        for dir in ["zeo-player-test-music", "zeo-player-test-formats"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(".nsh/tmp")
                .join(dir)
                .join(name);
            if path.is_file() {
                return Some(path);
            }
        }
        None
    }

    /// 复制到独立临时目录（语料是共享文件，绝不能改写）。
    fn scratch(source: &Path) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "zeo-tag-write-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join(source.file_name().unwrap());
        fs::copy(source, &dest).unwrap();
        dest
    }

    /// 目录里的 `.zeo-tmp` 文件（测试断言失败路径不留残局）。
    fn temp_files(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "zeo-tmp"))
            .collect()
    }

    /// `Source::open` 解码前 1 秒的帧数（写完的文件必须仍可读）。
    fn decoded_frames(path: &Path) -> usize {
        let mut source = crate::engine::codec::Source::open(path)
            .unwrap_or_else(|e| panic!("写完的 {} 解码失败: {e}", path.display()));
        let mut frames = 0;
        while frames < source.sample_rate as usize {
            match source.next_block() {
                Ok(Some(block)) => frames += block.len() / source.channels.max(1),
                Ok(None) => break,
                Err(e) => panic!("写完的 {} 解码出错: {e}", path.display()),
            }
        }
        frames
    }

    fn edits(pairs: &[(TagField, &str)]) -> TagEdits {
        TagEdits {
            fields: pairs
                .iter()
                .map(|(f, v)| (*f, Edit::Set((*v).to_owned())))
                .collect(),
            cover: None,
            cover_file: None,
            lrc: None,
        }
    }

    /// 八个容器往返：全字段写入 → `read_meta` / `read_fields` 读回一致；
    /// 写完的文件 `Source::open` 仍能解码。任一容器失败立即报出容器名。
    #[test]
    fn tag_write_roundtrip() {
        for name in [
            "sine.flac",
            "quiet.mp3",
            "sine.wav",
            "aac.m4a",
            "alac.m4a",
            "vorbis.ogg",
            "pcm.aiff",
        ] {
            let Some(source) = corpus(name) else {
                eprintln!("跳过 {name}：没有测试语料");
                continue;
            };
            let path = scratch(&source);
            let set = edits(&[
                (TagField::Title, "新标题"),
                (TagField::Artist, "歌手甲"),
                (TagField::Album, "专辑乙"),
                (TagField::AlbumArtist, "专辑歌手丙"),
                (TagField::Genre, "Rock"),
                (TagField::Year, "2024"),
                (TagField::Track, "3"),
                (TagField::TrackTotal, "12"),
                (TagField::Disc, "1"),
                (TagField::DiscTotal, "2"),
                (TagField::Composer, "作曲丁"),
                (TagField::Comment, "注释戊"),
            ]);
            write_tags(&path, &set).unwrap_or_else(|e| panic!("{name} 写标签失败: {e}"));

            let meta = crate::metadata::read_meta(&path)
                .unwrap_or_else(|e| panic!("{name} 写后读元数据失败: {e}"));
            assert_eq!(meta.title, "新标题", "{name}");
            assert_eq!(meta.artist, "歌手甲", "{name}");
            assert_eq!(meta.album, "专辑乙", "{name}");
            assert_eq!(meta.album_artist, "专辑歌手丙", "{name}");
            assert_eq!(meta.genres, ["Rock"], "{name}");
            assert_eq!(meta.year, Some(2024), "{name}");
            assert_eq!(meta.track, Some(3), "{name}");
            assert_eq!(meta.track_total, Some(12), "{name}");
            assert_eq!(meta.disk, Some(1), "{name}");
            assert_eq!(meta.disk_total, Some(2), "{name}");
            assert_eq!(meta.composers, ["作曲丁"], "{name}");

            let fields =
                read_fields(&path).unwrap_or_else(|e| panic!("{name} 写后读字段失败: {e}"));
            assert_eq!(fields[&TagField::Comment], "注释戊", "{name}");

            assert!(decoded_frames(&path) > 0, "{name} 写完解不出声音");
            assert!(
                temp_files(path.parent().unwrap()).is_empty(),
                "{name} 留下 .zeo-tmp"
            );
        }
    }

    /// Set 写进字段后 Clear 删掉（`Set("")` 与 `Clear` 等价）。
    #[test]
    fn tag_write_clear_removes_key() {
        let Some(source) = corpus("sine.flac") else {
            return eprintln!("跳过：没有测试语料");
        };
        let path = scratch(&source);
        let set = edits(&[(TagField::Album, "临时专辑")]);
        write_tags(&path, &set).unwrap();
        assert_eq!(crate::metadata::read_meta(&path).unwrap().album, "临时专辑");
        let clear = TagEdits {
            fields: vec![(TagField::Album, Edit::Clear)],
            cover: None,
            cover_file: None,
            lrc: None,
        };
        write_tags(&path, &clear).unwrap();
        assert_eq!(crate::metadata::read_meta(&path).unwrap().album, "");
        let blank = edits(&[(TagField::Title, "")]);
        write_tags(&path, &blank).unwrap();
        // 空串 = Clear：标题不再回退到文件名，保持空串。
        assert_eq!(crate::metadata::read_meta(&path).unwrap().title, "");
    }

    /// TRACKNUMBER="3/12" 合并写法：动号 / 动总数 / 清号 / 清总数都
    /// 不能把另一边弄丢或留下读得回来的旧值。
    #[test]
    fn tag_write_pair_format_total_survives() {
        let Some(source) = corpus("sine.flac") else {
            return eprintln!("跳过：没有测试语料");
        };
        // 造一个 TRACKNUMBER="3/12" 的文件（合并写法）。
        let pair_file = |path: &Path| {
            let mut tagged = Probe::open(path)
                .unwrap()
                .guess_file_type()
                .unwrap()
                .read()
                .unwrap();
            let tag = tagged.primary_tag_mut().unwrap();
            tag.insert_text(ItemKey::TrackNumber, "3/12".to_owned());
            tag.remove_key(ItemKey::TrackTotal);
            tagged.save_to_path(path, WriteOptions::default()).unwrap();
        };
        let meta = |path: &Path| crate::metadata::read_meta(path).unwrap();

        let path = scratch(&source);
        pair_file(&path);
        let clear_track = TagEdits {
            fields: vec![(TagField::Track, Edit::Clear)],
            cover: None,
            cover_file: None,
            lrc: None,
        };
        write_tags(&path, &clear_track).unwrap();
        // 清了号，总数还在（拆到独立键）。
        assert_eq!(
            (meta(&path).track, meta(&path).track_total),
            (None, Some(12))
        );

        let path = scratch(&source);
        pair_file(&path);
        write_tags(&path, &edits(&[(TagField::Track, "5")])).unwrap();
        assert_eq!(
            (meta(&path).track, meta(&path).track_total),
            (Some(5), Some(12))
        );

        let path = scratch(&source);
        pair_file(&path);
        write_tags(&path, &edits(&[(TagField::TrackTotal, "15")])).unwrap();
        assert_eq!(
            (meta(&path).track, meta(&path).track_total),
            (Some(3), Some(15))
        );

        let path = scratch(&source);
        pair_file(&path);
        let clear_total = TagEdits {
            fields: vec![(TagField::TrackTotal, Edit::Clear)],
            cover: None,
            cover_file: None,
            lrc: None,
        };
        write_tags(&path, &clear_total).unwrap();
        assert_eq!(
            (meta(&path).track, meta(&path).track_total),
            (Some(3), None)
        );
    }

    /// 歌词：写入 → `read_lyrics` 读回；清空后 `read_lyrics` 为 `None`。
    /// LYRICS / UNSYNCEDLYRICS 两个键都不许留旧值。
    #[test]
    fn tag_write_lyrics_roundtrip() {
        for name in ["sine.flac", "quiet.mp3", "vorbis.ogg", "aac.m4a"] {
            let Some(source) = corpus(name) else {
                eprintln!("跳过 {name}：没有测试语料");
                continue;
            };
            let path = scratch(&source);
            let text = "[00:01.00]第一行\n[00:02.00]第二行";
            let set = TagEdits {
                fields: vec![(TagField::Lyrics, Edit::Set(text.to_owned()))],
                cover: None,
                cover_file: None,
                lrc: None,
            };
            match write_tags(&path, &set) {
                Ok(()) => assert_eq!(
                    crate::metadata::read_lyrics(&path).as_deref(),
                    Some(text),
                    "{name} 歌词应原样读回"
                ),
                Err(e) => {
                    eprintln!("{name} 不支持写歌词键: {e}");
                    continue;
                }
            }
            let clear = TagEdits {
                fields: vec![(TagField::Lyrics, Edit::Clear)],
                cover: None,
                cover_file: None,
                lrc: None,
            };
            write_tags(&path, &clear).unwrap();
            assert_eq!(crate::metadata::read_lyrics(&path), None, "{name}");
            assert!(decoded_frames(&path) > 0, "{name} 写完解不出声音");
        }
    }

    /// `.lrc` 侧车：Set 新建 / 更新同名歌词文件（不碰音频），Clear 删除
    /// 文件；失败不留 `.zeo-tmp`。大写 `.LRC` 已有时更新的是它。
    #[test]
    fn tag_write_lrc_sidecar() {
        let Some(source) = corpus("sine.flac") else {
            return eprintln!("跳过：没有测试语料");
        };
        let path = scratch(&source);
        let lrc_path = path.with_extension("lrc");
        let before = fs::read(&path).unwrap();
        let set = TagEdits {
            fields: Vec::new(),
            cover: None,
            cover_file: None,
            lrc: Some(Edit::Set("[00:01.00]侧车歌词".to_owned())),
        };
        write_tags(&path, &set).unwrap();
        assert_eq!(fs::read_to_string(&lrc_path).unwrap(), "[00:01.00]侧车歌词");
        assert_eq!(fs::read(&path).unwrap(), before, "侧车编辑不能动音频文件");
        assert!(decoded_frames(&path) > 0);

        let clear = TagEdits {
            fields: Vec::new(),
            cover: None,
            cover_file: None,
            lrc: Some(Edit::Clear),
        };
        write_tags(&path, &clear).unwrap();
        assert!(!lrc_path.exists(), "Clear 应删掉 .lrc");
        assert!(
            temp_files(path.parent().unwrap()).is_empty(),
            "失败不能留 .zeo-tmp"
        );
    }

    /// 非法数字：直接报错，文件字节不变，目录里没有 `.zeo-tmp`。
    #[test]
    fn tag_write_rejects_bad_numbers_without_touching_file() {
        let Some(source) = corpus("sine.flac") else {
            return eprintln!("跳过：没有测试语料");
        };
        let path = scratch(&source);
        let before = fs::read(&path).unwrap();
        for bad in ["abc", "0", "-1", "3.5"] {
            let err = write_tags(&path, &edits(&[(TagField::Track, bad)]))
                .expect_err(&format!("{bad} 应当被拒绝"));
            assert!(err.contains("正整数"), "{bad}: {err}");
        }
        assert_eq!(fs::read(&path).unwrap(), before, "原文件不能动");
        assert!(temp_files(path.parent().unwrap()).is_empty());
    }

    /// 封面替换（字节与 MIME 原样写入）与移除。
    #[test]
    fn tag_write_cover_replace_and_remove() {
        let Some(source) = corpus("sine.flac") else {
            return eprintln!("跳过：没有测试语料");
        };
        let path = scratch(&source);
        // 8×8 红 PNG。
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            8,
            8,
            image::Rgba([220, 40, 40, 255]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
        let png = png.into_inner();

        let replace = TagEdits {
            fields: Vec::new(),
            cover: Some(CoverEdit::Replace {
                data: png.clone(),
                mime: "image/png".to_owned(),
            }),
            cover_file: None,
            lrc: None,
        };
        write_tags(&path, &replace).unwrap();
        assert!(crate::metadata::read_meta(&path).unwrap().has_art);
        let (data, mime) = crate::metadata::read_artwork(&path).unwrap();
        assert_eq!(data, png, "封面字节应原样存进去");
        assert_eq!(mime.as_deref(), Some("image/png"));

        let remove = TagEdits {
            fields: Vec::new(),
            cover: Some(CoverEdit::Remove),
            cover_file: None,
            lrc: None,
        };
        write_tags(&path, &remove).unwrap();
        assert!(!crate::metadata::read_meta(&path).unwrap().has_art);
        assert!(decoded_frames(&path) > 0);
    }

    /// 同名图片侧车：Replace 新建 / 更新 `<曲名>.<后缀>`（mime 定后缀，
    /// bmp 转 PNG），写完删掉其它后缀的旧侧车；Remove 删除文件。音频
    /// 本身不动。
    #[test]
    fn tag_write_cover_sidecar() {
        let Some(source) = corpus("sine.flac") else {
            return eprintln!("跳过：没有测试语料");
        };
        let png8 = |px: [u8; 4]| {
            let mut png = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(8, 8, image::Rgba(px)))
                .write_to(&mut png, image::ImageFormat::Png)
                .unwrap();
            png.into_inner()
        };
        let replace = |data, mime: &str| TagEdits {
            fields: Vec::new(),
            cover: None,
            cover_file: Some(CoverEdit::Replace {
                data,
                mime: mime.to_owned(),
            }),
            lrc: None,
        };

        // 新建 .png，音频字节不动。
        let path = scratch(&source);
        let png_path = path.with_extension("png");
        let before = fs::read(&path).unwrap();
        let png = png8([220, 40, 40, 255]);
        write_tags(&path, &replace(png.clone(), "image/png")).unwrap();
        assert_eq!(fs::read(&png_path).unwrap(), png);
        assert_eq!(fs::read(&path).unwrap(), before, "侧车编辑不能动音频文件");

        // 再写 .jpg：旧 .png 删掉（jpg 读取序在前，留着会遮住新图）。
        let jpg_path = path.with_extension("jpg");
        write_tags(&path, &replace(b"fake-jpeg".to_vec(), "image/jpeg")).unwrap();
        assert_eq!(fs::read(&jpg_path).unwrap(), b"fake-jpeg");
        assert!(!png_path.exists(), "旧后缀的侧车应删掉");

        // bmp 转 PNG 写 .png，.jpg 同样被清掉。
        let mut bmp = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            8,
            8,
            image::Rgba([40, 40, 220, 255]),
        ))
        .write_to(&mut bmp, image::ImageFormat::Bmp)
        .unwrap();
        write_tags(&path, &replace(bmp.into_inner(), "image/bmp")).unwrap();
        assert!(png_path.exists(), "bmp 应转码写 .png");
        assert!(!jpg_path.exists());
        assert_eq!(
            image::open(&png_path).unwrap().to_rgba8().get_pixel(0, 0).0,
            [40, 40, 220, 255],
            "转码应保留原图内容"
        );

        // Remove 删掉同名图片文件。
        let remove = TagEdits {
            fields: Vec::new(),
            cover: None,
            cover_file: Some(CoverEdit::Remove),
            lrc: None,
        };
        write_tags(&path, &remove).unwrap();
        assert!(!png_path.exists(), "Remove 应删掉同名图片");
        assert!(
            temp_files(path.parent().unwrap()).is_empty(),
            "失败不能留 .zeo-tmp"
        );
    }

    /// 失败路径：只读文件 → `ensure_writable` 在复制前就报错，原文件
    /// 不变、目录里没有 `.zeo-tmp`。
    #[cfg(unix)]
    #[test]
    fn tag_write_leaves_no_temp_on_failure() {
        use std::os::unix::fs::PermissionsExt;
        let Some(source) = corpus("sine.flac") else {
            return eprintln!("跳过：没有测试语料");
        };
        let path = scratch(&source);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let before = fs::read(&path).unwrap();
        let err = write_tags(&path, &edits(&[(TagField::Title, "新标题")]))
            .expect_err("只读文件应当写不进去");
        assert!(err.contains("不可写"), "{err}");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before, "原文件不能动");
        assert!(
            temp_files(path.parent().unwrap()).is_empty(),
            "失败不能留 .zeo-tmp"
        );
    }

    /// raw.aac（ADTS 裸流）：没有标签容器，写标签应明确失败而不是悄悄
    /// 丢掉；失败也不能动文件。
    #[test]
    fn tag_write_reports_unsupported_container() {
        let Some(source) = corpus("raw.aac") else {
            return eprintln!("跳过：没有测试语料");
        };
        let path = scratch(&source);
        let before = fs::read(&path).unwrap();
        match write_tags(&path, &edits(&[(TagField::Title, "新标题")])) {
            Ok(()) => {
                // lofty 若能给 ADTS 套上 ID3v2，文件必须仍可解码。
                assert!(decoded_frames(&path) > 0, "写完的 ADTS 解不出声音");
            }
            Err(e) => {
                eprintln!("raw.aac 写标签失败（预期）: {e}");
                assert_eq!(fs::read(&path).unwrap(), before, "失败不能动原文件");
            }
        }
        assert!(temp_files(path.parent().unwrap()).is_empty());
    }

    /// 元数据视图新增字段：指挥 / 作词 / 发行商 / 版权 / 评分 /
    /// ReplayGain 四项写入 → `read_row` 读回（评分、增益、峰值读回
    /// 规范形态）；逐个 Clear 后全空。
    #[test]
    fn tag_write_extended_fields_roundtrip() {
        let extended = [
            TagField::Conductor,
            TagField::Lyricist,
            TagField::Publisher,
            TagField::Copyright,
            TagField::Rating,
            TagField::TrackGain,
            TagField::TrackPeak,
            TagField::AlbumGain,
            TagField::AlbumPeak,
        ];
        let mut covered = 0;
        for (name, tag_type) in [("sine.flac", "Vorbis Comments"), ("quiet.mp3", "ID3v2")] {
            let Some(source) = corpus(name) else {
                eprintln!("跳过 {name}：没有测试语料");
                continue;
            };
            covered += 1;
            let path = scratch(&source);
            let set = edits(&[
                (TagField::Conductor, "指挥甲"),
                (TagField::Lyricist, "作词乙"),
                (TagField::Publisher, "发行商丙"),
                (TagField::Copyright, "版权丁"),
                (TagField::Rating, "4"),
                (TagField::TrackGain, "-6.5"),
                (TagField::TrackPeak, "0.98"),
                (TagField::AlbumGain, "-3.25"),
                (TagField::AlbumPeak, "1.05"),
            ]);
            write_tags(&path, &set).unwrap_or_else(|e| panic!("{name} 写标签失败: {e}"));

            let (fields, read_type) =
                read_row(&path).unwrap_or_else(|e| panic!("{name} 写后读字段失败: {e}"));
            assert_eq!(read_type, Some(tag_type), "{name} 标签类型名");
            assert_eq!(fields[&TagField::Conductor], "指挥甲", "{name}");
            assert_eq!(fields[&TagField::Lyricist], "作词乙", "{name}");
            assert_eq!(fields[&TagField::Publisher], "发行商丙", "{name}");
            assert_eq!(fields[&TagField::Copyright], "版权丁", "{name}");
            assert_eq!(fields[&TagField::Rating], "4", "{name}");
            assert_eq!(fields[&TagField::TrackGain], "-6.50", "{name}");
            assert_eq!(fields[&TagField::TrackPeak], "0.980000", "{name}");
            assert_eq!(fields[&TagField::AlbumGain], "-3.25", "{name}");
            assert_eq!(fields[&TagField::AlbumPeak], "1.050000", "{name}");

            let clear = TagEdits {
                fields: extended.iter().map(|f| (*f, Edit::Clear)).collect(),
                cover: None,
                cover_file: None,
                lrc: None,
            };
            write_tags(&path, &clear).unwrap_or_else(|e| panic!("{name} 清字段失败: {e}"));
            let fields = read_fields(&path).unwrap();
            for field in extended {
                assert!(
                    fields[&field].is_empty(),
                    "{name} {:?} 清空后应读不回值",
                    field
                );
            }
            assert!(decoded_frames(&path) > 0, "{name} 写完解不出声音");
            assert!(
                temp_files(path.parent().unwrap()).is_empty(),
                "{name} 留下 .zeo-tmp"
            );
        }
        assert!(covered > 0, "测试语料缺失，什么都没跑到");
    }

    /// 评分与 ReplayGain 的校验：非法值在复制文件前就报错，文件不动。
    #[test]
    fn tag_write_rejects_bad_rating_and_gain() {
        let Some(source) = corpus("sine.flac") else {
            return eprintln!("跳过：没有测试语料");
        };
        let path = scratch(&source);
        let before = fs::read(&path).unwrap();
        for bad in ["0", "6", "x", "3.5"] {
            let err = write_tags(&path, &edits(&[(TagField::Rating, bad)]))
                .expect_err(&format!("评分 {bad} 应当被拒绝"));
            assert!(err.contains("1 到 5"), "{bad}: {err}");
        }
        for bad in ["abc", "100", "-61"] {
            for field in [TagField::TrackGain, TagField::AlbumGain] {
                let err = write_tags(&path, &edits(&[(field, bad)]))
                    .expect_err(&format!("增益 {bad} 应当被拒绝"));
                assert!(err.contains("dB"), "{bad}: {err}");
            }
        }
        for bad in ["abc", "0", "-1"] {
            for field in [TagField::TrackPeak, TagField::AlbumPeak] {
                let err = write_tags(&path, &edits(&[(field, bad)]))
                    .expect_err(&format!("峰值 {bad} 应当被拒绝"));
                assert!(err.contains("正数"), "{bad}: {err}");
            }
        }
        assert_eq!(fs::read(&path).unwrap(), before, "原文件不能动");
        assert!(temp_files(path.parent().unwrap()).is_empty());
    }
}
