//! 界面字体：用 font-enumeration 列出系统已安装的字体家族（macOS CoreText、
//! Windows DirectWrite、Linux fontconfig）。

/// 系统字体家族：得到的家族名就是渲染时能解析的名字。排序、去重，不含以
/// `.` 开头的系统私有字体。在后台线程调用；不缓存，查询结果用完即释放。
pub fn installed_families() -> Vec<String> {
    let Ok(collection) = font_enumeration::Collection::new() else {
        return Vec::new();
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_installed_families() {
        let list = installed_families();
        // macOS 与 Windows 都自带 Arial。
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            assert!(list.iter().any(|n| n == "Arial"), "{} 个家族", list.len());
        }
        assert!(list.windows(2).all(|w| w[0] != w[1]), "已去重");
    }
}
