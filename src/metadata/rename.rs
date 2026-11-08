//! 纯文件模板重命名（元数据视图）：只允许改文件名本身——模板里出现
//! 目录分隔符直接拒掉，标签值里的 `/` `\` 清洗成 `_`，目标路径始终
//! 留在原父目录下，绝不新建目录。音频改名时同名 `.lrc` 与同名图片
//! 侧车跟着换到新主干（保留各自扩展名）。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::write::TagField;

/// 模板里含目录分隔符时的报错（纯文件重命名的硬规则，先查它）。
pub const SEPARATOR_ERROR: &str = "仅支持重命名文件名，不能包含目录分隔符（/ 或 \\）";

/// 文件名（stem + `.` + ext）的字节上限（ext4 / NTFS / APFS 一致）。
const MAX_NAME_BYTES: usize = 255;

/// 解析好的重命名模板（字面文本与占位符按序排列）。
#[allow(dead_code, reason = "元数据视图接入用（metadata_view_plan 阶段三）")]
#[derive(Debug, Clone, PartialEq)]
pub struct Template(Vec<Part>);

/// 模板的一段：字面文本或一个占位符。
#[derive(Debug, Clone, PartialEq)]
enum Part {
    Text(String),
    Field {
        field: FieldKind,
        /// `{name:N}` 的宽度（0 = 不补位）。
        width: usize,
        /// 前导 0 补位（`{name:0N}` / `{name:0Nd}`，仅数字值生效）。
        zero: bool,
    },
}

/// 占位符的取值来源：标签字段或原文件名主干。
#[derive(Clone, Copy, Debug, PartialEq)]
enum FieldKind {
    Tag(TagField),
    Filename,
}

#[allow(dead_code, reason = "元数据视图接入用（metadata_view_plan 阶段三）")]
impl Template {
    /// 解析模板：先拦目录分隔符，再逐段解析 `{name}` / `{name:N}` /
    /// `{name:0N}` / `{name:0Nd}` 占位符与 `{{` `}}` 字面大括号。
    pub fn parse(src: &str) -> Result<Template, String> {
        if src.contains('/') || src.contains('\\') {
            return Err(SEPARATOR_ERROR.to_owned());
        }
        if src.trim().is_empty() {
            return Err("模板不能为空".to_owned());
        }
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut chars = src.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    if chars.next_if_eq(&'{').is_some() {
                        text.push('{');
                        continue;
                    }
                    let mut inside = String::new();
                    let mut closed = false;
                    for c in chars.by_ref() {
                        if c == '}' {
                            closed = true;
                            break;
                        }
                        inside.push(c);
                    }
                    if !closed {
                        return Err("模板里有未闭合的 {".to_owned());
                    }
                    if !text.is_empty() {
                        parts.push(Part::Text(std::mem::take(&mut text)));
                    }
                    parts.push(parse_placeholder(&inside)?);
                }
                '}' => {
                    if chars.next_if_eq(&'}').is_some() {
                        text.push('}');
                    } else {
                        return Err("模板里有未配对的 }".to_owned());
                    }
                }
                _ => text.push(c),
            }
        }
        if !text.is_empty() {
            parts.push(Part::Text(text));
        }
        Ok(Template(parts))
    }

    /// 用标签值渲染出文件名主干：占位符的值先过 [`sanitize_value`]
    /// （值里的 `/` `\` 换 `_`、不衍生目录），整串再过 [`sanitize`]
    /// 兜底其余非法字符。
    pub fn render(&self, fields: &HashMap<TagField, String>, stem: &str) -> String {
        let mut out = String::new();
        for part in &self.0 {
            match part {
                Part::Text(text) => out.push_str(text),
                Part::Field { field, width, zero } => {
                    let raw = match field {
                        FieldKind::Tag(tag) => fields.get(tag).map(String::as_str).unwrap_or(""),
                        FieldKind::Filename => stem,
                    };
                    out.push_str(&pad(&sanitize_value(raw), *width, *zero));
                }
            }
        }
        sanitize(&out)
    }
}

