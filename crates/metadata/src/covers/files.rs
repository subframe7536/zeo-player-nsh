use std::path::{Path, PathBuf};

use crate::MetaCache;

use super::{BACKDROP, LARGE, THUMB, TINY, cache_dir, find_sidecar, fnv};
use super::{ensure_hue, palette_hue, store_hue};

/// `path` 已缓存的列表封面（尺寸, 文件）（只查文件，不生成）：每个
/// 音频文件最多一份，两个名字都可能命中，取大的那份。
pub fn cached_list(path: &Path) -> Option<(u32, PathBuf)> {
    let key = cache_key(path)?;
    let dir = cache_dir();
    [THUMB, TINY]
        .iter()
        .map(|s| (*s, dir.join(format!("{key:016x}-{s}.jpg"))))
        .find(|(_, f)| f.is_file())
}

/// 删掉 `path` 已生成的各尺寸封面（删记录 / 删文件时调）：缓存键
/// 要 `path` 的文件指纹，文件没了就算不出键——删文件前先调。
pub fn evict(path: &Path) {
    let Some(key) = cache_key(path) else {
        return;
    };
    let dir = cache_dir();
    for name in [
        format!("{key:016x}-{TINY}.jpg"),
        format!("{key:016x}-{THUMB}.jpg"),
        format!("{key:016x}-{LARGE}.jpg"),
        format!("{key:016x}-bg{BACKDROP}.jpg"),
    ] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// 删掉比 `keep` 小的列表封面文件（`drain_covers` 把信号换到大文件
/// 之后调）：每个音频文件最多一份缓存封面，升档后旧的小图清掉。
pub fn prune_smaller(path: &Path, keep: u32) {
    let Some(key) = cache_key(path) else {
        return;
    };
    let dir = cache_dir();
    for side in [TINY, THUMB] {
        if side < keep {
            let _ = std::fs::remove_file(dir.join(format!("{key:016x}-{side}.jpg")));
        }
    }
}

/// `path` 的列表封面文件路径（没有则按 [`TINY`] 生成）：引擎给全屏
/// 背景与系统媒体会话拿文件用——背景拉伸铺满窗口，96px 足够。
pub fn list_cover_pub(path: &Path) -> Option<String> {
    list_cover(path, TINY).map(|(_, f)| f.to_string_lossy().into_owned())
}

/// `path` 的列表封面（尺寸, 文件）：已有 ≥ `side` 的直接复用（每个
/// 音频文件最多一份缓存封面），否则按 `side` 生成。生成 `THUMB` 始终
/// 用封面源重算——小图放大质量不够。
pub(crate) fn list_cover(path: &Path, side: u32) -> Option<(u32, PathBuf)> {
    let key = cache_key(path)?;
    let dir = cache_dir();
    for s in [TINY, THUMB] {
        if s >= side {
            let f = dir.join(format!("{key:016x}-{s}.jpg"));
            if f.is_file() {
                // 已在但色调未算（老库 / 灰封面）：补算一次写库。
                ensure_hue(path, &f);
                return Some((s, f));
            }
        }
    }
    Some((side, cover_file(path, side)?))
}

pub(crate) fn cache_key(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    // 本地封面参与指纹：同名图片出现 / 改动 / 删除、或「本地优先」开关
    // 变化，都要让已生成的封面失效重算。
    let side_stamp = find_sidecar(path)
        .and_then(|p| std::fs::metadata(&p).ok())
        .map_or(0, |m| crate::file_stamp(&m));
    let prefer = zeo_types::prefs::prefer_local_covers() as u64;
    Some(fnv(format!(
        "{}|{}|{}|{}",
        path.display(),
        crate::file_stamp(&meta),
        side_stamp,
        prefer
    )
    .as_bytes()))
}

/// 封面来源：解好的图（本地图片 / 内嵌封面解码成功）或内嵌的原始
/// 字节（解码失败也要原样写出交给系统解码，同旧行为）。
pub(crate) enum CoverSrc {
    Img(image::DynamicImage),
    Raw(Vec<u8>),
}

/// `path` 的封面来源，按「本地同名图片优先」开关取序；首选来源失败
/// 回退另一头，都没有为 `None`。`prefer` 参数化方便测试，运行时用
/// [`cover_source`]。
pub(crate) fn cover_source_for(path: &Path, prefer: bool) -> Option<CoverSrc> {
    let mut local = || {
        find_sidecar(path)
            .and_then(|p| image::open(p).ok())
            .map(CoverSrc::Img)
    };
    if prefer {
        local().or_else(|| embedded_source(path))
    } else {
        embedded_source(path).or_else(&mut local)
    }
}

/// 只看内嵌封面（`MetaCache` 记了无图直接 `None`，不解码）。
pub(crate) fn embedded_source(path: &Path) -> Option<CoverSrc> {
    MetaCache::global().get(path).ok()?.has_art.then_some(())?;
    let (bytes, _) = crate::read_artwork(path)?;
    Some(match image::load_from_memory(&bytes) {
        Ok(img) => CoverSrc::Img(img),
        Err(_) => CoverSrc::Raw(bytes),
    })
}

pub(crate) fn cover_source(path: &Path) -> Option<CoverSrc> {
    cover_source_for(path, zeo_types::prefs::prefer_local_covers())
}

/// 解出封面图（取色用；`Raw` 字节再试一次解码）。
pub(crate) fn decode_cover(path: &Path) -> Option<image::DynamicImage> {
    match cover_source(path)? {
        CoverSrc::Img(img) => Some(img),
        CoverSrc::Raw(bytes) => image::load_from_memory(&bytes).ok(),
    }
}

/// `path` 的封面缩到 `side` 像素以内、存为 JPEG（已缓存则直接返回）。
pub(crate) fn cover_file(path: &Path, side: u32) -> Option<PathBuf> {
    cover_file_keyed(path, cache_key(path)?, side, cover_source, true)
}

/// 只看内嵌封面的缩略图：「编辑元数据」弹窗「内嵌标签」目标的预览——
/// 有效封面按「本地优先」开关可能落到同名图片，内嵌目标得预览内嵌
/// 自己。键里混 `emb` 与偏好解析的缓存封面区分开；不写色调——色调
/// 要跟有效封面源走，内嵌图不一定是它。
pub fn embedded_cover_file(path: &Path, side: u32) -> Option<PathBuf> {
    cover_file_keyed(path, embedded_key(path)?, side, embedded_source, false)
}

/// `embedded_cover_file` 的缓存键：只跟音频内容指纹走——侧车出现 /
/// 变化不该让内嵌缩略图重算。
pub(crate) fn embedded_key(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    Some(fnv(format!(
        "{}|{}|emb",
        path.display(),
        crate::file_stamp(&meta)
    )
    .as_bytes()))
}

pub(crate) fn cover_file_keyed(
    path: &Path,
    key: u64,
    side: u32,
    src: impl FnOnce(&Path) -> Option<CoverSrc>,
    record_hue: bool,
) -> Option<PathBuf> {
    let dir = cache_dir();
    let out = dir.join(format!("{key:016x}-{side}.jpg"));
    if out.is_file() {
        if record_hue {
            // 缩略图已在但色调未算（老库 / 灰封面）：补算一次写库。
            ensure_hue(path, &out);
        }
        return Some(out);
    }
    let src = src(path)?;
    std::fs::create_dir_all(&dir).ok()?;
    let tmp = out.with_extension("tmp");
    match src {
        CoverSrc::Img(img) => {
            if record_hue && MetaCache::global().art_hue(path).is_none() {
                store_hue(path, palette_hue(&img));
            }
            let img = if img.width() > side || img.height() > side {
                img.resize(side, side, image::imageops::FilterType::Triangle)
            } else {
                img
            };
            let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
            let file = std::fs::File::create(&tmp).ok()?;
            let mut writer = std::io::BufWriter::new(file);
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 85)
                .encode_image(&rgb)
                .ok()?;
        }
        // 解不开的格式：原样写出，交给系统解码。
        CoverSrc::Raw(bytes) => std::fs::write(&tmp, &bytes).ok()?,
    }
    std::fs::rename(&tmp, &out).ok()?;
    Some(out)
}
