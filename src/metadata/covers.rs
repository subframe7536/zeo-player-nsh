//! 封面缩略图：内嵌封面解码、缩放到 [`THUMB`] 像素、存为 JPEG 缓存文件
//! （数据目录下的 `covers`：macOS `~/Library/Application Support/Zeo Player/covers`，
//! Windows `%APPDATA%\Zeo Player\covers`；按 路径 + 修改时间 + 大小 + 文件指纹 命名）。
//! 后台线程池处理请求，结果由 UI 轮询取走。

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use crate::metadata::MetaCache;

/// 缩略图边长（像素；列表最大 200 pt，2× 屏）。
pub const THUMB: u32 = 400;
/// 当前曲目的大封面边长（全屏页 326 pt，2× 屏）。
pub const LARGE: u32 = 720;
/// 背景图边长。
pub const BACKDROP: u32 = 64;
const WORKERS: usize = 3;

/// 已完成的（曲目路径, 缩略图路径），工作线程写、界面取走。
type Done = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// 本地同名封面：同目录 `<曲名>.jpg/jpeg/png/webp`（大小写后缀都试）。
const SIDE_EXTS: [&str; 8] = ["jpg", "jpeg", "png", "webp", "JPG", "JPEG", "PNG", "WEBP"];

/// `path` 的本地同名图片文件（存在与否只按文件名，不解码）。
pub(crate) fn find_sidecar(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy();
    let dir = path.parent()?;
    SIDE_EXTS
        .iter()
        .map(|ext| dir.join(format!("{stem}.{ext}")))
        .find(|candidate| candidate.is_file())
}

/// 工作任务：`Thumb` 只生成列表缩略图，`Warm` 生成全部尺寸（队尾
/// 预载——当前曲目的后三首，换歌时大封面 / 背景不再现算）。
#[derive(Clone, PartialEq, Eq, Hash)]
enum Job {
    Thumb(String),
    Warm(String),
}

/// 工作队列：界面可见与队尾预载的请求从队首进（高优先级），扫描
/// 预热从队尾进（低优先级）；`queued` 按（类型, 路径）去重，已在
/// 队列里的重复请求直接丢掉。
struct Pool {
    q: Arc<(Mutex<VecDeque<Job>>, Condvar)>,
    queued: Arc<Mutex<HashSet<Job>>>,
    done: Done,
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        let q = Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        let queued = Arc::new(Mutex::new(HashSet::new()));
        let done = Arc::new(Mutex::new(Vec::new()));
        for i in 0..WORKERS {
            let q = q.clone();
            let queued = queued.clone();
            let done = done.clone();
            std::thread::Builder::new()
                .name(format!("zeo-player-cover-{i}"))
                .spawn(move || {
                    let (m, cv) = &*q;
                    loop {
                        let job = {
                            let mut q = m.lock().unwrap();
                            loop {
                                if let Some(j) = q.pop_front() {
                                    break j;
                                }
                                q = cv.wait(q).unwrap();
                            }
                        };
                        // 先出标记再干活：处理中的重请求会再入队（文件可
                        // 能变了，值得重算），排队中的重请求则丢掉。
                        queued.lock().unwrap().remove(&job);
                        match job {
                            Job::Thumb(path) => {
                                let thumb = thumbnail(Path::new(&path))
                                    .map(|p| p.to_string_lossy().into_owned());
                                done.lock().unwrap().push((path, thumb));
                            }
                            Job::Warm(path) => {
                                let p = Path::new(&path);
                                let thumb = thumbnail(p);
                                large(p);
                                backdrop(p);
                                let thumb = thumb.map(|p| p.to_string_lossy().into_owned());
                                done.lock().unwrap().push((path, thumb));
                            }
                        }
                    }
                })
                .expect("spawn cover worker");
        }
        Pool { q, queued, done }
    })
}

fn enqueue(job: Job, front: bool) {
    let pool = pool();
    if !pool.queued.lock().unwrap().insert(job.clone()) {
        return;
    }
    let (m, cv) = &*pool.q;
    let mut q = m.lock().unwrap();
    if front {
        q.push_front(job);
    } else {
        q.push_back(job);
    }
    cv.notify_one();
}

