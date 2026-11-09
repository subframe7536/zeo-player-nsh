//! 封面缩略图：内嵌封面解码、缩放到 [`THUMB`] 像素、存为 JPEG 缓存文件
//! （数据目录下的 `covers`：macOS `~/Library/Application Support/ZeoPlayer/covers`，
//! Windows `%APPDATA%\ZeoPlayer\covers`；按 路径 + 修改时间 + 大小 + 文件指纹 命名）。
//! 后台线程池处理请求，结果由 UI 轮询取走。

mod art;
mod files;
mod hue;
mod pool;
#[cfg(test)]
mod tests;

pub use art::*;
pub use files::*;
pub use hue::*;
pub use pool::*;

use std::fs;
use std::path::{Path, PathBuf};

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

/// 本地同名封面：同目录 `<曲名>.jpg/jpeg/png/webp`（大小写后缀都试）。
const SIDE_EXTS: [&str; 8] = ["jpg", "jpeg", "png", "webp", "JPG", "JPEG", "PNG", "WEBP"];

/// `path` 的本地同名图片文件（存在与否只按文件名，不解码）。
pub fn find_sidecar(path: &Path) -> Option<PathBuf> {
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

/// 缩略图目录：数据目录下的 `covers`（与 `library.json`、`zeo-player.db`
/// 同处）。`ZEO_PLAYER_CACHE_DIR` 可单独覆盖（单元测试用）。
pub(crate) fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ZEO_PLAYER_CACHE_DIR") {
        return PathBuf::from(dir).join("covers");
    }
    zeo_types::config::data_dir().join("covers")
}

/// 删除全部缩略图（重建曲库）。
pub fn clear_cache() {
    let _ = std::fs::remove_dir_all(cache_dir());
}

/// FNV-1a（跨版本稳定的文件名）。
pub(crate) fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}
