//! KGM 数据源适配。
//!
//! 接口参考 `xhongc/music-tag-web` (`kugou.py`) 与 `SPlayer-Dev/SPlayer-Next` (`kugou/search.ts`)：
//! - 搜索：`https://complexsearch.kugou.com/v2/search/song`（MD5 签名）
//! - 歌词：`http://m.kugou.com/app/i/krc.php?cmd=100&timelength=999999&hash={hash}`

use std::time::{SystemTime, UNIX_EPOCH};

use md5::{Digest, Md5};
use serde_json::Value;
use zeo_types::config::MatchSource;

use crate::model::MatchCandidate;
use crate::provider::{MetadataProvider, encode_uri, extract_year, strip_em};

const KUGOU_SIGN_SALT: &str = "NVPh5oo715z5DIWAeQlhMDsWXXQV4hwt";

pub struct KugouProvider {
    agent: ureq::Agent,
}

impl KugouProvider {
    pub fn new(agent: ureq::Agent) -> Self {
        Self { agent }
    }
}

pub(crate) fn sign_search(millis: &str, keyword: &str, pagesize: usize) -> String {
    let raw = format!(
        "{KUGOU_SIGN_SALT}bitrate=0clienttime={millis}clientver=2000dfid=-inputtype=0iscorrection=1isfuzzy=0keyword={keyword}mid={millis}page=1pagesize={pagesize}platform=WebFilterprivilege_filter=0srcappid=2919tag=emuserid=-1uuid={millis}{KUGOU_SIGN_SALT}"
    );
    let digest = Md5::digest(raw.as_bytes());
    let mut hex = String::with_capacity(32);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02X}");
    }
    hex
}

impl MetadataProvider for KugouProvider {
    fn source(&self) -> MatchSource {
        MatchSource::Kugou
    }

    fn search(&self, keyword: &str, limit: usize) -> Result<Vec<MatchCandidate>, String> {
        let pagesize = limit.clamp(1, 20);
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
            .to_string();
        let signature = sign_search(&millis, keyword, pagesize);
        let url = format!(
            "https://complexsearch.kugou.com/v2/search/song?keyword={}&page=1&pagesize={pagesize}&bitrate=0&isfuzzy=0&tag=em&inputtype=0&platform=WebFilter&userid=-1&clientver=2000&iscorrection=1&privilege_filter=0&srcappid=2919&clienttime={millis}&mid={millis}&uuid={millis}&dfid=-&signature={signature}",
            encode_uri(keyword)
        );
        let mut resp = self
            .agent
            .get(&url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
            )
            .call()
            .map_err(|e| format!("KGM 搜索失败: {e}"))?;
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("读取 KGM 响应失败: {e}"))?;
        parse_search_response(&text)
    }

    fn fetch_lyric(&self, candidate: &MatchCandidate) -> Result<Option<String>, String> {
        let url = format!(
            "https://m.kugou.com/app/i/krc.php?cmd=100&timelength=999999&hash={}",
            encode_uri(&candidate.source_id)
        );
        let mut resp = self
            .agent
            .get(&url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
            )
            .call()
            .map_err(|e| format!("KGM 歌词请求失败: {e}"))?;
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("读取 KGM 歌词响应失败: {e}"))?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed.to_owned()))
        }
    }
}

pub(crate) fn parse_search_response(body: &str) -> Result<Vec<MatchCandidate>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("解析 KGM JSON 失败: {e}"))?;
    let Some(list) = v.pointer("/data/lists").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(list.len());
    for item in list {
        let hash = item
            .get("FileHash")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let title = strip_em(
            item.get("SongName")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .trim()
        .to_owned();
        if hash.is_empty() || title.is_empty() {
            continue;
        }
        let singer_raw = strip_em(
            item.get("SingerName")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        let artists: Vec<String> = singer_raw
            .split(['、', ',', '/'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let album = item
            .get("AlbumName")
            .and_then(Value::as_str)
            .map(strip_em)
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        let img = item
            .get("Image")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let cover_thumb_url =
            img.map(|u| u.replace("{size}", "240").replace("http://", "https://"));
        let cover_original_url =
            img.map(|u| u.replace("{size}", "480").replace("http://", "https://"));
        let duration_ms = item
            .get("Duration")
            .and_then(Value::as_u64)
            .filter(|&d| d > 0)
            .map(|d| d * 1000);
        let year = item
            .get("PublishTime")
            .and_then(Value::as_str)
            .and_then(extract_year);
        out.push(MatchCandidate {
            source: MatchSource::Kugou,
            source_id: hash.to_owned(),
            ext_id: item.get("AlbumID").and_then(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            }),
            title,
            artists,
            album,
            album_artist: None,
            year,
            track_number: None,
            disc_number: None,
            duration_ms,
            cover_thumb_url,
            cover_original_url,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kugou_search_fixture() {
        let fixture = r#"{
            "data": {
                "lists": [{
                    "FileHash": "ABCDEF123456",
                    "SongName": "<em>晴天</em>",
                    "SingerName": "<em>周杰伦</em>",
                    "AlbumName": "叶惠美",
                    "AlbumID": "12345",
                    "Image": "http://imge.kugou.com/stdmusic/{size}/test.jpg",
                    "Duration": 269,
                    "PublishTime": "2003-07-31"
                }]
            }
        }"#;
        let list = parse_search_response(fixture).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "晴天");
        assert_eq!(list[0].artists, vec!["周杰伦"]);
        assert_eq!(list[0].duration_ms, Some(269_000));
        assert_eq!(
            list[0].cover_thumb_url.as_deref(),
            Some("https://imge.kugou.com/stdmusic/240/test.jpg")
        );
    }
}
