use std::fs;
use std::path::{Path, PathBuf};

use super::apply_tag_edits;
use super::{Edit, TagEdits, TagField};
use crate::{parse_gain, parse_peak};
use lofty::config::WriteOptions;
use lofty::file::AudioFile;
use lofty::probe::Probe;

/// 数字字段在复制文件之前校验：音轨 / 碟号必须是正整数，年份是
/// `u16` 范围内的整数；非法值直接报错，不碰文件。
pub(crate) fn validate(edits: &TagEdits) -> Result<(), String> {
    for (field, edit) in &edits.fields {
        let Edit::Set(raw) = edit else { continue };
        let value = raw.trim();
        if value.is_empty() {
            continue; // 空白串 = Clear
        }
        match field {
            TagField::Track | TagField::TrackTotal | TagField::Disc | TagField::DiscTotal => {
                match value.parse::<u32>() {
                    Ok(n) if n > 0 => {}
                    _ => return Err(format!("{}必须是正整数", field.label())),
                }
            }
            TagField::Year => {
                if value.parse::<u16>().is_err() {
                    return Err("年份必须是整数".to_owned());
                }
            }
            TagField::Rating => match value.parse::<u8>() {
                Ok(1..=5) => {}
                _ => return Err(format!("{}必须是 1 到 5 的整数", field.label())),
            },
            TagField::TrackGain | TagField::AlbumGain => {
                if parse_gain(value).is_none() {
                    return Err(format!("{}必须是 -60 到 60 之间的数（dB）", field.label()));
                }
            }
            TagField::TrackPeak | TagField::AlbumPeak => {
                if parse_peak(value).is_none() {
                    return Err(format!("{}必须是正数", field.label()));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// rename 只需要目录可写：只读文件在 Unix 上会被悄悄换掉（Windows 上则
/// 到 rename 才失败）；打开一次写柄检查真实权限，不动文件。
pub(crate) fn ensure_writable(path: &Path) -> Result<(), String> {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map(|_| ())
        .map_err(|e| format!("文件不可写 {}: {e}", path.display()))
}

/// 同目录的临时文件：`.<文件名>.zeo-tmp`。
pub(crate) fn temp_path(target: &Path) -> Result<PathBuf, String> {
    let dir = target
        .parent()
        .ok_or_else(|| "文件路径没有父目录".to_owned())?;
    let name = target
        .file_name()
        .ok_or_else(|| "文件路径没有文件名".to_owned())?;
    Ok(dir.join(format!(".{}.zeo-tmp", name.to_string_lossy())))
}

/// 只补 owner 写权限：`Permissions::set_readonly(false)` 在 Unix 上会把
/// 文件变成人人可写。
pub(crate) fn owner_writable(mut permissions: fs::Permissions) -> fs::Permissions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    {
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
    }
    permissions
}

/// 复制 → 在副本上改标签并保存 → 恢复权限 → rename 覆盖。中间任何一步
/// 返回 `Err`，临时文件由调用方删掉。
pub(crate) fn write_temp(target: &Path, tmp: &Path, edits: &TagEdits) -> Result<(), String> {
    fs::copy(target, tmp).map_err(|e| format!("复制到临时文件失败: {e}"))?;
    // fs::copy 带上源的只读属性；先让临时文件可写，写完把权限恢复成原样。
    let permissions = fs::metadata(tmp)
        .map_err(|e| format!("读取临时文件失败: {e}"))?
        .permissions();
    fs::set_permissions(tmp, owner_writable(permissions.clone()))
        .map_err(|e| format!("临时文件授权失败: {e}"))?;
    // 临时文件名改了扩展名（`.zeo-tmp`），lofty 的类型猜测会失败——
    // 用原文件探出的 FileType 强制解析。
    let file_type = Probe::open(target)
        .ok()
        .and_then(|p| p.guess_file_type().ok())
        .and_then(|p| p.file_type())
        .ok_or_else(|| format!("无法识别文件类型 {}", target.display()))?;
    let mut tagged = Probe::open(tmp)
        .map_err(|e| format!("读取标签失败: {e}"))?
        .set_file_type(file_type)
        .read()
        .map_err(|e| format!("读取标签失败: {e}"))?;
    apply_tag_edits(&mut tagged, edits)?;
    tagged
        .save_to_path(tmp, WriteOptions::default())
        .map_err(|e| format!("写入标签失败: {e}"))?;
    fs::set_permissions(tmp, permissions).map_err(|e| format!("恢复权限失败: {e}"))?;
    fs::rename(tmp, target).map_err(|e| format!("替换原文件失败: {e}"))
}
