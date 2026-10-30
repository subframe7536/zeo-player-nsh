//! 自定义字体：用 font-enumeration 列出系统字体家族，用 fontdb 读导入文件的家族名；导入 .ttf / .otf / .ttc 文件（复制
//! 到 `~/Library/Application Support/Zeo/fonts`，只对本进程注册，不装进系统）。

use std::path::{Path, PathBuf};

/// 导入的字体放在这里（启动时全部注册）。
pub fn fonts_dir() -> PathBuf {
    crate::library::data_dir().join("fonts")
}

/// 可导入的扩展名。
pub const FONT_EXTENSIONS: &[&str] = &["ttf", "otf", "ttc"];

/// 字体文件里的家族名（fontdb 读 name 表，英文名优先；字体集合取全部）。
pub fn family_names(data: &[u8]) -> Vec<String> {
    let mut db = fontdb::Database::new();
    db.load_font_data(data.to_vec());
    families_of(&db)
}

fn families_of(db: &fontdb::Database) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for face in db.faces() {
        if let Some((name, _)) = face.families.first()
            && !name.trim().is_empty()
            && !out.contains(name)
        {
            out.push(name.clone());
        }
    }
    out
}

/// 导入一个字体文件：读出家族名、复制到 [`fonts_dir`]、注册。返回家族名。
pub fn import(path: &Path) -> Result<Vec<String>, String> {
    let data = std::fs::read(path).map_err(|e| format!("无法读取 {}：{e}", path.display()))?;
    let families = family_names(&data);
    if families.is_empty() {
        return Err(format!("{} 不是可用的字体文件", path.display()));
    }
    let dir = fonts_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = path.file_name().ok_or("没有文件名")?;
    let target = dir.join(name);
    if target != path {
        std::fs::write(&target, &data).map_err(|e| format!("无法保存字体：{e}"))?;
    }
    register(&target)?;
    Ok(families)
}

/// 已导入的字体文件。
pub fn imported_files() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(fonts_dir()) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| FONT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        })
        .collect();
    files.sort();
    files
}

/// 已导入字体的家族名。
pub fn imported_families() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for file in imported_files() {
        if let Ok(data) = std::fs::read(&file) {
            for name in family_names(&data) {
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
    }
    out
}

/// 启动时注册全部已导入的字体。
pub fn register_imported() {
    for file in imported_files() {
        if let Err(error) = register(&file) {
            eprintln!("zeo: {error}");
        }
    }
}

/// 删除导入的字体家族所在的文件（重启后不再可用）。
pub fn remove_imported(family: &str) {
    for file in imported_files() {
        if let Ok(data) = std::fs::read(&file)
            && family_names(&data).iter().any(|f| f == family)
        {
            unregister(&file);
            let _ = std::fs::remove_file(&file);
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::Path;

    use objc2_core_foundation::CFURL;
    use objc2_core_text::{CTFontManagerRegisterFontsForURL, CTFontManagerScope, CTFontManagerUnregisterFontsForURL};

    pub fn register(path: &Path) -> Result<(), String> {
        let url = CFURL::from_file_path(path).ok_or_else(|| format!("无效路径 {}", path.display()))?;
        // SAFETY: error 传空指针（允许）；只在本进程内注册。
        let ok = unsafe { CTFontManagerRegisterFontsForURL(&url, CTFontManagerScope::Process, std::ptr::null_mut()) };
        // 已注册过同一文件也会返回 false：此时字体照样可用。
        if !ok {
            log_once(path);
        }
        Ok(())
    }

    pub fn unregister(path: &Path) {
        if let Some(url) = CFURL::from_file_path(path) {
            // SAFETY: 同上。
            unsafe { CTFontManagerUnregisterFontsForURL(&url, CTFontManagerScope::Process, std::ptr::null_mut()) };
        }
    }

    fn log_once(path: &Path) {
        eprintln!("zeo: 字体已注册或无法注册：{}", path.display());
    }
}

/// 其它平台（含 Windows）：NSH 0.3.0 没有运行时注册字体的公开接口，导入
/// 暂不可用（见 README「待上游」）；已安装字体照常可选。
#[cfg(not(target_os = "macos"))]
mod platform {
    use std::path::Path;

    pub fn register(_path: &Path) -> Result<(), String> {
        Err("此平台暂不支持导入字体，可以选择系统里已安装的字体".into())
    }
    pub fn unregister(_path: &Path) {}
}

/// 系统字体家族：font-enumeration 向系统字体服务查询（macOS CoreText、
/// Windows DirectWrite、Linux fontconfig），得到的家族名就是渲染时能解析的
/// 名字。排序、去重，不含以 `.` 开头的系统私有字体。在后台线程调用；不缓存，
/// 查询结果用完即释放。
pub fn installed_families() -> Vec<String> {
    let Ok(collection) = font_enumeration::Collection::new() else { return Vec::new() };
    let mut list: Vec<String> = collection
        .take()
        .into_iter()
        .map(|f| f.family_name)
        .filter(|n| !n.trim().is_empty() && !n.starts_with('.'))
        .collect();
    list.sort_by_key(|n| n.to_lowercase());
    list.dedup();
    list
}

fn register(path: &Path) -> Result<(), String> {
    platform::register(path)
}

fn unregister(path: &Path) {
    platform::unregister(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_family_names_of_system_fonts() {
        // macOS 自带的字体文件：单个字体与字体集合。
        for (file, family) in [
            ("/System/Library/Fonts/Supplemental/Arial.ttf", "Arial"),
            ("/System/Library/Fonts/Helvetica.ttc", "Helvetica"),
        ] {
            let Ok(data) = std::fs::read(file) else { continue };
            let names = family_names(&data);
            assert!(names.iter().any(|n| n == family), "{file}: {names:?}");
        }
        assert!(family_names(b"not a font").is_empty());
    }

    #[test]
    fn lists_installed_families() {
        let started = std::time::Instant::now();
        let list = installed_families();
        eprintln!("{} 个字体家族，{:?}", list.len(), started.elapsed());
        assert!(list.len() > 20, "{} 个", list.len());
        // 各系统必有的界面字体。
        let expected: &[&str] = if cfg!(windows) { &["Segoe UI", "Arial"] } else { &["Helvetica", "PingFang SC"] };
        assert!(list.iter().any(|f| expected.contains(&f.as_str())), "{list:?}");
        assert!(list.iter().all(|f| !f.starts_with('.')));
    }
}
