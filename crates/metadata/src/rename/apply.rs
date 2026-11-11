use super::sidecars_of;
use super::{Planned, Status};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// 执行一条计划：先 `fs::rename` 音频（文件可能被播放器 / 索引器
/// 握着，重试 10 次 × 150ms，同 folders.rs 的后台改名），再把改名
/// 前列出的侧车逐个换到新主干。返回全部已发生的 `(旧, 新)` 移动与
/// 可选错误——音频已移走而侧车失败时，调用方仍能拿到前半的移动
/// 记录去刷新曲库索引。
#[allow(dead_code, reason = "元数据视图接入用（metadata_view_plan 阶段三）")]
pub fn apply(item: &Planned) -> (Vec<(String, String)>, Option<String>) {
    match &item.status {
        Status::Unchanged => return (Vec::new(), None),
        Status::Ok => {}
        Status::Conflict(message) | Status::Invalid(message) => {
            return (Vec::new(), Some(message.clone()));
        }
    }
    let src = Path::new(&item.src);
    let dest = Path::new(&item.dest);
    // 侧车按旧主干找：先列清单，音频改名后旧主干就查不到了。
    let sidecars = sidecars_of(src);
    let mut moves = Vec::new();
    if let Err(e) = rename_retry(src, dest) {
        return (moves, Some(format!("重命名 {} 失败: {e}", src.display())));
    }
    moves.push((item.src.clone(), item.dest.clone()));
    let Some(new_stem) = dest.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
        return (moves, None);
    };
    for sidecar in sidecars {
        let Some(ext) = sidecar
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
        else {
            continue;
        };
        let target = dest.with_file_name(format!("{new_stem}.{ext}"));
        match rename_retry(&sidecar, &target) {
            Ok(()) => moves.push((
                sidecar.to_string_lossy().into_owned(),
                target.to_string_lossy().into_owned(),
            )),
            Err(e) => {
                return (
                    moves,
                    Some(format!("重命名 {} 失败: {e}", sidecar.display())),
                );
            }
        }
    }
    (moves, None)
}

/// `fs::rename` 重试版（Windows 上文件句柄争用常见，同 folders.rs）。
fn rename_retry(src: &Path, dest: &Path) -> Result<(), String> {
    let mut last = String::new();
    for attempt in 0..10 {
        match fs::rename(src, dest) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = e.to_string();
                if attempt < 9 {
                    std::thread::sleep(Duration::from_millis(150));
                }
            }
        }
    }
    Err(last)
}
