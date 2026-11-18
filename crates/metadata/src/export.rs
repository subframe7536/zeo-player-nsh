//! 外部文件的导入 / 导出：标签乱码修复、外部文本文件解码、歌词导出格式化
//! （去时间戳 / 去空行）、封面转码与原子落盘。纯函数居多，界面线程之外调用。

use std::collections::HashSet;
use std::fs;
use std::io::Cursor;
use std::path::Path;

use encoding_rs::Encoding;

use crate::lyrics::{clean_content, is_meta_tag_line, split_timestamps};

// --- 标签乱码修复 -------------------------------------------------------------

/// 乱码修复会试的原始编码，按并列时的优先顺序：UTF-8（双重编码）、简体中文、
/// 繁体中文、日文、韩文。没有西里尔 / 单字节西欧编码——它们对任何字节都「解码
/// 成功」，会把正常的带重音拉丁文本误改成西里尔字母。
const MOJIBAKE_CANDIDATES: [&Encoding; 5] = [
    encoding_rs::UTF_8,
    encoding_rs::GB18030,
    encoding_rs::BIG5,
    encoding_rs::SHIFT_JIS,
    encoding_rs::EUC_KR,
];

/// 乱码里的字符 → 原字节：Latin-1 范围直接取码点；超出的（Windows-1252 把
/// 0x80–0x9F 读成 `€` `‚` `™` 一类）按 Windows-1252 反查；反查不出就不是这种乱码。
fn mojibake_bytes(text: &str) -> Option<Vec<u8>> {
    text.chars()
        .map(|c| {
            if let Ok(byte) = u8::try_from(u32::from(c)) {
                return Some(byte);
            }
            let mut buf = [0u8; 4];
            let (bytes, _, unmappable) = encoding_rs::WINDOWS_1252.encode(c.encode_utf8(&mut buf));
            (!unmappable && bytes.len() == 1).then(|| bytes[0])
        })
        .collect()
}

/// 文字属于「像样的」东亚文字（汉字、假名、谚文）。
fn is_script_char(c: char) -> bool {
    matches!(u32::from(c),
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0x3040..=0x30FF | 0xAC00..=0xD7AF)
}