/// 请求 `path` 的缩略图（结果经 [`take_done`] 取回）。工作线程顺手算
/// 封面主色调写库，色调结果经 [`take_dominant`] 取回。高优先级：界面
/// 可见的封面插到队首，不排在扫描预热后面。
pub fn request(path: &str) {
    enqueue(Job::Thumb(path.to_owned()), true);
}

/// 队尾预载：`path` 的全部封面尺寸（缩略图 + 大封面 + 背景）都提前
/// 生成好；同样插队首，排在扫描预热前面。
pub fn request_warm(path: &str) {
    enqueue(Job::Warm(path.to_owned()), true);
}

/// 低优先级请求（扫描时给整库预热缩略图）：排到队尾。
pub fn request_low(path: &str) {
    enqueue(Job::Thumb(path.to_owned()), false);
}

/// 取走已完成的（曲目路径, 缩略图路径）。
pub fn take_done() -> Vec<(String, Option<String>)> {
    std::mem::take(&mut *pool().done.lock().unwrap())
}

/// 缩略图目录：数据目录下的 `covers`（与 `library.json`、`zeo-player.db`
/// 同处）。`ZEO_PLAYER_CACHE_DIR` 可单独覆盖（单元测试用）。
fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ZEO_PLAYER_CACHE_DIR") {
        return PathBuf::from(dir).join("covers");
    }
    crate::config::data_dir().join("covers")
}

/// 删除全部缩略图（重建曲库）。
pub fn clear_cache() {
    let _ = std::fs::remove_dir_all(cache_dir());
}

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
/// 缩略图（小 JPEG，省一次 lofty + 原图解码），没有才回封面来源解码
/// （与 [`cover_source`] 同一优先序：本地同名图片 ↔ 内嵌封面）。
fn ensure_hue(path: &Path, cached: &Path) {
    if MetaCache::global().art_hue(path).is_some() {
        return;
    }
    let hue = image::open(cached)
        .ok()
        .and_then(|img| palette_hue(&img))
        .or_else(|| decode_cover(path).and_then(|img| palette_hue(&img)));
    store_hue(path, hue);
}

/// 主色调写进元数据缓存（随元数据行落 SQLite）并通知界面。
fn store_hue(path: &Path, hue: Option<(f32, f32)>) {
    MetaCache::global().set_art_hue(path, hue);
    hue_done()
        .lock()
        .unwrap()
        .push((path.to_string_lossy().into_owned(), hue));
}

