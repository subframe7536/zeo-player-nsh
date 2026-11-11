/// 流派切分：任一分隔符处切开 → trim 每段 → 去空段、去重。
pub fn split_genres(raw: &[String], separators: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in raw {
        let mut parts = vec![value.as_str()];
        for sep in separators.iter().filter(|s| !s.is_empty()) {
            parts = parts
                .into_iter()
                .flat_map(|p| p.split(sep.as_str()))
                .collect();
        }
        for part in parts {
            let part = part.trim();
            if !part.is_empty() && !out.iter().any(|g| g == part) {
                out.push(part.to_owned());
            }
        }
    }
    out
}

/// 艺术家切分：任一分隔符处切开 → trim 每段 → 去空段、去重。
pub fn split_artists(raw: &str, separators: &[String]) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }
    let mut parts = vec![raw];
    for sep in separators.iter().filter(|s| !s.is_empty()) {
        parts = parts
            .into_iter()
            .flat_map(|p| p.split(sep.as_str()))
            .collect();
    }
    let mut out: Vec<String> = Vec::new();
    for part in parts {
        let part = part.trim();
        if !part.is_empty() && !out.iter().any(|a| a == part) {
            out.push(part.to_owned());
        }
    }
    out
}
