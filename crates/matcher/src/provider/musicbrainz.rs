//! MusicBrainz + Cover Art Archive 数据源适配（适合欧美 / 日音 / 古典曲库）。
//!
//! 遵循 MusicBrainz API 规范：设置带项目信息的 `User-Agent`，并在进程内维持
//! 至少 1.1 秒的请求间隔（1 req/s 礼貌限流）。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use zeo_types::config::MatchSource;

use crate::model::MatchCandidate;
use crate::provider::{MetadataProvider, encode_uri, extract_year};

const USER_AGENT: &str = "ZeoPlayer/0.1 (https://github.com/subframe7536/zeo-player-nsh)";
static LAST_REQ: Mutex<Option<Instant>> = Mutex::new(None);

pub struct MusicBrainzProvider {
    agent: ureq::Agent,
}

impl MusicBrainzProvider {
    pub fn new(agent: ureq::Agent) -> Self {
        Self { agent }
    }

    fn wait_rate_limit() {
        let mut guard = LAST_REQ.lock().unwrap_or_else(|e| e.into_inner());
        let min_gap = Duration::from_millis(1100);
        if let Some(last) = *guard {
            let elapsed = last.elapsed();
            if elapsed < min_gap {
                std::thread::sleep(min_gap - elapsed);
            }
        }
        *guard = Some(Instant::now());
    }
}

impl MetadataProvider for MusicBrainzProvider {
    fn source(&self) -> MatchSource {
        MatchSource::MusicBrainz
    }

    fn search(&self, keyword: &str, limit: usize) -> Result<Vec<MatchCandidate>, String> {
        Self::wait_rate_limit();
        let lim = limit.clamp(1, 20);
        let url = format!(
            "https://musicbrainz.org/ws/2/recording/?query={}&fmt=json&limit={lim}",
            encode_uri(keyword)
        );
        let mut resp = self
            .agent
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .call()
            .map_err(|e| format!("MusicBrainz 搜索失败: {e}"))?;
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("读取 MusicBrainz 响应失败: {e}"))?;
        parse_search_response(&text)
    }

    fn fetch_lyric(&self, _candidate: &MatchCandidate) -> Result<Option<String>, String> {
        // MusicBrainz 不提供歌词服务。
        Ok(None)
    }
}

pub(crate) fn parse_search_response(body: &str) -> Result<Vec<MatchCandidate>, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| format!("解析 MusicBrainz JSON 失败: {e}"))?;
    let Some(list) = v.get("recordings").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(list.len());
    for rec in list {
        let id = rec
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let title = rec
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if id.is_empty() || title.is_empty() {
            continue;
        }
        let artists: Vec<String> = rec
            .get("artist-credit")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        c.get("name")
                            .or_else(|| c.pointer("/artist/name"))
                            .and_then(Value::as_str)
                    })
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let release = rec
            .get("releases")
            .and_then(Value::as_array)
            .and_then(|arr| arr.first());
        let album = release
            .and_then(|r| r.get("title"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let release_id = release
            .and_then(|r| r.get("id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let year = rec
            .get("first-release-date")
            .or_else(|| release.and_then(|r| r.get("date")))
            .and_then(Value::as_str)
            .and_then(extract_year);
        let duration_ms = rec.get("length").and_then(Value::as_u64).filter(|&d| d > 0);
        let media = release
            .and_then(|r| r.get("media"))
            .and_then(Value::as_array)
            .and_then(|arr| arr.first());
        let disc_number = media
            .and_then(|m| m.get("position"))
            .and_then(Value::as_u64)
            .filter(|&n| n > 0)
            .map(|n| n as u32);
        let track_number = media
            .and_then(|m| m.get("track"))
            .and_then(Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|t| t.get("number"))
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|&n| n > 0);
        out.push(MatchCandidate {
            source: MatchSource::MusicBrainz,
            source_id: id.to_owned(),
            ext_id: release_id.map(str::to_owned),
            title: title.to_owned(),
            artists,
            album,
            album_artist: None,
            year,
            track_number,
            disc_number,
            duration_ms,
            cover_thumb_url: release_id
                .map(|rid| format!("https://coverartarchive.org/release/{rid}/front-250")),
            cover_original_url: release_id
                .map(|rid| format!("https://coverartarchive.org/release/{rid}/front-500")),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_musicbrainz_fixture() {
        let fixture = r#"{
            "recordings": [{
                "id": "rec-1",
                "title": "Bohemian Rhapsody",
                "length": 354000,
                "first-release-date": "1975-10-31",
                "artist-credit": [{"name": "Queen"}],
                "releases": [{
                    "id": "rel-1",
                    "title": "A Night at the Opera",
                    "date": "1975-11-21",
                    "media": [{"position": 1, "track": [{"number": "11"}]}]
                }]
            }]
        }"#;
        let list = parse_search_response(fixture).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Bohemian Rhapsody");
        assert_eq!(list[0].artists, vec!["Queen"]);
        assert_eq!(list[0].track_number, Some(11));
        assert_eq!(list[0].year.as_deref(), Some("1975"));
    }
}