/// `{name}` / `{name:spec}`：spec 是 `[0]N[d]`（前导 0 = 零补位，
/// 结尾 `d` 是显式的数字标记，两者可组合）。
fn parse_placeholder(inside: &str) -> Result<Part, String> {
    let (name, spec) = inside
        .split_once(':')
        .map_or((inside, None), |(n, s)| (n, Some(s)));
    let name = name.trim();
    let field = field_kind(name).ok_or_else(|| format!("未知占位符 {{{name}}}"))?;
    let (width, zero) = match spec {
        None => (0, false),
        Some(spec) => {
            let digits = spec.strip_suffix('d').unwrap_or(spec);
            let width: usize = digits
                .parse()
                .map_err(|_| format!("占位符格式无效 {{{inside}}}"))?;
            (width, digits.starts_with('0'))
        }
    };
    Ok(Part::Field { field, width, zero })
}

/// 占位符名 → 取值来源。
fn field_kind(name: &str) -> Option<FieldKind> {
    use TagField::*;
    Some(match name {
        "title" => FieldKind::Tag(Title),
        "artist" => FieldKind::Tag(Artist),
        "album" => FieldKind::Tag(Album),
        "album_artist" => FieldKind::Tag(AlbumArtist),
        "genre" => FieldKind::Tag(Genre),
        "year" => FieldKind::Tag(Year),
        "track" => FieldKind::Tag(Track),
        "track_total" => FieldKind::Tag(TrackTotal),
        "disc" => FieldKind::Tag(Disc),
        "disc_total" => FieldKind::Tag(DiscTotal),
        "composer" => FieldKind::Tag(Composer),
        "conductor" => FieldKind::Tag(Conductor),
        "lyricist" => FieldKind::Tag(Lyricist),
        "publisher" => FieldKind::Tag(Publisher),
        "filename" => FieldKind::Filename,
        _ => return None,
    })
}

/// 宽度补位：零补位仅数字值前导 0 补齐（非数字与空串原样返回）；
/// 空格补位左填空格右对齐。
fn pad(value: &str, width: usize, zero: bool) -> String {
    if width == 0 || value.chars().count() >= width {
        return value.to_owned();
    }
    if zero {
        if let Ok(n) = value.parse::<u64>() {
            return format!("{n:0width$}");
        }
        return value.to_owned();
    }
    format!("{}{value}", " ".repeat(width - value.chars().count()))
}

/// 占位符值的清洗：只把 `/` `\` 换成 `_`（值里不允许衍生出目录）；
/// 其余非法字符由整串的 [`sanitize`] 兜底。
fn sanitize_value(value: &str) -> String {
    value.replace(['/', '\\'], "_")
}

/// 文件名的通用清洗：`< > : " / \ | ? *` 与 ASCII 控制字符换 `_`，
/// 去首尾空白与结尾的点；Windows 保留名（CON、PRN、AUX、NUL、
/// COM1-9、LPT1-9，不分大小写）加 `_` 前缀。
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                || (c.is_ascii() && c.is_control())
            {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned
        .trim()
        .trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.iter().any(|r| trimmed.eq_ignore_ascii_case(r)) {
        return format!("_{trimmed}");
    }
    trimmed.to_owned()
}

