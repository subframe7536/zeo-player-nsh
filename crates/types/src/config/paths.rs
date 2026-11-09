use std::path::PathBuf;

/// 扫描进度。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanProgress {
    pub scanning: bool,
    /// 已处理 / 总数（遍历目录阶段总数为 0）。
    pub done: usize,
    pub total: usize,
    /// 正在读取的文件名。
    pub current: String,
}

/// 应用数据目录（`ZEO_PLAYER_DATA_DIR` 可覆盖；开发构建写到仓库的
/// `.nsh/dev-data`，不碰系统目录；正式包才用系统数据目录）。
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ZEO_PLAYER_DATA_DIR") {
        return PathBuf::from(dir);
    }
    if cfg!(debug_assertions) {
        // CARGO_MANIFEST_DIR 是 crates/types：回两级到仓库根。
        return PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(".nsh")
            .join("dev-data");
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ZeoPlayer")
}
