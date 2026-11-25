//! 网易云音乐数据源适配。
//!
//! 使用 `linuxapi` 转发协议（AES-128-ECB + 固定密钥 `rFgB&h#%2?^eDg:Q`，
//! 无需大整数 RSA 依赖），参考 `xhongc/music-tag-web` (`encrypt.py` / `music_resource.py`)
//! 与 `SPlayer-Dev/SPlayer-Next` (`netease`)。

use aes::Aes128;
use aes::cipher::{BlockCipherEncrypt, KeyInit};
use serde_json::{Value, json};
use zeo_types::config::MatchSource;

use crate::model::MatchCandidate;
use crate::provider::{MetadataProvider, year_from_unix_ms};

const LINUX_KEY: &[u8; 16] = b"rFgB&h#%2?^eDg:Q";
const FORWARD_URL: &str = "https://music.163.com/api/linux/forward";

pub struct NetEaseProvider {
    agent: ureq::Agent,
}

impl NetEaseProvider {
    pub fn new(agent: ureq::Agent) -> Self {
        Self { agent }
    }

    fn post_linux_api(&self, target_url: &str, params: Value) -> Result<String, String> {
        let payload = json!({
            "method": "POST",
            "url": target_url,
            "params": params,
        });
        let eparams = linux_encrypt(&payload.to_string());
        let body = format!("eparams={eparams}");
        let mut resp = self
            .agent
            .post(FORWARD_URL)
            .header("Referer", "https://music.163.com/")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header(
                "User-Agent",
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .send(body)
            .map_err(|e| format!("网易云音乐请求失败: {e}"))?;
        resp.body_mut()
            .read_to_string()
            .map_err(|e| format!("读取网易云音乐响应失败: {e}"))
    }

    fn sized_cover(url: &str, size: u32) -> String {
        let url = url.replace("http://", "https://");
        if url.contains("?param=") {
            url
        } else {
            format!("{url}?param={size}y{size}")
        }
    }
}

/// AES-128-ECB + PKCS7 填充 → 大写十六进制（网易云 `linuxapi` 协议）。
pub(crate) fn linux_encrypt(plain: &str) -> String {
    let bytes = plain.as_bytes();
    let pad = 16 - (bytes.len() % 16);
    let mut buf = Vec::with_capacity(bytes.len() + pad);
    buf.extend_from_slice(bytes);
    buf.resize(bytes.len() + pad, pad as u8);

    let cipher = Aes128::new(LINUX_KEY.into());
    for chunk in buf.chunks_exact_mut(16) {
        let block: &mut [u8; 16] = chunk.try_into().unwrap();
        cipher.encrypt_block(block.into());
    }
    let mut hex = String::with_capacity(buf.len() * 2);
    for b in buf {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02X}");
    }
    hex
}

impl MetadataProvider for NetEaseProvider {
    fn source(&self) -> MatchSource {
        MatchSource::NetEase
    }

    fn search(&self, keyword: &str, limit: usize) -> Result<Vec<MatchCandidate>, String> {
        let text = self.post_linux_api(
            "https://music.163.com/api/cloudsearch/pc",
            json!({
                "s": keyword,
                "type": 1,
                "limit": limit.clamp(1, 30),
                "offset": 0,
                "total": true
            }),
        )?;
        parse_search_response(&text)
    }

    fn fetch_lyric(&self, candidate: &MatchCandidate) -> Result<Option<String>, String> {
        let id: i64 = candidate
            .source_id
            .parse()
            .map_err(|_| "无效的网易云歌曲 ID".to_owned())?;
        let text = self.post_linux_api(
            "https://music.163.com/api/song/lyric?lv=-1&kv=-1&tv=-1",
            json!({ "id": id }),
        )?;
        parse_lyric_response(&text)
    }
}

pub(crate) fn parse_search_response(body: &str) -> Result<Vec<MatchCandidate>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("解析网易云 JSON 失败: {e}"))?;
    let Some(songs) = v.pointer("/result/songs").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(songs.len());
    for song in songs {
        let Some(id) = song.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let title = song
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if title.is_empty() {
            continue;
        }
        let artists: Vec<String> = song
            .get("ar")
            .or_else(|| song.get("artists"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a.get("name").and_then(Value::as_str))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let album_obj = song.get("al").or_else(|| song.get("album"));
        let album = album_obj
            .and_then(|a| a.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let pic_url = album_obj
            .and_then(|a| a.get("picUrl"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let year = song
            .get("publishTime")
            .and_then(Value::as_i64)
            .and_then(year_from_unix_ms);
        let duration_ms = song
            .get("dt")
            .or_else(|| song.get("duration"))
            .and_then(Value::as_u64)
            .filter(|&d| d > 0);
        let track_number = song
            .get("no")
            .and_then(Value::as_u64)
            .filter(|&n| n > 0)
            .map(|n| n as u32);
        let disc_number = song
            .get("cd")
            .and_then(Value::as_str)
            .and_then(|s| s.trim().parse::<u32>().ok())
            .filter(|&n| n > 0);
        out.push(MatchCandidate {
            source: MatchSource::NetEase,
            source_id: id.to_string(),
            ext_id: None,
            title: title.to_owned(),
            artists,
            album,
            album_artist: None,
            year,
            track_number,
            disc_number,
            duration_ms,
            cover_thumb_url: pic_url.map(|u| NetEaseProvider::sized_cover(u, 300)),
            cover_original_url: pic_url.map(|u| NetEaseProvider::sized_cover(u, 1024)),
        });
    }
    Ok(out)
}

pub(crate) fn parse_lyric_response(body: &str) -> Result<Option<String>, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| format!("解析网易云歌词 JSON 失败: {e}"))?;
    let lrc = v
        .pointer("/lrc/lyric")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    Ok(lrc.map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_netease_search_fixture() {
        let fixture = r#"{
            "result": {
                "songs": [{
                    "id": 186016,
                    "name": "晴天",
                    "ar": [{"id": 6452, "name": "周杰伦"}],
                    "al": {"id": 18905, "name": "叶惠美", "picUrl": "http://p1.music.126.net/test.jpg"},
                    "dt": 269000,
                    "publishTime": 1059580800000,
                    "no": 3,
                    "cd": "01"
                }]
            }
        }"#;
        let list = parse_search_response(fixture).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].source_id, "186016");
        assert_eq!(list[0].year.as_deref(), Some("2003"));
        assert_eq!(list[0].track_number, Some(3));
        assert_eq!(list[0].disc_number, Some(1));
        assert_eq!(
            list[0].cover_original_url.as_deref(),
            Some("https://p1.music.126.net/test.jpg?param=1024y1024")
        );
    }

    #[test]
    fn linux_encrypt_produces_valid_hex_blocks() {
        let enc = linux_encrypt(r#"{"method":"POST"}"#);
        assert_eq!(enc.len() % 32, 0);
        assert!(enc.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