/// 按字节上限截断（停在字符边界上）。
fn truncate_bytes(name: &str, max: usize) -> &str {
    if name.len() <= max {
        return name;
    }
    let mut end = max;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

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
fn sidecars_of(audio: &Path) -> Vec<PathBuf> {
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
    all.extend(crate::metadata::covers::sidecar_files(audio));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 独立临时目录（测试之间互不干扰）。
    fn scratch_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "zeo-rename-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fields(pairs: &[(TagField, &str)]) -> HashMap<TagField, String> {
        pairs.iter().map(|(f, v)| (*f, (*v).to_owned())).collect()
    }

    /// 目录分隔符是硬规则（先查它，哪怕模板其余部分也合法）。
    #[test]
    fn template_rejects_separators() {
        for bad in ["a/b", "{artist}/{title}", "a\\b", "{artist}\\{title}"] {
            assert_eq!(
                Template::parse(bad),
                Err(SEPARATOR_ERROR.to_owned()),
                "{bad}"
            );
        }
    }

    /// 模板语法：空模板、未知占位符、未闭合 / 未配对的大括号。
    #[test]
    fn template_rejects_bad_syntax() {
        assert_eq!(Template::parse("   "), Err("模板不能为空".to_owned()));
        assert_eq!(
            Template::parse("{artis}"),
            Err("未知占位符 {artis}".to_owned())
        );
        assert_eq!(
            Template::parse("{title"),
            Err("模板里有未闭合的 {".to_owned())
        );
        assert_eq!(
            Template::parse("title}"),
            Err("模板里有未配对的 }".to_owned())
        );
    }

    /// `{name}` / `{name:N}` / `{name:0N}` / `{name:0Nd}` 与 `{{` `}}`。
    #[test]
    fn template_renders_fields_and_padding() {
        let t = Template::parse("{track:02d} - {title}").unwrap();
        assert_eq!(
            t.render(
                &fields(&[(TagField::Track, "1"), (TagField::Title, "夜曲")]),
                "x"
            ),
            "01 - 夜曲"
        );
        // 显式 d 与空格补位；非数字值不零补。
        let t = Template::parse("{track:03d},{title:6},{track}").unwrap();
        assert_eq!(
            t.render(
                &fields(&[(TagField::Track, "abc"), (TagField::Title, "歌")]),
                "x"
            ),
            "abc,     歌,abc"
        );
        // 字面大括号。
        let t = Template::parse("{{{title}}}").unwrap();
        assert_eq!(t.render(&fields(&[(TagField::Title, "歌")]), "x"), "{歌}");
    }

    /// 值里的 `/` `\` 换 `_`（不衍生目录），其余非法字符整串兜底清洗；
    /// Windows 保留名加 `_` 前缀。
    #[test]
    fn template_sanitizes_values() {
        let t = Template::parse("{artist} - {title}").unwrap();
        assert_eq!(
            t.render(
                &fields(&[(TagField::Artist, "AC/DC"), (TagField::Title, "a\\b")]),
                "x"
            ),
            "AC_DC - a_b"
        );
        let t = Template::parse("{title}").unwrap();
        assert_eq!(
            t.render(&fields(&[(TagField::Title, "a<b>:\"c|?*d")]), "x"),
            "a_b___c___d"
        );
        assert_eq!(t.render(&fields(&[(TagField::Title, "CON")]), "x"), "_CON");
        assert_eq!(
            t.render(&fields(&[(TagField::Title, "com1")]), "x"),
            "_com1"
        );
        // 文件名去头尾空白与结尾的点。
        assert_eq!(
            t.render(&fields(&[(TagField::Title, "  name.  ")]), "x"),
            "name"
        );
    }

    /// `{filename}` 取原主干；渲染出原名 → Unchanged，大小写差异 → Ok。
    #[test]
    fn plan_unchanged_and_case_only() {
        let dir = scratch_dir();
        let src = dir.join("Song.mp3");
        fs::write(&src, b"x").unwrap();
        let t = Template::parse("{filename}").unwrap();
        let out = plan(&[(src.to_string_lossy().into_owned(), HashMap::new())], &t);
        assert_eq!(out[0].status, Status::Unchanged);

        let t = Template::parse("{title}").unwrap();
        let out = plan(
            &[(
                src.to_string_lossy().into_owned(),
                fields(&[(TagField::Title, "SONG")]),
            )],
            &t,
        );
        assert_eq!(out[0].status, Status::Ok, "仅大小写不同应放行");
        assert!(out[0].dest.ends_with("SONG.mp3"));
    }

    /// 磁盘撞名与批次内撞名都是 Conflict。
    #[test]
    fn plan_conflicts() {
        let dir = scratch_dir();
        let taken = dir.join("taken.mp3");
        let other = dir.join("other.mp3");
        fs::write(&taken, b"x").unwrap();
        fs::write(&other, b"x").unwrap();
        let t = Template::parse("{title}").unwrap();

        // 磁盘已有同名文件。
        let out = plan(
            &[(
                other.to_string_lossy().into_owned(),
                fields(&[(TagField::Title, "taken")]),
            )],
            &t,
        );
        assert_eq!(
            out[0].status,
            Status::Conflict("目录里已有同名文件".to_owned())
        );

        // 批次内两条计划渲出同名。
        let a = dir.join("a.mp3");
        let b = dir.join("b.mp3");
        fs::write(&a, b"x").unwrap();
        fs::write(&b, b"x").unwrap();
        let out = plan(
            &[
                (
                    a.to_string_lossy().into_owned(),
                    fields(&[(TagField::Title, "same")]),
                ),
                (
                    b.to_string_lossy().into_owned(),
                    fields(&[(TagField::Title, "SAME")]),
                ),
            ],
            &t,
        );
        for p in &out {
            assert_eq!(
                p.status,
                Status::Conflict("与批次内其他文件重名".to_owned()),
                "{p:?}"
            );
        }
    }

    /// 空渲染结果 → Invalid；侧车目标被占 → Conflict。
    #[test]
    fn plan_invalid_and_sidecar_conflict() {
        let dir = scratch_dir();
        let src = dir.join("a.mp3");
        fs::write(&src, b"x").unwrap();
        let t = Template::parse("{title}").unwrap();
        let out = plan(&[(src.to_string_lossy().into_owned(), HashMap::new())], &t);
        assert_eq!(out[0].status, Status::Invalid("文件名为空".to_owned()));

        // a.lrc 跟着改成 b.lrc，但 b.lrc 已存在。
        fs::write(dir.join("a.lrc"), b"[00:00]x").unwrap();
        fs::write(dir.join("b.lrc"), b"occupied").unwrap();
        let out = plan(
            &[(
                src.to_string_lossy().into_owned(),
                fields(&[(TagField::Title, "b")]),
            )],
            &t,
        );
        assert_eq!(
            out[0].status,
            Status::Conflict("同名歌词 / 封面文件已存在".to_owned())
        );
    }

    /// apply：音频 + `.lrc` + `.jpg` 侧车一起换主干，全部留在原目录。
    #[test]
    fn apply_renames_audio_and_sidecars() {
        let dir = scratch_dir();
        let src = dir.join("old.flac");
        fs::write(&src, b"audio").unwrap();
        fs::write(dir.join("old.lrc"), b"[00:00]x").unwrap();
        fs::write(dir.join("old.jpg"), b"img").unwrap();
        let t = Template::parse("{track:02d} - {title}").unwrap();
        let out = plan(
            &[(
                src.to_string_lossy().into_owned(),
                fields(&[(TagField::Track, "7"), (TagField::Title, "新歌")]),
            )],
            &t,
        );
        assert_eq!(out[0].status, Status::Ok);
        let (moves, error) = apply(&out[0]);
        assert_eq!(error, None);
        let dest = dir.join("07 - 新歌.flac");
        assert_eq!(
            moves,
            vec![
                (
                    src.to_string_lossy().into_owned(),
                    dest.to_string_lossy().into_owned()
                ),
                (
                    dir.join("old.lrc").to_string_lossy().into_owned(),
                    dir.join("07 - 新歌.lrc").to_string_lossy().into_owned()
                ),
                (
                    dir.join("old.jpg").to_string_lossy().into_owned(),
                    dir.join("07 - 新歌.jpg").to_string_lossy().into_owned()
                ),
            ]
        );
        // 目录里只有改完的三个文件，没有新建目录、没有残件。
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            {
                let mut n = names;
                n.sort();
                n
            },
            ["07 - 新歌.flac", "07 - 新歌.jpg", "07 - 新歌.lrc"]
        );
        assert_eq!(fs::read(dir.join("07 - 新歌.flac")).unwrap(), b"audio");
    }

    /// apply 只跑 Ok：Unchanged 空移动，Conflict / Invalid 带错返回。
    #[test]
    fn apply_gates_on_status() {
        let unchanged = Planned {
            src: "a.mp3".to_owned(),
            dest: "a.mp3".to_owned(),
            status: Status::Unchanged,
        };
        assert_eq!(apply(&unchanged), (Vec::new(), None));
        let conflict = Planned {
            src: "a.mp3".to_owned(),
            dest: "b.mp3".to_owned(),
            status: Status::Conflict("撞名".to_owned()),
        };
        assert_eq!(apply(&conflict), (Vec::new(), Some("撞名".to_owned())));
    }

    /// `plan_name`：目录分隔符一律 Invalid（硬规则，先查），dest 保持
    /// 源路径不变。
    #[test]
    fn plan_name_rejects_separators() {
        let dir = scratch_dir();
        let src = dir.join("song.flac");
        fs::write(&src, b"x").unwrap();
        let src = src.to_string_lossy().into_owned();
        for bad in ["a/b", "a\\b", "x / y"] {
            let p = plan_name(&src, bad);
            assert_eq!(
                p.status,
                Status::Invalid(SEPARATOR_ERROR.to_owned()),
                "{bad}"
            );
            assert_eq!(p.dest, src, "{bad}");
        }
    }

    /// `plan_name`：顺手带上的「.原扩展名」剥掉（不分大小写），剥完
    /// 才清洗；剥的不是原扩展名就当普通字符留下。
    #[test]
    fn plan_name_strips_original_ext() {
        let dir = scratch_dir();
        let src = dir.join("song.flac");
        fs::write(&src, b"x").unwrap();
        let src = src.to_string_lossy().into_owned();
        for typed in ["new name.flac", "new name.FLAC", "new name"] {
            let p = plan_name(&src, typed);
            assert_eq!(p.status, Status::Ok, "{typed}: {p:?}");
            assert!(p.dest.ends_with("new name.flac"), "{typed}: {}", p.dest);
        }
        // `.mp3` 不是原扩展名：留在主干里，扩展名仍是 `.flac`。
        let p = plan_name(&src, "new.mp3");
        assert_eq!(p.status, Status::Ok);
        assert!(p.dest.ends_with("new.mp3.flac"), "{}", p.dest);
        // 只敲了扩展名：剥完剩空名 → Invalid。
        assert_eq!(
            plan_name(&src, ".flac").status,
            Status::Invalid("文件名为空".to_owned())
        );
    }

    /// `plan_name`：非法字符 / Windows 保留名走 `sanitize` 同款清洗。
    #[test]
    fn plan_name_sanitizes() {
        let dir = scratch_dir();
        let src = dir.join("song.flac");
        fs::write(&src, b"x").unwrap();
        let src = src.to_string_lossy().into_owned();
        let p = plan_name(&src, "a:b*c");
        assert_eq!(p.status, Status::Ok);
        assert!(p.dest.ends_with("a_b_c.flac"), "{}", p.dest);
        let p = plan_name(&src, "con");
        assert_eq!(p.status, Status::Ok);
        assert!(p.dest.ends_with("_con.flac"), "{}", p.dest);
        // 首尾空白与结尾的点去掉。
        let p = plan_name(&src, "  done.  ");
        assert_eq!(p.status, Status::Ok);
        assert!(p.dest.ends_with("done.flac"), "{}", p.dest);
    }

    /// `plan_name`：原名 → Unchanged；磁盘撞名 → Conflict；空名 →
    /// Invalid。批次内撞名不参与（单条计划）。
    #[test]
    fn plan_name_statuses() {
        let dir = scratch_dir();
        let src = dir.join("song.flac");
        fs::write(&src, b"x").unwrap();
        let src = src.to_string_lossy().into_owned();
        assert_eq!(plan_name(&src, "song").status, Status::Unchanged);
        // 大小写差异的原地改名 → Ok。
        let p = plan_name(&src, "SONG");
        assert_eq!(p.status, Status::Ok);
        assert!(p.dest.ends_with("SONG.flac"), "{}", p.dest);
        // 磁盘已有同名文件 → Conflict。
        fs::write(dir.join("taken.flac"), b"x").unwrap();
        assert_eq!(
            plan_name(&src, "taken").status,
            Status::Conflict("目录里已有同名文件".to_owned())
        );
        // 同名 `.lrc` 侧车目标被占 → Conflict（与模板计划同规）。
        fs::write(dir.join("song.lrc"), b"[00:00]x").unwrap();
        fs::write(dir.join("b.lrc"), b"occupied").unwrap();
        assert_eq!(
            plan_name(&src, "b").status,
            Status::Conflict("同名歌词 / 封面文件已存在".to_owned())
        );
    }
}
