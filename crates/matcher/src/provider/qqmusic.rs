//! QQ 音乐数据源适配。
//!
//! 接口参考 `xhongc/music-tag-web` (`qm.py`) 与 `SPlayer-Dev/SPlayer-Next` (`qqmusic`)：
//! - 搜索：`POST https://u.y.qq.com/cgi-bin/musicu.fcg`（`DoSearchForQQMusicDesktop`）
//! - 歌词：`GET https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg`（Base64 解码）
//! - 封面：`https://y.gtimg.cn/music/photo_new/T002R{size}x{size}M000{albumMid}.jpg`

use base64::Engine;
use serde_json::{Value, json};
use zeo_types::config::MatchSource;

use crate::model::MatchCandidate;
use crate::provider::{MetadataProvider, encode_uri, extract_year};

pub struct QQMusicProvider {
    agent: ureq::Agent,
}

impl QQMusicProvider {
    pub fn new(agent: ureq::Agent) -> Self {
        Self { agent }
    }

    fn cover_url(album_mid: &str, size: u32) -> String {
        format!("https://y.gtimg.cn/music/photo_new/T002R{size}x{size}M000{album_mid}.jpg")
    }
}

impl MetadataProvider for QQMusicProvider {
    fn source(&self) -> MatchSource {
        MatchSource::QQMusic
    }

    fn search(&self, keyword: &str, limit: usize) -> Result<Vec<MatchCandidate>, String> {
        let num = limit.clamp(1, 30);
        let payload = json!({
            "comm": {
                "ct": "6",
                "cv": "80600",
                "gzip": "0",
                "nettype": "2",
                "tmeAppID": "qqmusic",
                "tmeLoginType": "2"
            },
            "music.search.SearchCgiService.DoSearchForQQMusicDesktop": {
                "module": "music.search.SearchCgiService",
                "method": "DoSearchForQQMusicDesktop",
                "param": {
                    "num_per_page": num,
                    "page_num": 1,
                    "remoteplace": "txt.mac.search",
                    "search_type": 0,
                    "query": keyword,
                    "grp": 1,
                    "nqc_flag": 0
                }
            }
        });
        let mut resp = self
            .agent
            .post("https://u.y.qq.com/cgi-bin/musicu.fcg")
            .header("Referer", "https://y.qq.com/portal/profile.html")
            .header("Content-Type", "application/json;charset=utf-8")
            .header(
                "User-Agent",
                "QQ%E9%9F%B3%E4%B9%90/73222 CFNetwork/1406.0.3 Darwin/22.4.0",
            )
            .send(payload.to_string())
            .map_err(|e| format!("QQ 音乐搜索失败: {e}"))?;
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("读取 QQ 音乐响应失败: {e}"))?;
        parse_search_response(&text)
    }

    fn fetch_lyric(&self, candidate: &MatchCandidate) -> Result<Option<String>, String> {
        let url = format!(
            "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg?g_tk=5381&format=json&inCharset=utf-8&outCharset=utf-8&notice=0&platform=h5&needNewCode=1&ct=121&cv=0&songmid={}",
            encode_uri(&candidate.source_id)
        );
        let mut resp = self
            .agent
            .get(&url)
            .header("Referer", "https://y.qq.com/")
            .header(
                "User-Agent",
                "QQ音乐/73222 CFNetwork/1406.0.2 Darwin/22.4.0",
            )
            .call()
            .map_err(|e| format!("QQ 音乐歌词请求失败: {e}"))?;
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("读取 QQ 音乐歌词响应失败: {e}"))?;
        parse_lyric_response(&text)
    }
}

pub(crate) fn parse_search_response(body: &str) -> Result<Vec<MatchCandidate>, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| format!("解析 QQ 音乐 JSON 失败: {e}"))?;
    let Some(list) = v
        .pointer("/music.search.SearchCgiService.DoSearchForQQMusicDesktop/data/body/song/list")
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(list.len());
    for item in list {
        let mid = item
            .get("mid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let title = item
            .get("title")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if mid.is_empty() || title.is_empty() {
            continue;
        }
        let ext_id = item.get("id").and_then(|v| {
            v.as_u64()
                .map(|n| n.to_string())
                .or_else(|| v.as_str().map(str::to_owned))
        });
        let artists: Vec<String> = item
            .get("singer")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.get("name").and_then(Value::as_str))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let album_obj = item.get("album");
        let album = album_obj
            .and_then(|a| a.get("title").or_else(|| a.get("name")))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let album_mid = album_obj
            .and_then(|a| a.get("mid"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let year = item
            .get("time_public")
            .and_then(Value::as_str)
            .and_then(extract_year);
        let duration_ms = item
            .get("interval")
            .and_then(Value::as_u64)
            .filter(|&s| s > 0)
            .map(|s| s * 1000);
        let track_number = item
            .get("index_album")
            .and_then(Value::as_u64)
            .filter(|&n| n > 0)
            .map(|n| n as u32);
        let disc_number = item
            .get("index_cd")
            .and_then(Value::as_u64)
            .map(|n| (n + 1) as u32);
        out.push(MatchCandidate {
            source: MatchSource::QQMusic,
            source_id: mid.to_owned(),
            ext_id,
            title: title.to_owned(),
            artists,
            album,
            album_artist: None,
            year,
            track_number,
            disc_number,
            duration_ms,
            cover_thumb_url: album_mid.map(|m| QQMusicProvider::cover_url(m, 300)),
            cover_original_url: album_mid.map(|m| QQMusicProvider::cover_url(m, 800)),
        });
    }
    Ok(out)
}

pub(crate) fn parse_lyric_response(body: &str) -> Result<Option<String>, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| format!("解析 QQ 音乐歌词 JSON 失败: {e}"))?;
    let Some(encoded) = v.get("lyric").and_then(Value::as_str) else {
        return Ok(None);
    };
    if encoded.trim().is_empty() {
        return Ok(None);
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|e| format!("Base64 解码 QQ 音乐歌词失败: {e}"))?;
    let text = String::from_utf8_lossy(&decoded).trim().to_owned();
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_qqmusic_search_fixture() {
        let fixture = r#"{
            "music.search.SearchCgiService.DoSearchForQQMusicDesktop": {
                "data": {
                    "body": {
                        "song": {
                            "list": [{
                                "id": 97773,
                                "mid": "0039MnYb0qxYhV",
                                "title": "晴天",
                                "singer": [{"name": "周杰伦"}],
                                "album": {"mid": "000MkMni19ClKG", "title": "叶惠美"},
                                "time_public": "2003-07-31",
                                "interval": 269,
                                "index_album": 3,
                                "index_cd": 0
                            }]
                        }
                    }
                }
            }
        }"#;
        let list = parse_search_response(fixture).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "晴天");
        assert_eq!(list[0].artists, vec!["周杰伦"]);
        assert_eq!(list[0].album.as_deref(), Some("叶惠美"));
        assert_eq!(list[0].year.as_deref(), Some("2003"));
        assert_eq!(list[0].duration_ms, Some(269_000));
        assert_eq!(list[0].track_number, Some(3));
        assert!(
            list[0]
                .cover_original_url
                .as_deref()
                .unwrap()
                .contains("T002R800x800M000000MkMni19ClKG")
        );
    }

    #[test]
    fn parses_qqmusic_lyric_base64() {
        let b64 = base64::engine::general_purpose::STANDARD.encode("[00:00.00]晴天");
        let fixture = format!(r#"{{"lyric":"{b64}"}}"#);
        let lrc = parse_lyric_response(&fixture).unwrap();
        assert_eq!(lrc.as_deref(), Some("[00:00.00]晴天"));
    }
}
