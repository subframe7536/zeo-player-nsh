//! KWM 数据源适配。
//!
//! 接口参考 `xhongc/music-tag-web` (`kuwo.py`)：
//! - 搜索：`https://www.kuwo.cn/api/www/search/searchMusicBykeyWord`（`Hm_token` + `Cross = md5(sha1(token))`）
//! - 歌词：`https://kuwo.cn/newh5/singles/songinfoandlrc`

use std::time::{SystemTime, UNIX_EPOCH};

use md5::Md5;
use serde_json::Value;
use sha1::{Digest, Sha1};
use zeo_types::config::MatchSource;

use crate::model::MatchCandidate;
use crate::provider::{MetadataProvider, encode_uri, extract_year};

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub struct KuwoProvider {
    agent: ureq::Agent,
}

impl KuwoProvider {
    pub fn new(agent: ureq::Agent) -> Self {
        Self { agent }
    }

    fn auth_headers() -> (String, String) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let token = format!("zeo{nanos:029x}");
        let token = &token[..32.min(token.len())];
        let sha1_hex = hex_lower(&Sha1::digest(token.as_bytes()));
        let cross = hex_lower(&Md5::digest(sha1_hex.as_bytes()));
        (format!("Hm_token={token}"), cross)
    }
}

impl MetadataProvider for KuwoProvider {
    fn source(&self) -> MatchSource {
        MatchSource::Kuwo
    }

    fn search(&self, keyword: &str, limit: usize) -> Result<Vec<MatchCandidate>, String> {
        let rn = limit.clamp(1, 20);
        let url = format!(
            "https://www.kuwo.cn/api/www/search/searchMusicBykeyWord?key={}&pn=1&rn={rn}&httpsStatus=1",
            encode_uri(keyword)
        );
        let (cookie, cross) = Self::auth_headers();
        let mut resp = self
            .agent
            .get(&url)
            .header("Referer", "https://www.kuwo.cn/")
            .header(
                "User-Agent",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
            )
            .header("Cross", &cross)
            .header("Cookie", &cookie)
            .call()
            .map_err(|e| format!("KWM 搜索失败: {e}"))?;
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("读取 KWM 响应失败: {e}"))?;
        parse_search_response(&text)
    }

    fn fetch_lyric(&self, candidate: &MatchCandidate) -> Result<Option<String>, String> {
        let url = format!(
            "https://kuwo.cn/newh5/singles/songinfoandlrc?musicId={}&mid={}&type=music&httpsStatus=1&plat=web_www",
            encode_uri(&candidate.source_id),
            encode_uri(&candidate.source_id)
        );
        let (cookie, cross) = Self::auth_headers();
        let mut resp = self
            .agent
            .get(&url)
            .header("Referer", "https://www.kuwo.cn/")
            .header(
                "User-Agent",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
            )
            .header("Cross", &cross)
            .header("Cookie", &cookie)
            .call()
            .map_err(|e| format!("KWM 歌词请求失败: {e}"))?;
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("读取 KWM 歌词响应失败: {e}"))?;
        parse_lyric_response(&text)
    }
}

pub(crate) fn parse_search_response(body: &str) -> Result<Vec<MatchCandidate>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("解析 KWM JSON 失败: {e}"))?;
    let Some(list) = v.pointer("/data/list").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(list.len());
    for item in list {
        let rid = item
            .get("rid")
            .and_then(|v| {
                v.as_u64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_str().map(str::to_owned))
            })
            .unwrap_or_default();
        let title = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if rid.is_empty() || title.is_empty() {
            continue;
        }
        let artists: Vec<String> = item
            .get("artist")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .split(['&', '、', ','])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let album = item
            .get("album")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let cover = item
            .get("albumpic")
            .or_else(|| item.get("pic"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|u| u.replace("http://", "https://"));
        let duration_ms = item
            .get("duration")
            .and_then(Value::as_u64)
            .filter(|&d| d > 0)
            .map(|d| d * 1000);
        let year = item
            .get("releaseDate")
            .and_then(Value::as_str)
            .and_then(extract_year);
        out.push(MatchCandidate {
            source: MatchSource::Kuwo,
            source_id: rid,
            ext_id: None,
            title: title.to_owned(),
            artists,
            album,
            album_artist: None,
            year,
            track_number: None,
            disc_number: None,
            duration_ms,
            cover_thumb_url: cover.clone(),
            cover_original_url: cover,
        });
    }
    Ok(out)
}

pub(crate) fn parse_lyric_response(body: &str) -> Result<Option<String>, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| format!("解析 KWM 歌词 JSON 失败: {e}"))?;
    let Some(lines) = v.pointer("/data/lrclist").and_then(Value::as_array) else {
        return Ok(None);
    };
    let mut out = String::new();
    for line in lines {
        let secs: f64 = line
            .get("time")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        let content = line
            .get("lineLyric")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let total_ms = (secs.max(0.0) * 1000.0).round() as u64;
        let mins = total_ms / 60_000;
        let rem_secs = (total_ms % 60_000) / 1000;
        let centis = (total_ms % 1000) / 10;
        use std::fmt::Write;
        let _ = writeln!(out, "[{mins:02}:{rem_secs:02}.{centis:02}]{content}");
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kuwo_search_and_lyric() {
        let search = r#"{
            "data": {
                "list": [{
                    "rid": 228908,
                    "name": "晴天",
                    "artist": "周杰伦",
                    "album": "叶惠美",
                    "albumpic": "http://img.kuwo.cn/cover.jpg",
                    "duration": 269,
                    "releaseDate": "2003-07-31"
                }]
            }
        }"#;
        let list = parse_search_response(search).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].source_id, "228908");
        assert_eq!(list[0].year.as_deref(), Some("2003"));

        let lyric = r#"{
            "data": {
                "lrclist": [
                    {"time": "12.34", "lineLyric": "故事的小黄花"}
                ]
            }
        }"#;
        let lrc = parse_lyric_response(lyric).unwrap();
        assert_eq!(lrc.as_deref(), Some("[00:12.34]故事的小黄花"));
    }
}
