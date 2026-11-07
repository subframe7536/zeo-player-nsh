//! 封面缩略图：内嵌封面解码、缩放到 [`THUMB`] 像素、存为 JPEG 缓存文件
//! （数据目录下的 `covers`：macOS `~/Library/Application Support/ZeoPlayer/covers`，
//! Windows `%APPDATA%\ZeoPlayer\covers`；按 路径 + 修改时间 + 大小 + 文件指纹 命名）。
//! 后台线程池处理请求，结果由 UI 轮询取走。

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use crate::metadata::MetaCache;

/// 列表封面小尺寸（≤48pt 插槽：曲目行、队列、chips、播放条）：解码后
/// 每条约 37KB，是 400px 的 1/17——滚动的主要是小插槽，NSH 的 128MB
/// 解码缓存因此能多装十几倍封面。
pub const TINY: u32 = 96;
/// 缩略图边长（像素；列表最大 200 pt，2× 屏）。
pub const THUMB: u32 = 400;
/// 当前曲目的大封面边长（全屏页 326 pt，2× 屏）。
pub const LARGE: u32 = 720;
/// 背景图边长。
pub const BACKDROP: u32 = 64;
const WORKERS: usize = 3;

/// 已完成的（曲目路径, 实际尺寸, 缩略图路径），工作线程写、界面取走。
type Done = Arc<Mutex<Vec<(String, u32, Option<String>)>>>;

/// 本地同名封面：同目录 `<曲名>.jpg/jpeg/png/webp`（大小写后缀都试）。
const SIDE_EXTS: [&str; 8] = ["jpg", "jpeg", "png", "webp", "JPG", "JPEG", "PNG", "WEBP"];

/// `path` 的本地同名图片文件（存在与否只按文件名，不解码）。
pub(crate) fn find_sidecar(path: &Path) -> Option<PathBuf> {
    sidecar_files(path).into_iter().next()
}

/// `path` 的全部同名图片文件。`find_sidecar` 只回读取序的第一张；
/// 写侧车时删掉旧后缀的侧车要枚举全部，免得旧图遮住新写的。
/// 大小写不敏感的文件系统（macOS / Windows 默认）上 `sine.png` 和
/// `sine.PNG` 是同一个文件——`is_file` 两种写法都命中，按规范路径
/// 去重，不然枚举出别名会把同一文件当多份删/找。
pub(crate) fn sidecar_files(path: &Path) -> Vec<PathBuf> {
    let Some(stem) = path.file_stem().map(|s| s.to_string_lossy()) else {
        return Vec::new();
    };
    let Some(dir) = path.parent() else {
        return Vec::new();
    };
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for ext in SIDE_EXTS {
        let candidate = dir.join(format!("{stem}.{ext}"));
        if !candidate.is_file() {
            continue;
        }
        let canon = fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
        if seen.contains(&canon) {
            continue;
        }
        seen.push(canon);
        out.push(candidate);
    }
    out
}

