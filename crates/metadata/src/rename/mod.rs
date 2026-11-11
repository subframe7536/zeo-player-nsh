//! 纯文件模板重命名（元数据视图）：只允许改文件名本身——模板里出现
//! 目录分隔符直接拒掉，标签值里的 `/` `\` 清洗成 `_`，目标路径始终
//! 留在原父目录下，绝不新建目录。音频改名时同名 `.lrc` 与同名图片
//! 侧车跟着换到新主干（保留各自扩展名）。

use std::collections::HashMap;

use super::write::TagField;

mod apply;
mod plan;
#[cfg(test)]
mod tests;

pub use apply::*;
pub use plan::*;

/// 模板里含目录分隔符时的报错（纯文件重命名的硬规则，先查它）。
pub const SEPARATOR_ERROR: &str = "仅支持重命名文件名，不能包含目录分隔符（/ 或 \\）";

/// 文件名（stem + `.` + ext）的字节上限（ext4 / NTFS / APFS 一致）。
pub(crate) const MAX_NAME_BYTES: usize = 255;

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
pub(crate) fn truncate_bytes(name: &str, max: usize) -> &str {
    if name.len() <= max {
        return name;
    }
    let mut end = max;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}
