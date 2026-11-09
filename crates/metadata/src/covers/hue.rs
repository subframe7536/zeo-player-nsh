use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::MetaCache;

use super::decode_cover;

// --- 主色调 -------------------------------------------------------------------

/// 已完成的（曲目路径, 主色调），工作线程写、界面取走。
fn hue_done() -> &'static Mutex<Vec<(String, Option<(f32, f32)>)>> {
    static DONE: OnceLock<Mutex<Vec<(String, Option<(f32, f32)>)>>> = OnceLock::new();
    DONE.get_or_init(|| Mutex::new(Vec::new()))
}

/// 取走已完成的（曲目路径, 主色调）（「跟随封面」强调色的实时更新）。
pub fn take_dominant() -> Vec<(String, Option<(f32, f32)>)> {
    std::mem::take(&mut *hue_done().lock().unwrap())
}

/// `path` 封面已存的主色调（界面同步读；存在 SQLite 曲目行里，扫描时算好，
/// 没有内存缓存层）。
pub fn art_hue(path: &Path) -> Option<(f32, f32)> {
    MetaCache::global().art_hue(path)
}

/// 库里没有色调时解码封面算一次（covers 工作线程；扫描时每个有封面的
/// 文件都会走到这里，保证 `art_hue` 尽量入库）。优先用已缓存的 `cached`
/// 缩略图（小 JPEG，省一次 lofty + 原图解码），**解不开**才回封面来源
/// 解码（与 [`cover_source`] 同一优先序：本地同名图片 ↔ 内嵌封面）。
/// 缩略图是灰图（`palette_hue` 为 `None`）不回退——它是封面源的缩放，
/// 原图同样给不出色调，回退只会白做一次全尺寸解码（内嵌大图一条几十
/// MB 的临时缓冲）。
pub(crate) fn ensure_hue(path: &Path, cached: &Path) {
    if MetaCache::global().art_hue(path).is_some() {
        return;
    }
    let hue = match image::open(cached) {
        Ok(img) => palette_hue(&img),
        Err(_) => decode_cover(path).and_then(|img| palette_hue(&img)),
    };
    store_hue(path, hue);
}

/// 主色调写进元数据缓存（随元数据行落 SQLite）并通知界面。
pub(crate) fn store_hue(path: &Path, hue: Option<(f32, f32)>) {
    MetaCache::global().set_art_hue(path, hue);
    hue_done()
        .lock()
        .unwrap()
        .push((path.to_string_lossy().into_owned(), hue));
}

/// 封面主色调（色相 0..1 圈、饱和度 0..1）：中位切分取调色板里最饱和的
/// 颜色（color-thief，同 JS color-thief 算法）；全是灰 / 太白太黑时
/// 为 `None`——调用方回主题色。
pub(crate) fn palette_hue(img: &image::DynamicImage) -> Option<(f32, f32)> {
    let rgb = img
        .resize(64, 64, image::imageops::FilterType::Triangle)
        .to_rgb8();
    let palette =
        color_thief::get_palette(rgb.as_raw(), color_thief::ColorFormat::Rgb, 4, 5).ok()?;
    palette
        .iter()
        .filter_map(|c| {
            let (h, s, l) = hsl_of(c.r, c.g, c.b);
            (s >= 0.15 && (0.12..0.85).contains(&l)).then_some((h, s))
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

/// sRGB → HSL（各 0..1，色相为圈）。
pub(crate) fn hsl_of(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255., g as f32 / 255., b as f32 / 255.);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.;
    let d = max - min;
    if d == 0. {
        return (0., 0., l);
    }
    let s = d / (1. - (2. * l - 1.).abs());
    let h = if max == r {
        (g - b) / d + if g < b { 6. } else { 0. }
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    } / 6.;
    (h, s, l)
}
