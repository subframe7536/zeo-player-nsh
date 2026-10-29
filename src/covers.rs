//! 封面缩略图：内嵌封面解码、缩放到 [`THUMB`] 像素、存为 JPEG 缓存文件
//! （`~/Library/Caches/Zeo/covers`，按 路径 + 修改时间 + 大小 + 文件指纹 命名）。
//! 后台线程池处理请求，结果由 UI 轮询取走。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};

use crate::metadata::MetaCache;

/// 缩略图边长（像素；列表最大 200 pt，2× 屏）。
pub const THUMB: u32 = 400;
const WORKERS: usize = 3;

struct Pool {
    tx: Mutex<Sender<String>>,
    done: Arc<Mutex<Vec<(String, Option<String>)>>>,
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        let (tx, rx) = channel::<String>();
        let rx = Arc::new(Mutex::new(rx));
        let done = Arc::new(Mutex::new(Vec::new()));
        for i in 0..WORKERS {
            let rx: Arc<Mutex<Receiver<String>>> = rx.clone();
            let done = done.clone();
            std::thread::Builder::new()
                .name(format!("zeo-cover-{i}"))
                .spawn(move || loop {
                    let next = rx.lock().unwrap().recv();
                    let Ok(path) = next else { return };
                    let thumb = thumbnail(Path::new(&path)).map(|p| p.to_string_lossy().into_owned());
                    done.lock().unwrap().push((path, thumb));
                })
                .expect("spawn cover worker");
        }
        Pool { tx: Mutex::new(tx), done }
    })
}

/// 请求 `path` 的缩略图（结果经 [`take_done`] 取回）。
pub fn request(path: &str) {
    let _ = pool().tx.lock().unwrap().send(path.to_owned());
}

/// 取走已完成的（曲目路径, 缩略图路径）。
pub fn take_done() -> Vec<(String, Option<String>)> {
    std::mem::take(&mut *pool().done.lock().unwrap())
}

fn cache_dir() -> PathBuf {
    std::env::var_os("ZEO_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("Zeo"))
        .join("covers")
}

/// 删除全部缩略图（重建曲库）。
pub fn clear_cache() {
    let _ = std::fs::remove_dir_all(cache_dir());
}

/// FNV-1a（跨版本稳定的文件名）。
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

/// `path` 的封面缩略图文件（已缓存则直接返回）；没有封面为 `None`。
pub fn thumbnail(path: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    let key = format!("{}|{}", path.display(), crate::metadata::file_stamp(&meta));
    let dir = cache_dir();
    let out = dir.join(format!("{:016x}-{THUMB}.jpg", fnv(key.as_bytes())));
    if out.is_file() {
        return Some(out);
    }
    let info = MetaCache::global().get(path).ok()?;
    let bytes = info.artwork.as_ref()?;
    std::fs::create_dir_all(&dir).ok()?;
    let tmp = out.with_extension("tmp");
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let img = if img.width() > THUMB || img.height() > THUMB {
                img.resize(THUMB, THUMB, image::imageops::FilterType::Triangle)
            } else {
                img
            };
            let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
            let file = std::fs::File::create(&tmp).ok()?;
            let mut writer = std::io::BufWriter::new(file);
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 85).encode_image(&rgb).ok()?;
        }
        // 解不开的格式：原样写出，交给系统解码。
        Err(_) => std::fs::write(&tmp, bytes.as_slice()).ok()?,
    }
    std::fs::rename(&tmp, &out).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实曲库里的一首（没有则跳过）：生成缩略图并命中缓存。
    #[test]
    fn thumbnails_are_small_jpegs() {
        let Some(home) = dirs::home_dir() else { return };
        let Some(file) = walkdir::WalkDir::new(home.join("Music"))
            .max_depth(3)
            .into_iter()
            .flatten()
            .map(|e| e.into_path())
            .find(|p| p.extension().is_some_and(|e| e == "flac" || e == "mp3"))
        else {
            eprintln!("跳过：~/Music 里没有曲目");
            return;
        };
        // SAFETY(测试): 只有本测试读写这个变量。
        unsafe { std::env::set_var("ZEO_CACHE_DIR", "/tmp/zeo-test-cache") };
        let _ = std::fs::remove_dir_all("/tmp/zeo-test-cache");
        let started = std::time::Instant::now();
        let Some(thumb) = thumbnail(&file) else {
            eprintln!("跳过：{} 没有封面", file.display());
            return;
        };
        let first = started.elapsed();
        let img = image::open(&thumb).expect("缩略图可解码");
        assert!(img.width() <= THUMB && img.height() <= THUMB);
        let size = std::fs::metadata(&thumb).unwrap().len();
        let again = std::time::Instant::now();
        assert_eq!(thumbnail(&file), Some(thumb.clone()));
        eprintln!("{} → {}×{} {size} B，首次 {first:?}，缓存 {:?}", file.display(), img.width(), img.height(), again.elapsed());
    }
}