/// 解码结果里不该出现的字符：替换符、控制符（换行 / 制表除外，歌词里有）、私用区。
fn is_garbage_char(c: char) -> bool {
    c == '\u{FFFD}'
        || (c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        || matches!(u32::from(c), 0xE000..=0xF8FF)
}

/// 一个解码出的字符在候选编码里「常用」的程度（越高越像真文本）：常用字区的汉字 /
/// 假名 / 谚文 2 分，其余汉字 1 分，非文字 0 分。用来在 GBK / Big5 / Shift-JIS /
/// EUC-KR 都「能解」的字节串里挑最像人话的那个。
fn char_weight(encoding: &'static Encoding, c: char) -> i32 {
    if !is_script_char(c) {
        return 0;
    }
    if std::ptr::eq(encoding, encoding_rs::UTF_8) {
        return 3; // 严格合法的 UTF-8 字节串几乎不会是巧合
    }
    let mut buf = [0u8; 4];
    let (bytes, _, unmappable) = encoding.encode(c.encode_utf8(&mut buf));
    if unmappable || bytes.len() != 2 {
        return 1;
    }
    let (lead, trail) = (bytes[0], bytes[1]);
    let common = if std::ptr::eq(encoding, encoding_rs::GB18030) {
        // GB2312 一级 / 二级汉字区。
        (0xB0..=0xF7).contains(&lead) && trail >= 0xA1
    } else if std::ptr::eq(encoding, encoding_rs::BIG5) {
        // Big5 常用字区。
        (0xA4..=0xC6).contains(&lead)
    } else if std::ptr::eq(encoding, encoding_rs::SHIFT_JIS) {
        // 假名与 JIS 一级汉字。
        (0x81..=0x98).contains(&lead)
    } else {
        // EUC-KR：谚文音节区（汉字区 0xCA 起不算常用）。
        (0xB0..=0xC8).contains(&lead)
    };
    if common { 2 } else { 1 }
}

/// 尽力把「别的编码被当成 Latin-1 / Windows-1252 读成的乱码」转回正常文本（UTF-8
/// 字符串）：自动在 UTF-8（双重编码）、GBK、Big5、Shift-JIS、EUC-KR 里挑最像人话的
/// 解码；认不准、不像乱码、解码结果可疑就返回 `None`（保持原值）。
///
/// 局限：简体中文与韩文的字节区几乎重叠，同一串字节两者都解得开时没有词典可判，
/// 并列取简体中文；繁体 / 日文靠「常用字区」打分区分，短字符串仍可能认错——认错
/// 的代价是改成另一种看不懂的字，所以界面上有预览，可撤回（标签历史）。
///
/// 防误伤正常的带重音文本（`Björk` `Ações`）：纯 ASCII 不动；至少要有一段连续的
/// 两个以上高位字节；高位字节要么至少 4 个、要么占满整个字符串；解码结果不能含
/// 替换符 / 控制符 / 私用区，且绝大多数非 ASCII 字符得是汉字 / 假名 / 谚文。
pub fn fix_mojibake(text: &str) -> Option<String> {
    let bytes = mojibake_bytes(text)?;
    let high = bytes.iter().filter(|b| **b >= 0x80).count();
    if high == 0 {
        return None;
    }
    let has_pair = bytes.windows(2).any(|w| w[0] >= 0x80 && w[1] >= 0x80);
    let total = text.chars().filter(|c| !c.is_whitespace()).count();
    if !has_pair || (high < 4 && high != total) {
        return None;
    }
    MOJIBAKE_CANDIDATES
        .iter()
        .filter_map(|encoding| {
            let decoded = encoding
                .decode_without_bom_handling_and_without_replacement(&bytes)?
                .into_owned();
            if decoded.chars().any(is_garbage_char) {
                return None;
            }
            // 汉字夹在两个 ASCII 字母中间（`A?es`）是带重音的西文单词被误解码的典型样子。
            let chars: Vec<char> = decoded.chars().collect();
            if chars.windows(3).any(|w| {
                w[0].is_ascii_alphabetic() && is_script_char(w[1]) && w[2].is_ascii_alphabetic()
            }) {
                return None;
            }
            let non_ascii = decoded.chars().filter(|c| !c.is_ascii()).count();
            let script = decoded.chars().filter(|c| is_script_char(*c)).count();
            if script == 0 || script * 10 < non_ascii * 8 {
                return None;
            }
            let score: i32 = decoded.chars().map(|c| char_weight(encoding, c)).sum();
            Some((score, decoded))
        })
        // 同分取靠前的候选（`max_by_key` 并列时取最后一个，所以反过来比较）。
        .reduce(|best, next| if next.0 > best.0 { next } else { best })
        .map(|(_, decoded)| decoded)
        .filter(|decoded| decoded != text)
}

/// 外部文本文件（歌词）的字节 → 文本：认 UTF-8 / UTF-16 BOM；无 BOM 先试
/// 严格 UTF-8，不是就按 GB18030（GBK 超集，国内 LRC 最常见）；还不行用
/// 宽松 UTF-8 兜底。
pub fn decode_text_file(bytes: &[u8]) -> String {
    if let Some((encoding, bom)) = Encoding::for_bom(bytes) {
        return encoding
            .decode_without_bom_handling(&bytes[bom..])
            .0
            .into_owned();
    }
    if let Some(text) =
        encoding_rs::UTF_8.decode_without_bom_handling_and_without_replacement(bytes)
    {
        return text.into_owned();
    }
    encoding_rs::GB18030
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|text| text.into_owned())
        .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned())
}

// --- 歌词导出格式化 -----------------------------------------------------------

/// 导出歌词时的整理选项。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LyricsFormat {
    /// 去掉时间戳（含卡拉 OK 碎片和 `[ti:]` 一类头标签），只留歌词文字；
    /// 一行多个时间戳（副歌）按时间展开成多行，全是带时间戳的行时按时间排序。
    pub strip_timestamps: bool,
    /// 去掉空行（含只有时间戳的空歌词行）。
    pub remove_blank_lines: bool,
}

