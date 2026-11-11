use std::fs;
use std::path::Path;

use super::{CoverEdit, Edit};

/// 同名 `.lrc` 侧车歌词文件：`Set` 原子写（同目录临时文件 → `rename`；
/// 已有大写 `.LRC` 就更新它，否则新建小写 `.lrc`）；`Clear` 删除文件——
/// 空文件会遮住内嵌歌词，删了它读取才会回退到内嵌。
pub(crate) fn write_lrc_sidecar(audio: &Path, edit: &Edit) -> Result<(), String> {
    let Some(stem) = audio.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
        return Err(format!("音频文件没有文件名 {}", audio.display()));
    };
    let Some(dir) = audio.parent() else {
        return Err(format!("音频文件没有父目录 {}", audio.display()));
    };
    let lrc = ["lrc", "LRC"]
        .into_iter()
        .map(|ext| dir.join(format!("{stem}.{ext}")))
        .find(|p| p.is_file())
        .unwrap_or_else(|| dir.join(format!("{stem}.lrc")));
    match edit {
        Edit::Keep => Ok(()),
        Edit::Clear => match fs::remove_file(&lrc) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("删除 {} 失败: {e}", lrc.display())),
        },
        Edit::Set(text) => {
            let name = lrc
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let tmp = dir.join(format!(".{name}.zeo-tmp"));
            let result = fs::write(&tmp, text.as_bytes())
                .map_err(|e| format!("写临时歌词文件失败: {e}"))
                .and_then(|()| {
                    fs::rename(&tmp, &lrc).map_err(|e| format!("替换歌词文件失败: {e}"))
                });
            if result.is_err() {
                let _ = fs::remove_file(&tmp);
            }
            result
        }
    }
}

/// 同名图片侧车：`Replace` 原子写 `<曲名>.<后缀>`（`mime` → `jpg` /
/// `png` / `webp`；bmp / gif 读取侧车不认，转 PNG 再写），已有同后缀
/// （大小写变体）的就更新它；写完删掉其它后缀的旧侧车——jpg 在读
/// 取序最前，旧 `.jpg` 留着会遮住新写的 `.png`。`Remove` 删掉同名
/// 图片文件（没有就不动）。都不碰音频文件。
pub(crate) fn write_cover_sidecar(audio: &Path, edit: &CoverEdit) -> Result<(), String> {
    let Some(stem) = audio.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
        return Err(format!("音频文件没有文件名 {}", audio.display()));
    };
    let Some(dir) = audio.parent() else {
        return Err(format!("音频文件没有父目录 {}", audio.display()));
    };
    match edit {
        CoverEdit::Remove => {
            for old in crate::covers::sidecar_files(audio) {
                fs::remove_file(&old).map_err(|e| format!("删除 {} 失败: {e}", old.display()))?;
            }
            Ok(())
        }
        CoverEdit::Replace { data, mime } => {
            let (data, ext) = match mime.as_str() {
                "image/jpeg" => (data.clone(), "jpg"),
                "image/png" => (data.clone(), "png"),
                "image/webp" => (data.clone(), "webp"),
                _ => {
                    let img = image::load_from_memory(data)
                        .map_err(|e| format!("解码封面图片失败: {e}"))?;
                    let mut png = std::io::Cursor::new(Vec::new());
                    img.write_to(&mut png, image::ImageFormat::Png)
                        .map_err(|e| format!("封面转码 PNG 失败: {e}"))?;
                    (png.into_inner(), "png")
                }
            };
            // 已有同后缀（大小写变体）的侧车就更新它（`.lrc` 同款策略）。
            let target = crate::covers::sidecar_files(audio)
                .into_iter()
                .find(|p| {
                    p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
                })
                .unwrap_or_else(|| dir.join(format!("{stem}.{ext}")));
            let name = target
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let tmp = dir.join(format!(".{name}.zeo-tmp"));
            let result = fs::write(&tmp, &data)
                .map_err(|e| format!("写临时封面文件失败: {e}"))
                .and_then(|()| {
                    fs::rename(&tmp, &target).map_err(|e| format!("替换封面文件失败: {e}"))
                });
            if result.is_err() {
                let _ = fs::remove_file(&tmp);
            }
            result?;
            for old in crate::covers::sidecar_files(audio) {
                if old != target {
                    fs::remove_file(&old)
                        .map_err(|e| format!("删除旧封面 {} 失败: {e}", old.display()))?;
                }
            }
            Ok(())
        }
    }
}
