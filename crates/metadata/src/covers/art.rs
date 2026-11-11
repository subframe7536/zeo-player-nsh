use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use super::{CoverSrc, LARGE, cache_key, cover_source};

/// `path` 的全屏大封面 `data:` URI：原图读到内存、缩到 [`LARGE`] 像素
/// 以内、JPEG 编码、base64 成串，不落盘——只有全屏页显示 / 队尾预载
/// 才实时读元数据；结果进小 LRU（播放位置附近几首），解码副本交给
/// NSH 图片缓存（有自己的 LRU 上限）。`Arc` 共享：快照与界面信号克隆
/// 它只加引用计数（轮询每 100ms 取一次快照，String 克隆就是每秒几 MB）。
pub fn artwork_uri(path: &Path) -> Option<Arc<str>> {
    let key = cache_key(path)?;
    let name = path.to_string_lossy().into_owned();
    {
        let mut cache = art_cache().lock().unwrap();
        if let Some(pos) = cache.iter().position(|(p, k, _)| p == &name && *k == key) {
            let entry = cache.remove(pos);
            let uri = entry.2.clone();
            cache.push(entry);
            return Some(uri);
        }
    }
    let uri: Arc<str> = art_data_uri(path)?.into();
    let mut cache = art_cache().lock().unwrap();
    cache.retain(|(p, _, _)| p != &name);
    cache.push((name, key, uri.clone()));
    while cache.len() > ART_CAP {
        cache.remove(0);
    }
    Some(uri)
}

/// `path` 的全屏大封面已在内存（LRU 命中才给）：引擎换曲时同步调它，
/// 热封面直接进快照不露占位符；生成走 [`artwork_uri`]（工作线程）。
pub fn cached_artwork(path: &Path) -> Option<Arc<str>> {
    let key = cache_key(path)?;
    let name = path.to_string_lossy();
    let mut cache = art_cache().lock().unwrap();
    let pos = cache
        .iter()
        .position(|(p, k, _)| p == name.as_ref() && *k == key)?;
    let entry = cache.remove(pos);
    let uri = entry.2.clone();
    cache.push(entry);
    Some(uri)
}

/// 大封面 URI 的小缓存：只留播放位置附近的几首（当前曲目 + 队尾预载，
/// 同歌词缓存的规模）；每条是一个 ~150KB 的 base64 串，供快照与信号共享。
const ART_CAP: usize = 8;
type ArtCache = Mutex<Vec<(String, u64, Arc<str>)>>;

fn art_cache() -> &'static ArtCache {
    static CACHE: OnceLock<ArtCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

/// 清空大图 LRU（隐藏到托盘时调）：当前曲目的大图 `Arc` 还被
/// `NowPlaying` 信号持着不受影响，只放掉队尾预载；显示后按需重建。
pub fn clear_artwork() {
    art_cache().lock().unwrap().clear();
}

/// 读封面源 → 缩到 [`LARGE`] → JPEG data URI（解不开的格式按原始字节
/// 直接进 URI，交给系统解码，同 `cover_file` 的 Raw 分支）。
fn art_data_uri(path: &Path) -> Option<String> {
    match cover_source(path)? {
        CoverSrc::Img(img) => {
            let img = if img.width() > LARGE || img.height() > LARGE {
                img.resize(LARGE, LARGE, image::imageops::FilterType::Triangle)
            } else {
                img
            };
            let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 85)
                .encode_image(&rgb)
                .ok()?;
            Some(format!(
                "data:image/jpeg;base64,{}",
                base64(&bytes.into_inner())
            ))
        }
        CoverSrc::Raw(bytes) => {
            let mime = crate::read_artwork(path)
                .and_then(|(_, m)| m)
                .unwrap_or_else(|| "image/jpeg".to_owned());
            Some(format!("data:{mime};base64,{}", base64(&bytes)))
        }
    }
}

/// RFC 4648 base64（data URI 用；不引入新依赖，几十行自实现）。
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let (a, b, c) = (
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        );
        let n = (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}