/// 按 [`LyricsFormat`] 整理歌词文本；换行统一成 `\n`，结尾带一个换行。
/// 两个选项都关就只是规整换行。
pub fn format_lyrics(text: &str, format: LyricsFormat) -> String {
    // (时间, 输出行)：时间只在去时间戳时用来排序。
    let mut out: Vec<(Option<f64>, String)> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end_matches(['\r', '\n']);
        let (times, content) = split_timestamps(line);
        let meta = times.is_empty() && is_meta_tag_line(line);
        let blank = !meta && clean_content(&content).trim().is_empty();
        if format.remove_blank_lines && blank {
            continue;
        }
        if !format.strip_timestamps {
            out.push((None, line.to_owned()));
            continue;
        }
        if meta {
            continue;
        }
        let content = clean_content(&content).trim().to_owned();
        if times.is_empty() {
            out.push((None, clean_content(line).trim().to_owned()));
        } else {
            out.extend(times.into_iter().map(|t| (Some(t), content.clone())));
        }
    }
    if format.strip_timestamps && out.iter().any(|(t, _)| t.is_some()) {
        // 带时间戳的文件里，行间无时间戳的空行只是排版噪声；剩下全是带时间戳
        // 的行就按时间排序（夹着无时间戳的歌词行时保持文件顺序）。
        out.retain(|(t, line)| t.is_some() || !line.is_empty());
        if out.iter().all(|(t, _)| t.is_some()) {
            out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        }
    }
    let mut text = out
        .into_iter()
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

// --- 封面导出 -----------------------------------------------------------------

/// 导出封面的图片格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverFormat {
    /// 内嵌图片原样写出（字节不动、后缀按原格式）。
    Original,
    Jpeg,
    Png,
    Webp,
}

impl CoverFormat {
    pub const ALL: [CoverFormat; 4] = [
        CoverFormat::Original,
        CoverFormat::Jpeg,
        CoverFormat::Png,
        CoverFormat::Webp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CoverFormat::Original => "保持原格式",
            CoverFormat::Jpeg => "JPEG",
            CoverFormat::Png => "PNG",
            CoverFormat::Webp => "WebP（无损）",
        }
    }
}

/// JPEG 转码质量。
const JPEG_QUALITY: u8 = 92;

/// 内嵌封面字节 → `(导出字节, 后缀)`。目标格式与原图相同时不重编码（不损）；
/// 否则解码后转码（JPEG 先丢掉透明通道）。
pub fn encode_cover(data: &[u8], format: CoverFormat) -> Result<(Vec<u8>, &'static str), String> {
    use image::ImageFormat;
    let source = image::guess_format(data).map_err(|e| format!("无法识别封面图片格式: {e}"))?;
    let (target, ext) = match (format, source) {
        (CoverFormat::Original, ImageFormat::Jpeg) | (CoverFormat::Jpeg, _) => {
            (ImageFormat::Jpeg, "jpg")
        }
        (CoverFormat::Original, ImageFormat::Png) | (CoverFormat::Png, _) => {
            (ImageFormat::Png, "png")
        }
        (CoverFormat::Original, ImageFormat::WebP) | (CoverFormat::Webp, _) => {
            (ImageFormat::WebP, "webp")
        }
        (CoverFormat::Original, ImageFormat::Gif) => (ImageFormat::Gif, "gif"),
        (CoverFormat::Original, ImageFormat::Bmp) => (ImageFormat::Bmp, "bmp"),
        (CoverFormat::Original, other) => {
            return Err(format!("不支持导出 {other:?} 格式的封面"));
        }
    };
    if target == source {
        return Ok((data.to_vec(), ext));
    }
    let img = image::load_from_memory(data).map_err(|e| format!("解码封面图片失败: {e}"))?;
    let mut out = Cursor::new(Vec::new());
    match target {
        ImageFormat::Jpeg => {
            let rgb = img.to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
                .encode_image(&rgb)
                .map_err(|e| format!("封面转码 JPEG 失败: {e}"))?;
        }
        other => img
            .write_to(&mut out, other)
            .map_err(|e| format!("封面转码 {ext} 失败: {e}"))?,
    }
    Ok((out.into_inner(), ext))
}

// --- 落盘 ---------------------------------------------------------------------

/// 原子写一个导出文件（同目录临时文件 → `rename`）。目标已存在且不覆盖时
/// 不动它、返回 `Ok(false)`；写成返回 `Ok(true)`。目录不存在会先建。
pub fn write_export(target: &Path, bytes: &[u8], overwrite: bool) -> Result<bool, String> {
    if !overwrite && target.exists() {
        return Ok(false);
    }
    let Some(dir) = target.parent() else {
        return Err(format!("无效的导出路径 {}", target.display()));
    };
    fs::create_dir_all(dir).map_err(|e| format!("无法创建目录 {}: {e}", dir.display()))?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.zeo-tmp"));
    let result = fs::write(&tmp, bytes)
        .map_err(|e| format!("写临时文件失败: {e}"))
        .and_then(|()| {
            fs::rename(&tmp, target).map_err(|e| format!("写入 {} 失败: {e}", target.display()))
        });
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map(|()| true)
}

