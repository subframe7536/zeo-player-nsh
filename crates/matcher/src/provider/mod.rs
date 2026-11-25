//! 音乐平台数据源抽象与通用 HTTP / 封面下载工具。

pub mod kugou;
pub mod kuwo;
pub mod musicbrainz;
pub mod netease;
pub mod qqmusic;

use std::time::Duration;

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use zeo_types::config::MatchSource;

use crate::model::MatchCandidate;

/// 统一的数据源接口：搜索曲目与按平台 ID 拉取歌词。
pub trait MetadataProvider: Send + Sync {
    fn source(&self) -> MatchSource;

    /// 按关键词搜索候选曲目（最多返回 `limit` 条）。
    fn search(&self, keyword: &str, limit: usize) -> Result<Vec<MatchCandidate>, String>;

    /// 根据候选的 `source_id` / `ext_id` 拉取 LRC 歌词（无歌词返回 `Ok(None)`）。
    fn fetch_lyric(&self, candidate: &MatchCandidate) -> Result<Option<String>, String>;
}

/// 构造统一超时策略的 `ureq::Agent`（连接 5s，总超时 10s）。
pub fn build_agent() -> ureq::Agent {
    let cfg = ureq::config::Config::builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .build();
    ureq::Agent::new_with_config(cfg)
}

/// URL query 编码。
pub fn encode_uri(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

/// 去除平台搜索高亮标签（`<em>...</em>`）。
pub fn strip_em(s: &str) -> String {
    s.replace("<em>", "").replace("</em>", "")
}

/// 从 `YYYY-MM-DD` 或时间戳提取 4 位年份。
pub fn extract_year(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() >= 4 && trimmed.as_bytes()[..4].iter().all(u8::is_ascii_digit) {
        let y = &trimmed[..4];
        if y != "0000" {
            return Some(y.to_owned());
        }
    }
    None
}

/// 从 Unix 毫秒时间戳估算年份（不引入额外日期库）。
pub fn year_from_unix_ms(ms: i64) -> Option<String> {
    if ms <= 0 {
        return None;
    }
    // 1970-01-01 起按公历闰年精确推进年份。
    let mut days = ms / 86_400_000;
    let mut year: i64 = 1970;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let year_days = if leap { 366 } else { 365 };
        if days < year_days {
            break;
        }
        days -= year_days;
        year += 1;
    }
    if (1900..=2100).contains(&year) {
        Some(year.to_string())
    } else {
        None
    }
}

/// 根据图片字节魔数判断 MIME 类型（只接受常见封面格式）。
pub fn detect_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else {
        None
    }
}

/// MIME 类型 → 临时文件扩展名。
pub fn mime_extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "jpg",
    }
}

/// 下载在线封面并返回 `(原始字节, MIME 类型)`，上限 10 MB。
pub fn download_cover_bytes(agent: &ureq::Agent, url: &str) -> Result<(Vec<u8>, String), String> {
    let url = url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("无效的封面 URL".to_owned());
    }
    let mut resp = agent
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
        )
        .call()
        .map_err(|e| format!("下载封面失败: {e}"))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("下载封面失败: HTTP {status}"));
    }
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(10 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| format!("读取封面数据失败: {e}"))?;
    let mime =
        detect_image_mime(&bytes).ok_or_else(|| "下载的封面不是有效的图片格式".to_owned())?;
    Ok((bytes, mime.to_owned()))
}