/// 工作任务：`Thumb` 只生成列表缩略图（第二元是需要的尺寸，已有
/// 更大的直接复用——每个音频文件最多一份缓存封面），`Warm` 生成
/// 全部尺寸（队尾预载——当前曲目的后三首，换歌时大封面 / 背景不再
/// 现算）。
#[derive(Clone, PartialEq, Eq, Hash)]
enum Job {
    Thumb(String, u32),
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
                            Job::Thumb(path, side) => {
                                let got = list_cover(Path::new(&path), side);
                                done.lock().unwrap().push((
                                    path,
                                    got.as_ref().map(|(s, _)| *s).unwrap_or(side),
                                    got.map(|(_, p)| p.to_string_lossy().into_owned()),
                                ));
                            }
                            Job::Warm(path) => {
                                let p = Path::new(&path);
                                let got = list_cover(p, TINY);
                                artwork_uri(p);
                                done.lock().unwrap().push((
                                    path,
                                    got.as_ref().map(|(s, _)| *s).unwrap_or(TINY),
                                    got.map(|(_, p)| p.to_string_lossy().into_owned()),
                                ));
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

/// 请求 `path` 的列表封面（结果经 [`take_done`] 取回）：`side` 是
/// 需要的边长，已缓存的更大文件直接复用（每个音频文件最多一份缓存
/// 封面，升级后小文件由 [`prune_smaller`] 删掉）。工作线程顺手算封面
/// 主色调写库，色调结果经 [`take_dominant`] 取回。高优先级：界面
/// 可见的封面插到队首，不排在扫描预热后面。
pub fn request(path: &str, side: u32) {
    enqueue(Job::Thumb(path.to_owned(), side), true);
}

/// 队尾预载：`path` 的全部封面尺寸（缩略图 + 大封面 + 背景）都提前
/// 生成好；同样插队首，排在扫描预热前面。
pub fn request_warm(path: &str) {
    enqueue(Job::Warm(path.to_owned()), true);
}

/// 低优先级请求（扫描时给整库预热缩略图）：只出 [`TINY`]——行级插槽
/// 够用且生成快，需要大图的曲目由界面请求时升档重算。排到队尾。
pub fn request_low(path: &str) {
    enqueue(Job::Thumb(path.to_owned(), TINY), false);
}

/// 取走已完成的（曲目路径, 实际尺寸, 缩略图路径）。
pub fn take_done() -> Vec<(String, u32, Option<String>)> {
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
            let mime = crate::metadata::read_artwork(path)
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

/// `path` 的列表封面文件路径（没有则按 [`TINY`] 生成）：引擎给全屏
/// 背景与系统媒体会话拿文件用——背景拉伸铺满窗口，96px 足够。
pub fn list_cover_pub(path: &Path) -> Option<String> {
    list_cover(path, TINY).map(|(_, f)| f.to_string_lossy().into_owned())
}

/// `path` 的列表封面（尺寸, 文件）：已有 ≥ `side` 的直接复用（每个
/// 音频文件最多一份缓存封面），否则按 `side` 生成。生成 `THUMB` 始终
/// 用封面源重算——小图放大质量不够。
fn list_cover(path: &Path, side: u32) -> Option<(u32, PathBuf)> {
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
    if prefer {
        local().or_else(|| embedded_source(path))
    } else {
        embedded_source(path).or_else(&mut local)
    }
}

/// 只看内嵌封面（`MetaCache` 记了无图直接 `None`，不解码）。
fn embedded_source(path: &Path) -> Option<CoverSrc> {
    MetaCache::global().get(path).ok()?.has_art.then_some(())?;
    let (bytes, _) = crate::metadata::read_artwork(path)?;
    Some(match image::load_from_memory(&bytes) {
        Ok(img) => CoverSrc::Img(img),
        Err(_) => CoverSrc::Raw(bytes),
    })
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
    cover_file_keyed(path, cache_key(path)?, side, cover_source, true)
}

/// 只看内嵌封面的缩略图：「编辑元数据」弹窗「内嵌标签」目标的预览——
/// 有效封面按「本地优先」开关可能落到同名图片，内嵌目标得预览内嵌
/// 自己。键里混 `emb` 与偏好解析的缓存封面区分开；不写色调——色调
/// 要跟有效封面源走，内嵌图不一定是它。
pub(crate) fn embedded_cover_file(path: &Path, side: u32) -> Option<PathBuf> {
    cover_file_keyed(path, embedded_key(path)?, side, embedded_source, false)
}

/// `embedded_cover_file` 的缓存键：只跟音频内容指纹走——侧车出现 /
/// 变化不该让内嵌缩略图重算。
fn embedded_key(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    Some(fnv(format!(
        "{}|{}|emb",
        path.display(),
        crate::metadata::file_stamp(&meta)
    )
    .as_bytes()))
}

fn cover_file_keyed(
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
        assert!(
            (0.55..0.75).contains(&local),
            "本地优先应取蓝 sidecar：{local:.3}"
        );
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
        let Some(thumb) = cover_file(&file, THUMB) else {
            eprintln!("跳过：{} 没有封面", file.display());
            return;
        };
        let first = started.elapsed();
        let img = image::open(&thumb).expect("缩略图可解码");
        assert!(img.width() <= THUMB && img.height() <= THUMB);
        let size = std::fs::metadata(&thumb).unwrap().len();
        let again = std::time::Instant::now();
        assert_eq!(cover_file(&file, THUMB), Some(thumb.clone()));
        eprintln!(
            "{} → {}×{} {size} B，首次 {first:?}，缓存 {:?}",
            file.display(),
            img.width(),
            img.height(),
            again.elapsed()
        );
    }
}
