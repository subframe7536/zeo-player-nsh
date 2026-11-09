use super::{MAX_NAME_BYTES, SEPARATOR_ERROR, Template, sanitize, truncate_bytes};
use crate::write::TagField;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// 单条计划的状态。
#[allow(dead_code, reason = "元数据视图接入用（metadata_view_plan 阶段三）")]
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// 可以改名。
    Ok,
    /// 目标与源相同，无事可做。
    Unchanged,
    /// 目标名撞车（磁盘已有 / 批次内重复 / 侧车目标被占）。
    Conflict(String),
    /// 模板渲染出的名字不可用（如空名）。
    Invalid(String),
}

/// 一条改名计划：`src` → `dest`（同一父目录）。
#[allow(dead_code, reason = "元数据视图接入用（metadata_view_plan 阶段三）")]
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub src: String,
    pub dest: String,
    pub status: Status,
}

/// 一批曲目的改名计划：目标一律落在源文件原父目录里，扩展名原样
/// 保留；状态含批次内冲突与磁盘冲突预检。
#[allow(dead_code, reason = "元数据视图接入用（metadata_view_plan 阶段三）")]
pub fn plan(items: &[(String, HashMap<TagField, String>)], template: &Template) -> Vec<Planned> {
    let mut planned: Vec<Planned> = items
        .iter()
        .map(|(src, fields)| {
            let stem = Path::new(src)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let name = template.render(fields, &stem);
            plan_item(src, &name)
        })
        .collect();
    // 批次内冲突：两条计划的目标名相同（不分大小写）→ 都标冲突；
    // 已标错的不改（保留更具体的报错）。
    for i in 0..planned.len() {
        for j in (i + 1)..planned.len() {
            if planned[i].dest.to_lowercase() == planned[j].dest.to_lowercase() {
                for k in [i, j] {
                    if planned[k].status == Status::Ok {
                        planned[k].status = Status::Conflict("与批次内其他文件重名".to_owned());
                    }
                }
            }
        }
    }
    planned
}

/// 直接输入的新文件名主干 → 单条计划（单选面板「文件名」行用，不经
/// 模板）：先拦目录分隔符，再把顺手带上的「.原扩展名」剥掉（不分
/// 大小写），清洗后交给 [`plan_item`] 的截断 / 落位 / 状态逻辑。
pub fn plan_name(src: &str, new_stem: &str) -> Planned {
    if new_stem.contains('/') || new_stem.contains('\\') {
        return Planned {
            src: src.to_owned(),
            dest: src.to_owned(),
            status: Status::Invalid(SEPARATOR_ERROR.to_owned()),
        };
    }
    let ext = Path::new(src)
        .extension()
        .map(|e| e.to_string_lossy().into_owned());
    let trimmed = new_stem.trim();
    let stem = match &ext {
        Some(e) => {
            // 带上的恰好是原扩展名才剥（`name.flac` → `name`；别的不
            // 算——`x.y` 对新名字是合法字符）。
            let suffix = format!(".{}", e.to_lowercase());
            if trimmed.to_lowercase().ends_with(&suffix) {
                trimmed[..trimmed.len() - suffix.len()]
                    .trim_end()
                    .to_owned()
            } else {
                trimmed.to_owned()
            }
        }
        None => trimmed.to_owned(),
    };
    plan_item(src, &sanitize(&stem))
}

/// 由新主干（已清洗）造一条计划：主干截到 `stem + "." + ext` ≤
/// [`MAX_NAME_BYTES`] 字节的余量，补回原扩展名，落在源文件原父目录，
/// 交给 [`status_of`] 算状态。模板渲染（[`plan`]）与直接输入
/// （[`plan_name`]）共用。
fn plan_item(src: &str, new_stem: &str) -> Planned {
    let src_path = Path::new(src);
    let ext = src_path
        .extension()
        .map(|e| e.to_string_lossy().into_owned());
    // 截断按 stem + "." + ext ≤ 255 字节留给扩展名的余量。
    let limit = MAX_NAME_BYTES.saturating_sub(ext.as_ref().map_or(0, |e| e.len() + 1));
    let name = truncate_bytes(new_stem, limit)
        .trim_end_matches(|c: char| c == '.' || c.is_whitespace())
        .to_owned();
    let file_name = match &ext {
        Some(e) if !e.is_empty() => format!("{name}.{e}"),
        _ => name.clone(),
    };
    let dest = src_path
        .parent()
        .map(|dir| dir.join(&file_name))
        .unwrap_or_else(|| PathBuf::from(&file_name))
        .to_string_lossy()
        .into_owned();
    Planned {
        src: src.to_owned(),
        status: status_of(src, &dest, &name),
        dest,
    }
}

/// 单条计划的状态：空名 → Invalid；原地 → Unchanged；磁盘撞名 →
/// Conflict；大小写差异的原地改名（`same_file` 命中）与全新名 → Ok。
fn status_of(src: &str, dest: &str, name: &str) -> Status {
    if name.is_empty() {
        return Status::Invalid("文件名为空".to_owned());
    }
    if dest == src {
        return Status::Unchanged;
    }
    let src_path = Path::new(src);
    let dest_path = Path::new(dest);
    if same_file(dest_path, src_path) {
        // 仅大小写不同（或别名指向同一文件）：允许，文件系统视为原地改名。
        return Status::Ok;
    }
    if dest_path.exists() {
        return Status::Conflict("目录里已有同名文件".to_owned());
    }
    if sidecar_conflict(src_path, dest_path) {
        return Status::Conflict("同名歌词 / 封面文件已存在".to_owned());
    }
    Status::Ok
}

/// 两个路径指向同一文件：先比路径本身，再比规范路径——大小写不敏感
/// 文件系统上大小写变体解析到同一路径。
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// 改名要联动的侧车目标是否已被占用（占用者不是这次要改的侧车自身，
/// `same_file` 排除大小写变体命中自身的情况）。
fn sidecar_conflict(src: &Path, dest: &Path) -> bool {
    let (Some(dir), Some(new_stem)) = (dest.parent(), dest.file_stem()) else {
        return false;
    };
    let new_stem = new_stem.to_string_lossy();
    for sidecar in sidecars_of(src) {
        let Some(ext) = sidecar
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
        else {
            continue;
        };
        let target = dir.join(format!("{new_stem}.{ext}"));
        if target.exists() && !same_file(&target, &sidecar) {
            return true;
        }
    }
    false
}

/// 音频文件的侧车清单：同主干的 `.lrc` / `.LRC`（与 write.rs 的
/// `write_lrc_sidecar` 同一组候选）加同名图片侧车；大小写变体指向
/// 同一文件时按规范路径去重（与 `covers::sidecar_files` 同款处理）。
pub(crate) fn sidecars_of(audio: &Path) -> Vec<PathBuf> {
    let mut all = Vec::new();
    if let (Some(dir), Some(stem)) = (audio.parent(), audio.file_stem()) {
        let stem = stem.to_string_lossy();
        for ext in ["lrc", "LRC"] {
            let path = dir.join(format!("{stem}.{ext}"));
            if path.is_file() {
                all.push(path);
            }
        }
    }
    all.extend(crate::covers::sidecar_files(audio));
    let mut seen: Vec<PathBuf> = Vec::new();
    all.retain(|path| {
        let canon = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        if seen.contains(&canon) {
            false
        } else {
            seen.push(canon);
            true
        }
    });
    all
}