/// 批量导出到同一个文件夹时给重名的文件名加 ` (2)` `(3)`…（不分大小写，
/// 与 Windows / macOS 默认文件系统一致）；`used` 记着这一批已占用的名字。
pub fn dedupe_stem(used: &mut HashSet<String>, stem: &str) -> String {
    let mut candidate = stem.to_owned();
    let mut n = 2;
    while !used.insert(candidate.to_lowercase()) {
        candidate = format!("{stem} ({n})");
        n += 1;
    }
    candidate
}

// --- 批量导出 -----------------------------------------------------------------

/// 批量导出的内容。
#[derive(Debug, Clone, Copy)]
pub enum ExportKind {
    /// 内嵌歌词 → 文本文件（后缀由调用方定，`lrc` / `txt`）。内容一律是
    /// UTF-8、不带 BOM（`format_lyrics` 的 `String` 直接取字节）。
    Lyrics {
        ext: &'static str,
        format: LyricsFormat,
    },
    /// 内嵌封面 → 图片文件。
    Cover(CoverFormat),
}

/// 一次批量导出的结果统计。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ExportSummary {
    pub written: usize,
    /// 目标已存在且不覆盖。
    pub skipped: usize,
    /// 文件里没有内嵌歌词 / 封面。
    pub no_data: usize,
    pub failed: Vec<String>,
}

impl ExportSummary {
    /// 一句话汇总（`noun` 是「歌词」/「封面」）。
    pub fn describe(&self, noun: &str) -> String {
        let mut parts = vec![format!("已导出 {} 个", self.written)];
        if self.skipped > 0 {
            parts.push(format!("跳过 {} 个已存在的", self.skipped));
        }
        if self.no_data > 0 {
            parts.push(format!("{} 首没有内嵌{noun}", self.no_data));
        }
        if !self.failed.is_empty() {
            let shown: Vec<&str> = self.failed.iter().take(2).map(String::as_str).collect();
            parts.push(format!(
                "{} 个失败：{}",
                self.failed.len(),
                shown.join("；")
            ));
        }
        parts.join("，")
    }
}