/// 封面主色调（色相 0..1 圈、饱和度 0..1）：中位切分取调色板里最饱和的
/// 颜色（color-thief，同 JS color-thief 算法）；全是灰 / 太白太黑时
/// 为 `None`——调用方回主题色。
fn palette_hue(img: &image::DynamicImage) -> Option<(f32, f32)> {
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
fn hsl_of(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
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

/// FNV-1a（跨版本稳定的文件名）。
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// `path` 已缓存的大封面与背景（两个文件都在才返回）：只查文件存在
/// 与否、不生成——引擎换曲时同步调它，热缓存的封面直接进快照、不露
/// 占位符；色调补算 / 生成仍走后台（`load_covers` / 工作池）。
pub fn cached(path: &Path) -> Option<(PathBuf, PathBuf)> {
    let key = cache_key(path)?;
    let dir = cache_dir();
    let large = dir.join(format!("{key:016x}-{LARGE}.jpg"));
    let bg = dir.join(format!("{key:016x}-bg{BACKDROP}.jpg"));
    (large.is_file() && bg.is_file()).then_some((large, bg))
}

/// `path` 已缓存的列表缩略图（只查文件，不生成）。
pub fn cached_thumb(path: &Path) -> Option<PathBuf> {
    let key = cache_key(path)?;
    let out = cache_dir().join(format!("{key:016x}-{THUMB}.jpg"));
    out.is_file().then_some(out)
}

/// `path` 的封面缩略图文件（[`THUMB`] 像素）；没有封面为 `None`。
pub fn thumbnail(path: &Path) -> Option<PathBuf> {
    cover_file(path, THUMB)
}

/// 全屏页与系统媒体会话用的大封面（[`LARGE`] 像素）。
pub fn large(path: &Path) -> Option<PathBuf> {
    cover_file(path, LARGE)
}

/// 全屏页的背景：封面缩到 [`BACKDROP`] 像素并在这里高斯模糊好。系统解码
/// 不会放大图片，所以它在内存里也只有几十 KB，由图层拉伸铺满窗口；
/// 不必在窗口尺寸上实时模糊（那要几十 MB）。
pub fn backdrop(path: &Path) -> Option<PathBuf> {
    let key = cache_key(path)?;
    let out = cache_dir().join(format!("{key:016x}-bg{BACKDROP}.jpg"));
    if out.is_file() {
        return Some(out);
    }
    let small = image::open(cover_file(path, THUMB)?).ok()?;
    let img = small
        .resize_to_fill(BACKDROP, BACKDROP, image::imageops::FilterType::Triangle)
        .blur(BACKDROP as f32 / 10.0);
    let tmp = out.with_extension("tmp");
    img.to_rgb8()
        .save_with_format(&tmp, image::ImageFormat::Jpeg)
        .ok()?;
    std::fs::rename(&tmp, &out).ok()?;
    Some(out)
}

fn cache_key(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    // 本地封面参与指纹：同名图片出现 / 改动 / 删除、或「本地优先」开关
    // 变化，都要让已生成的封面失效重算。
    let side_stamp = find_sidecar(path)
        .and_then(|p| std::fs::metadata(&p).ok())
        .map_or(0, |m| crate::metadata::file_stamp(&m));
    let prefer = crate::config::library_store().prefer_local_covers() as u64;
    Some(fnv(format!(
        "{}|{}|{}|{}",
        path.display(),
        crate::metadata::file_stamp(&meta),
        side_stamp,
        prefer
    )
    .as_bytes()))
}

/// 封面来源：解好的图（本地图片 / 内嵌封面解码成功）或内嵌的原始
/// 字节（解码失败也要原样写出交给系统解码，同旧行为）。
enum CoverSrc {
    Img(image::DynamicImage),
    Raw(Vec<u8>),
}

/// `path` 的封面来源，按「本地同名图片优先」开关取序；首选来源失败
/// 回退另一头，都没有为 `None`。`prefer` 参数化方便测试，运行时用
/// [`cover_source`]。
fn cover_source_for(path: &Path, prefer: bool) -> Option<CoverSrc> {
    let mut local = || {
        find_sidecar(path)
            .and_then(|p| image::open(p).ok())
            .map(CoverSrc::Img)
    };
    let mut embedded = || {
        MetaCache::global().get(path).ok()?.has_art.then_some(())?;
        let (bytes, _) = crate::metadata::read_artwork(path)?;
        Some(match image::load_from_memory(&bytes) {
            Ok(img) => CoverSrc::Img(img),
            Err(_) => CoverSrc::Raw(bytes),
        })
    };
    if prefer {
        local().or_else(&mut embedded)
    } else {
        embedded().or_else(&mut local)
    }
}

fn cover_source(path: &Path) -> Option<CoverSrc> {
    cover_source_for(path, crate::config::library_store().prefer_local_covers())
}

/// 解出封面图（取色用；`Raw` 字节再试一次解码）。
fn decode_cover(path: &Path) -> Option<image::DynamicImage> {
    match cover_source(path)? {
        CoverSrc::Img(img) => Some(img),
        CoverSrc::Raw(bytes) => image::load_from_memory(&bytes).ok(),
    }
}

/// `path` 的封面缩到 `side` 像素以内、存为 JPEG（已缓存则直接返回）。
fn cover_file(path: &Path, side: u32) -> Option<PathBuf> {
    let key = cache_key(path)?;
    let dir = cache_dir();
    let out = dir.join(format!("{key:016x}-{side}.jpg"));
    if out.is_file() {
        // 缩略图已在但色调未算（老库 / 灰封面）：补算一次写库。
        ensure_hue(path, &out);
        return Some(out);
    }
    let src = cover_source(path)?;
    std::fs::create_dir_all(&dir).ok()?;
    let tmp = out.with_extension("tmp");
    match src {
        CoverSrc::Img(img) => {
            if MetaCache::global().art_hue(path).is_none() {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 调色板取色：纯色图出主色，灰图不给强调色。
    #[test]
    fn palette_hue_picks_the_saturated_color() {
        let red = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            64,
            64,
            image::Rgb([220, 30, 40]),
        ));
        let (h, s) = palette_hue(&red).expect("红图应有主色调");
        assert!((h - 0.0).abs() < 0.02 || h > 0.98, "红色相约 0: {h}");
        assert!(s > 0.5, "饱和度应高: {s}");
        let gray = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            64,
            64,
            image::Rgb([128, 128, 128]),
        ));
        assert_eq!(palette_hue(&gray), None, "灰图不给强调色");
    }

    /// sRGB → HSL 的基准色与灰度。
    #[test]
    fn hsl_of_primaries_and_grays() {
        let (h, s, l) = hsl_of(255, 0, 0);
        assert!((h - 0.0).abs() < 1e-6 && (s - 1.0).abs() < 1e-6 && (l - 0.5).abs() < 1e-6);
        let (h, _, l) = hsl_of(0, 255, 0);
        assert!((h - 1.0 / 3.0).abs() < 1e-6 && (l - 0.5).abs() < 1e-6);
        let (h, _, l) = hsl_of(0, 0, 255);
        assert!((h - 2.0 / 3.0).abs() < 1e-6 && (l - 0.5).abs() < 1e-6);
        let (_, s, l) = hsl_of(255, 255, 255);
        assert_eq!((s, l), (0.0, 1.0));
        let (_, s, l) = hsl_of(128, 128, 128);
        assert_eq!(s, 0.0);
        assert!((l - 128.0 / 255.0).abs() < 1e-6);
    }

    /// 语料 sine.flac（内嵌红色封面）：读标签 → has_art → 取封面 → 主色调。
    #[test]
    fn corpus_cover_pipeline() {
        let path = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/.nsh/tmp/zeo-player-test-music/sine.flac"
        ));
        if !path.is_file() {
            eprintln!("跳过：语料不存在（sh scripts/make-test-music.sh）");
            return;
        }
        let info = crate::metadata::read_meta(path).expect("read_meta");
        assert!(info.has_art, "sine.flac 应有内嵌封面");
        let (bytes, mime) = crate::metadata::read_artwork(path).expect("read_artwork");
        let img = image::load_from_memory(&bytes).expect("decode cover");
        let hue = palette_hue(&img);
        assert!(hue.is_some(), "红色封面应有主色调（mime={mime:?}）");
    }

    /// 语料 sine.flac 内嵌红色封面、同目录有蓝色 sine.png：本地图片优先
    /// 取蓝（sidecar），内嵌优先取红（read_artwork）。
    #[test]
    fn cover_source_respects_local_first() {
        let path = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/.nsh/tmp/zeo-player-test-music/sine.flac"
        ));
        if !path.is_file() || find_sidecar(path).is_none() {
            eprintln!("跳过：语料不存在（sh scripts/make-test-music.sh）");
            return;
        }
        let hue_of = |src: Option<CoverSrc>| match src? {
            CoverSrc::Img(img) => palette_hue(&img).map(|(h, _)| h),
            CoverSrc::Raw(_) => None,
        };
        let local = hue_of(cover_source_for(path, true)).expect("sidecar 应有主色调");
        assert!((0.55..0.75).contains(&local), "本地优先应取蓝 sidecar：{local:.3}");
        let embedded = hue_of(cover_source_for(path, false)).expect("内嵌封面应有主色调");
        assert!(
            !(0.55..0.75).contains(&embedded),
            "内嵌优先应取红封面而非蓝 sidecar：{embedded:.3}"
        );
    }

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
        unsafe { std::env::set_var("ZEO_PLAYER_CACHE_DIR", "/tmp/zeo-player-test-cache") };
        let _ = std::fs::remove_dir_all("/tmp/zeo-player-test-cache");
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
        eprintln!(
            "{} → {}×{} {size} B，首次 {first:?}，缓存 {:?}",
            file.display(),
            img.width(),
            img.height(),
            again.elapsed()
        );
    }
}