/// 把 `jobs`（`(音频路径, 文件夹模式下的文件名主干)`）的内嵌歌词 / 封面
/// 导出成外部文件：`dir` 为 `None` 写在音频旁边、与音频同名；`Some` 写进
/// 这个文件夹，用 job 里渲染好的主干（批内重名自动加 ` (2)`）。不碰音频和标签。
pub fn export_files(
    kind: ExportKind,
    dir: Option<&Path>,
    overwrite: bool,
    jobs: &[(String, String)],
) -> ExportSummary {
    let mut summary = ExportSummary::default();
    let mut used = HashSet::new();
    for (audio, folder_stem) in jobs {
        let audio_path = Path::new(audio);
        let name = audio_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| audio.clone());
        let (bytes, ext) = match kind {
            ExportKind::Lyrics { ext, format } => {
                let text = crate::read_lyrics(audio_path)
                    .map(|raw| format_lyrics(&raw, format))
                    .filter(|text| !text.trim().is_empty());
                match text {
                    Some(text) => (text.into_bytes(), ext),
                    None => {
                        summary.no_data += 1;
                        continue;
                    }
                }
            }
            ExportKind::Cover(format) => {
                let Some((data, _)) = crate::read_artwork(audio_path) else {
                    summary.no_data += 1;
                    continue;
                };
                match encode_cover(&data, format) {
                    Ok(encoded) => encoded,
                    Err(e) => {
                        summary.failed.push(format!("{name}：{e}"));
                        continue;
                    }
                }
            }
        };
        let (folder, stem) = match dir {
            Some(dir) => (dir.to_path_buf(), dedupe_stem(&mut used, folder_stem)),
            None => {
                let Some(parent) = audio_path.parent() else {
                    summary.failed.push(format!("{name}：没有所在目录"));
                    continue;
                };
                let stem = audio_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (parent.to_path_buf(), stem)
            }
        };
        match write_export(&folder.join(format!("{stem}.{ext}")), &bytes, overwrite) {
            Ok(true) => summary.written += 1,
            Ok(false) => summary.skipped += 1,
            Err(e) => summary.failed.push(format!("{name}：{e}")),
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 把 `text` 按 `encoding` 编码、再把每个字节当 Latin-1 读出来——模拟标签乱码。
    fn garble(text: &str, encoding: &'static Encoding) -> String {
        encoding
            .encode(text)
            .0
            .iter()
            .map(|b| char::from(*b))
            .collect()
    }

    /// 自动识别：简体 / 繁体 / 日文 / 韩文 / UTF-8 双重编码的乱码都能转回，
    /// 带 ASCII 的混合标题也行；Windows-1252 读法（`–` `‡` 一类）同样。
    #[test]
    fn fix_mojibake_auto_detects_encoding() {
        // 韩文（EUC-KR）的字节区与简体中文（GBK）几乎完全重叠，没有词典分不出来，
        // 并列时优先简体中文——所以这里不测 EUC-KR（见 `fix_mojibake` 的文档）。
        let cases: [(&str, &'static Encoding); 6] = [
            ("周杰伦", encoding_rs::GBK),
            ("周杰伦 - 晴天 (Live)", encoding_rs::GBK),
            ("周杰倫", encoding_rs::BIG5),
            ("すずめの戸締まり", encoding_rs::SHIFT_JIS),
            ("周杰伦", encoding_rs::UTF_8),
            ("歌", encoding_rs::GBK),
        ];
        for (text, encoding) in cases {
            let garbled = garble(text, encoding);
            assert_ne!(garbled, text);
            assert_eq!(
                fix_mojibake(&garbled).as_deref(),
                Some(text),
                "{text} via {}",
                encoding.name()
            );
        }
        // UTF-8 字节按 Windows-1252 读出来（0x96 → `–`、0x87 → `‡`）。
        let cp1252 = encoding_rs::WINDOWS_1252
            .decode("中文".as_bytes())
            .0
            .into_owned();
        assert!(cp1252.chars().any(|c| u32::from(c) > 0xFF));
        assert_eq!(fix_mojibake(&cp1252).as_deref(), Some("中文"));
    }

    /// 尽力而为：正常文本、带重音的西文、解不出来的一律保持原样（`None`）。
    #[test]
    fn fix_mojibake_leaves_normal_text_alone() {
        for text in [
            "plain ascii",
            "已经是中文",
            "Björk",
            "Ações",
            "Ações das Ações",
            "Café au lait",
            "Mötley Crüe",
            "Beyoncé",
            "\u{ff}\u{ff}",
            "",
        ] {
            assert_eq!(fix_mojibake(text), None, "{text:?}");
        }
        // 已经修好的结果再修一次不变。
        let fixed = fix_mojibake(&garble("周杰伦", encoding_rs::GBK)).unwrap();
        assert_eq!(fix_mojibake(&fixed), None);
    }

    #[test]
    fn decode_text_file_detects_encoding() {
        assert_eq!(decode_text_file("héllo 你好".as_bytes()), "héllo 你好");
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice("歌词".as_bytes());
        assert_eq!(decode_text_file(&bom), "歌词");
        let (gbk, _, _) = encoding_rs::GBK.encode("[00:01.00]你好");
        assert_eq!(decode_text_file(&gbk), "[00:01.00]你好");
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("hi 你".encode_utf16().flat_map(|u| u.to_le_bytes()))
            .collect();
        assert_eq!(decode_text_file(&utf16), "hi 你");
    }

    const LRC: &str = "[ti:Song]\r\n[ar:Me]\r\n[00:05.00]second\r\n[00:01.00]first\r\n[00:03.00]\r\n[00:02.00][00:08.00]chorus\r\n\r\n";

    #[test]
    fn format_lyrics_strips_timestamps_and_sorts() {
        let out = format_lyrics(
            LRC,
            LyricsFormat {
                strip_timestamps: true,
                remove_blank_lines: false,
            },
        );
        // 头标签丢掉；副歌按时间展开；空歌词行按时间排进去（保留为空行）。
        assert_eq!(out, "first\nchorus\n\nsecond\nchorus\n");
    }

    #[test]
    fn format_lyrics_remove_blank_lines_keeps_timestamps() {
        let out = format_lyrics(
            LRC,
            LyricsFormat {
                strip_timestamps: false,
                remove_blank_lines: true,
            },
        );
        // 只有时间戳的空歌词行与纯空行都去掉，其余原样（头标签也留着）。
        assert_eq!(
            out,
            "[ti:Song]\n[ar:Me]\n[00:05.00]second\n[00:01.00]first\n[00:02.00][00:08.00]chorus\n"
        );
        let both = format_lyrics(
            LRC,
            LyricsFormat {
                strip_timestamps: true,
                remove_blank_lines: true,
            },
        );
        assert_eq!(both, "first\nchorus\nsecond\nchorus\n");
        // 都不开：只规整换行。
        assert_eq!(format_lyrics("a\r\nb", LyricsFormat::default()), "a\nb\n");
    }

    fn tiny_png() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 10, 10, 128]));
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn encode_cover_passthrough_and_convert() {
        let png = tiny_png();
        // 同格式 / 原格式：字节不动。
        assert_eq!(
            encode_cover(&png, CoverFormat::Original).unwrap(),
            (png.clone(), "png")
        );
        assert_eq!(encode_cover(&png, CoverFormat::Png).unwrap().0, png);
        // 转 JPEG（带透明通道的 PNG 也能转）与 WebP。
        let (jpg, ext) = encode_cover(&png, CoverFormat::Jpeg).unwrap();
        assert_eq!(ext, "jpg");
        assert_eq!(image::guess_format(&jpg).unwrap(), image::ImageFormat::Jpeg);
        let (webp, ext) = encode_cover(&png, CoverFormat::Webp).unwrap();
        assert_eq!(ext, "webp");
        assert_eq!(
            image::guess_format(&webp).unwrap(),
            image::ImageFormat::WebP
        );
        assert!(encode_cover(b"not an image", CoverFormat::Png).is_err());
    }

    #[test]
    fn write_export_respects_overwrite_and_dedupe() {
        let dir = std::env::temp_dir().join(format!("zeo-export-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let target = dir.join("sub").join("a.lrc");
        assert_eq!(write_export(&target, b"one", false), Ok(true));
        assert_eq!(write_export(&target, b"two", false), Ok(false));
        assert_eq!(fs::read(&target).unwrap(), b"one");
        assert_eq!(write_export(&target, b"two", true), Ok(true));
        assert_eq!(fs::read(&target).unwrap(), b"two");
        let _ = fs::remove_dir_all(&dir);

        let mut used = HashSet::new();
        assert_eq!(dedupe_stem(&mut used, "A - B"), "A - B");
        assert_eq!(dedupe_stem(&mut used, "a - b"), "a - b (2)");
        assert_eq!(dedupe_stem(&mut used, "A - B"), "A - B (3)");
    }

    /// 端到端：给语料副本写入歌词与封面，再批量导出——同目录同名 / 文件夹
    /// 模板两种落点、去时间戳、转码、不覆盖 / 覆盖、没有数据的计数。
    #[test]
    fn export_files_writes_lyrics_and_cover() {
        use crate::write::testutil::{corpus, scratch};
        use crate::write::{CoverEdit, Edit, TagEdits, TagField, write_tags};
        let Some(source) = corpus("sine.flac") else {
            return eprintln!("跳过：没有测试语料");
        };
        let audio = scratch(&source);
        let bare = scratch(&corpus("sine.wav").unwrap_or(source.clone()));
        let png = {
            let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]));
            let mut out = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(img)
                .write_to(&mut out, image::ImageFormat::Png)
                .unwrap();
            out.into_inner()
        };
        write_tags(
            &audio,
            &TagEdits {
                fields: vec![(
                    TagField::Lyrics,
                    Edit::Set("[00:01.00]你好\n\n[00:02.00]世界".to_owned()),
                )],
                cover: Some(CoverEdit::Replace {
                    data: png,
                    mime: "image/png".to_owned(),
                }),
                ..TagEdits::default()
            },
        )
        .unwrap();
        let audio_str = audio.to_string_lossy().into_owned();
        let bare_str = bare.to_string_lossy().into_owned();
        let stem = audio.file_stem().unwrap().to_string_lossy().into_owned();
        let kind = ExportKind::Lyrics {
            ext: "txt",
            format: LyricsFormat {
                strip_timestamps: true,
                remove_blank_lines: true,
            },
        };

        // 同目录同名：写在音频旁边。
        let jobs = vec![(audio_str.clone(), String::new())];
        let s = export_files(kind, None, false, &jobs);
        assert_eq!((s.written, s.skipped, s.no_data), (1, 0, 0), "{s:?}");
        let txt = audio.with_extension("txt");
        assert_eq!(fs::read_to_string(&txt).unwrap(), "你好\n世界\n");
        // 一律 UTF-8、不带 BOM。
        assert_eq!(fs::read(&txt).unwrap(), "你好\n世界\n".as_bytes());
        // 已存在：默认跳过，覆盖才重写。
        fs::write(&txt, "old").unwrap();
        let s = export_files(kind, None, false, &jobs);
        assert_eq!((s.written, s.skipped), (0, 1));
        assert_eq!(fs::read_to_string(&txt).unwrap(), "old");
        let s = export_files(kind, None, true, &jobs);
        assert_eq!(s.written, 1);
        assert_eq!(fs::read_to_string(&txt).unwrap(), "你好\n世界\n");

        // 文件夹模式 + 封面转 JPEG；没有封面 / 歌词的计 no_data；批内重名加序号。
        let out = audio.parent().unwrap().join("out");
        let jobs = vec![
            (audio_str.clone(), "Cover".to_owned()),
            (audio_str.clone(), "Cover".to_owned()),
            (bare_str.clone(), "Bare".to_owned()),
        ];
        let s = export_files(
            ExportKind::Cover(CoverFormat::Jpeg),
            Some(&out),
            false,
            &jobs,
        );
        assert_eq!((s.written, s.no_data), (2, 1), "{s:?}");
        assert!(out.join("Cover.jpg").is_file());
        assert!(out.join("Cover (2).jpg").is_file());
        let (jpg, _) = (fs::read(out.join("Cover.jpg")).unwrap(), ());
        assert_eq!(image::guess_format(&jpg).unwrap(), image::ImageFormat::Jpeg);
        assert!(s.describe("封面").contains("1 首没有内嵌封面"));
        let _ = stem;
        let _ = fs::remove_dir_all(audio.parent().unwrap());
        let _ = fs::remove_dir_all(bare.parent().unwrap());
    }

    /// `Edit::Redecode`：歌词 / 注释按完整文本在写回时修复；没乱码的文件解析后
    /// 是空改动（不会白白重写文件）；歌词里的换行与 ASCII 行原样保留。
    #[test]
    fn redecode_fixes_lyrics_and_comment_from_file_text() {
        use crate::write::testutil::{corpus, scratch};
        use crate::write::{Edit, TagEdits, TagField, read_fields, resolve_redecode, write_tags};
        let Some(source) = corpus("sine.flac") else {
            return eprintln!("跳过：没有测试语料");
        };
        let audio = scratch(&source);
        // 「周杰伦」「你好」的 GBK 字节被当成 Latin-1 读出来的样子。
        let garbled_name = garble("周杰伦", encoding_rs::GBK);
        let garbled_hi = garble("你好", encoding_rs::GBK);
        let lyrics = format!("[00:01.00]{garbled_hi}\n[00:02.00]plain ascii\n{garbled_name}");
        write_tags(
            &audio,
            &TagEdits {
                fields: vec![
                    (TagField::Lyrics, Edit::Set(lyrics)),
                    (TagField::Comment, Edit::Set(garbled_name.clone())),
                ],
                ..TagEdits::default()
            },
        )
        .unwrap();
        let redecode = TagEdits {
            fields: vec![
                (TagField::Lyrics, Edit::Redecode),
                (TagField::Comment, Edit::Redecode),
            ],
            ..TagEdits::default()
        };
        // 解析：两项都修得动。
        let mut resolved = redecode.clone();
        resolve_redecode(&audio, &mut resolved).unwrap();
        assert!(matches!(resolved.fields[0].1, Edit::Set(_)));
        write_tags(&audio, &redecode).unwrap();
        let fields = read_fields(&audio).unwrap();
        assert_eq!(
            fields[&TagField::Lyrics],
            "[00:01.00]你好\n[00:02.00]plain ascii\n周杰伦"
        );
        assert_eq!(fields[&TagField::Comment], "周杰伦");
        // 已经修好：再解析全是 Keep、改动为空。
        let mut again = redecode.clone();
        resolve_redecode(&audio, &mut again).unwrap();
        assert!(again.is_empty(), "{again:?}");
        let _ = fs::remove_dir_all(audio.parent().unwrap());
    }
}
