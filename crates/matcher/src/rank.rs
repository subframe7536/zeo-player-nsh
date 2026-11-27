//! 查询词构建、文件名回退解析、文本归一化（含轻量简繁统一）与多维候选打分。
//!
//! 打分规则融合了 SPlayer-Next `tagMatch.ts`（时长强校验 + `duration_far` 沉底）
//! 与 music-tag-web `utils.py`（简繁归并 + 歌手不符惩罚 + 严格/宽松阈值）。

use std::path::Path;

use zeo_types::config::MatchStrictness;

use crate::model::{MatchCandidate, MatchQuery, MatchedTags, RankedCandidate};

/// 与本地时长差超过 20 秒视为明显不同版本（如 Live / Remix / 伴奏）。
pub const DURATION_FAR_MS: i64 = 20_000;
/// 时长差在 5 秒内视为一致并加分。
pub const DURATION_CLOSE_MS: i64 = 5_000;

/// 常见繁体字 → 简体字轻量映射表（仅用于比对归一化，不改动写回的原始元数据）。
const TRAD_TO_SIMP: &[(char, char)] = &[
    ('愛', '爱'),
    ('夢', '梦'),
    ('風', '风'),
    ('雲', '云'),
    ('雨', '雨'),
    ('電', '电'),
    ('車', '车'),
    ('書', '书'),
    ('畫', '画'),
    ('樂', '乐'),
    ('歌', '歌'),
    ('聲', '声'),
    ('聽', '听'),
    ('說', '说'),
    ('話', '话'),
    ('語', '语'),
    ('詞', '词'),
    ('詩', '诗'),
    ('時', '时'),
    ('間', '间'),
    ('開', '开'),
    ('關', '关'),
    ('門', '门'),
    ('長', '长'),
    ('東', '东'),
    ('無', '无'),
    ('為', '为'),
    ('與', '与'),
    ('這', '这'),
    ('個', '个'),
    ('們', '们'),
    ('來', '来'),
    ('過', '过'),
    ('還', '还'),
    ('進', '进'),
    ('後', '后'),
    ('發', '发'),
    ('現', '现'),
    ('見', '见'),
    ('覺', '觉'),
    ('親', '亲'),
    ('戀', '恋'),
    ('傷', '伤'),
    ('淚', '泪'),
    ('離', '离'),
    ('別', '别'),
    ('飛', '飞'),
    ('鳥', '鸟'),
    ('魚', '鱼'),
    ('龍', '龙'),
    ('華', '华'),
    ('國', '国'),
    ('際', '际'),
    ('紅', '红'),
    ('藍', '蓝'),
    ('綠', '绿'),
    ('黃', '黄'),
    ('黑', '黑'),
    ('銀', '银'),
    ('鐵', '铁'),
    ('鋼', '钢'),
    ('鐘', '钟'),
    ('鏡', '镜'),
    ('歲', '岁'),
    ('舊', '旧'),
    ('歡', '欢'),
    ('嘆', '叹'),
    ('願', '愿'),
    ('記', '记'),
    ('憶', '忆'),
    ('總', '总'),
    ('結', '结'),
    ('終', '终'),
    ('絕', '绝'),
    ('線', '线'),
    ('紙', '纸'),
    ('經', '经'),
    ('給', '给'),
    ('續', '续'),
    ('誰', '谁'),
    ('請', '请'),
    ('讓', '让'),
    ('變', '变'),
    ('動', '动'),
    ('點', '点'),
    ('頭', '头'),
    ('臉', '脸'),
    ('眼', '眼'),
    ('雙', '双'),
    ('隻', '只'),
    ('對', '对'),
    ('錯', '错'),
    ('難', '难'),
    ('離', '离'),
    ('遠', '远'),
    ('邊', '边'),
    ('當', '当'),
    ('從', '从'),
    ('萬', '万'),
    ('億', '亿'),
    ('裡', '里'),
    ('裏', '里'),
    ('麼', '么'),
    ('嗎', '吗'),
    ('樣', '样'),
    ('機', '机'),
    ('會', '会'),
    ('學', '学'),
    ('實', '实'),
    ('專', '专'),
    ('輯', '辑'),
    ('錄', '录'),
    ('獨', '独'),
    ('唱', '唱'),
    ('張', '张'),
    ('陳', '陈'),
    ('楊', '杨'),
    ('趙', '赵'),
    ('鄧', '邓'),
    ('許', '许'),
    ('謝', '谢'),
    ('葉', '叶'),
    ('蘇', '苏'),
    ('盧', '卢'),
    ('劉', '刘'),
    ('孫', '孙'),
    ('馬', '马'),
    ('羅', '罗'),
    ('梁', '梁'),
    ('鄭', '郑'),
    ('傑', '杰'),
    ('倫', '伦'),
    ('偉', '伟'),
    ('榮', '荣'),
    ('華', '华'),
    ('麗', '丽'),
    ('韻', '韵'),
    ('靜', '静'),
    ('潔', '洁'),
    ('軒', '轩'),
];

fn trad_to_simp(c: char) -> char {
    for &(trad, simp) in TRAD_TO_SIMP {
        if c == trad {
            return simp;
        }
    }
    c
}

/// 归一化用于比对的文本：小写化、去除标点与空白、简繁统一。
pub fn normalize_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_whitespace() || is_ignored_punct(ch) {
            continue;
        }
        for lc in ch.to_lowercase() {
            out.push(trad_to_simp(lc));
        }
    }
    out
}

fn is_ignored_punct(c: char) -> bool {
    matches!(
        c,
        '、' | '&'
            | ';'
            | '，'
            | ','
            | '/'
            | '|'
            | '('
            | ')'
            | '（'
            | '）'
            | '['
            | ']'
            | '【'
            | '】'
            | '·'
            | '・'
            | '-'
            | '_'
            | '\''
            | '"'
            | '`'
            | '~'
            | '!'
            | '?'
            | '？'
            | '！'
            | '.'
            | '。'
            | '《'
            | '》'
            | ':'
            | '：'
    )
}

fn both_contains(left: &str, right: &str) -> bool {
    !left.is_empty() && !right.is_empty() && (left.contains(right) || right.contains(left))
}

/// 从文件名主干解析回退的 `(title, artist)`。
///
/// 常见格式：`01. 周杰伦 - 晴天.flac` → `("晴天", "周杰伦")`，
/// `02 - 稻香.mp3` → `("稻香", "")`。
pub fn parse_filename_fallback(file_name: &str) -> (String, String) {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name)
        .trim();
    let cleaned = strip_leading_track_num(stem).trim();
    if let Some((left, right)) = cleaned.split_once(" - ") {
        let l = left.trim();
        let r = right.trim();
        if !l.is_empty() && !r.is_empty() {
            return (r.to_owned(), l.to_owned());
        }
    }
    (cleaned.to_owned(), String::new())
}

fn strip_leading_track_num(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i > 3 {
        return s;
    }
    let rest = &s[i..];
    for prefix in [". ", ".", " - ", "-", "、", " "] {
        if let Some(after) = rest.strip_prefix(prefix) {
            return after;
        }
    }
    s
}

/// 根据曲目现有标签与文件名构建 [`MatchQuery`]。
pub fn build_query(
    file_name: &str,
    title: &str,
    artist: &str,
    album: &str,
    duration_ms: Option<u64>,
) -> MatchQuery {
    let (fb_title, fb_artist) = parse_filename_fallback(file_name);
    let title = if title.trim().is_empty() {
        fb_title
    } else {
        title.trim().to_owned()
    };
    let artist = if artist.trim().is_empty() {
        fb_artist
    } else {
        artist.trim().to_owned()
    };
    let keyword = match (title.is_empty(), artist.is_empty()) {
        (false, false) => format!("{title} {artist}"),
        (false, true) => title.clone(),
        (true, false) => artist.clone(),
        (true, true) => file_name.trim().to_owned(),
    };
    MatchQuery {
        keyword,
        title,
        artist,
        album: album.trim().to_owned(),
        duration_ms,
    }
}

/// 给候选列表打分并降序排列；`duration_far` 候选沉底。
pub fn rank_candidates(
    candidates: Vec<MatchCandidate>,
    query: &MatchQuery,
) -> Vec<RankedCandidate> {
    let ctx_title = normalize_text(&query.title);
    let ctx_artist = normalize_text(&query.artist);
    let ctx_album = normalize_text(&query.album);

    let mut ranked: Vec<RankedCandidate> = candidates
        .into_iter()
        .map(|candidate| {
            let cand_title = normalize_text(&candidate.title);
            let cand_artists_norm: Vec<String> = candidate
                .artists
                .iter()
                .map(|a| normalize_text(a))
                .filter(|a| !a.is_empty())
                .collect();
            let cand_artist_joined = cand_artists_norm.join("");
            let cand_album = candidate
                .album
                .as_deref()
                .map(normalize_text)
                .unwrap_or_default();

            let duration_diff_ms = match (candidate.duration_ms, query.duration_ms) {
                (Some(cand), Some(local)) if cand > 0 && local > 0 => {
                    Some(cand as i64 - local as i64)
                }
                _ => None,
            };
            let abs_diff = duration_diff_ms.map(i64::abs);
            let duration_far = abs_diff.is_some_and(|d| d > DURATION_FAR_MS);
            let duration_close = abs_diff.is_some_and(|d| d <= DURATION_CLOSE_MS);

            let title_exact = !cand_title.is_empty() && cand_title == ctx_title;
            let artist_exact = !ctx_artist.is_empty() && cand_artist_joined == ctx_artist;
            let artist_any_hit = !ctx_artist.is_empty()
                && (both_contains(&cand_artist_joined, &ctx_artist)
                    || cand_artists_norm
                        .iter()
                        .any(|a| both_contains(a, &ctx_artist)));

            let mut score = 0;
            if title_exact {
                score += 10;
            } else if both_contains(&cand_title, &ctx_title) {
                score += 4;
            }

            if artist_exact {
                score += 5;
            } else if artist_any_hit {
                score += 2;
            } else if !ctx_artist.is_empty() && !cand_artist_joined.is_empty() {
                // 本地有明确歌手但候选完全不命中：扣分以防同名翻唱误匹配。
                score -= 2;
            }

            if !ctx_album.is_empty() && !cand_album.is_empty() {
                if cand_album == ctx_album {
                    score += 2;
                } else if both_contains(&cand_album, &ctx_album) {
                    score += 1;
                }
            }

            if duration_close {
                score += 3;
            }

            RankedCandidate {
                candidate,
                score,
                duration_diff_ms,
                duration_far,
                tags: MatchedTags {
                    title_exact,
                    artist_match: artist_exact || artist_any_hit,
                    duration_close,
                    best: false,
                },
            }
        })
        .collect();

    ranked.sort_by(|a, b| {
        a.duration_far
            .cmp(&b.duration_far)
            .then_with(|| b.score.cmp(&a.score))
    });

    if let Some(top) = ranked.first_mut()
        && !top.duration_far
        && top.score >= 10
    {
        top.tags.best = true;
    }

    ranked
}

/// 批量自动匹配时判定一条最高分候选是否达到自动采纳阈值。
pub fn passes_batch_threshold(top: &RankedCandidate, strictness: MatchStrictness) -> bool {
    if top.duration_far {
        return false;
    }
    match strictness {
        MatchStrictness::Standard => {
            top.score >= 12
                || (top.tags.title_exact && (top.tags.artist_match || top.tags.duration_close))
        }
        MatchStrictness::Relaxed => top.score >= 10 || top.tags.title_exact,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeo_types::config::MatchSource;

    fn sample(title: &str, artist: &str, album: &str, duration_ms: u64) -> MatchCandidate {
        MatchCandidate {
            source: MatchSource::QQMusic,
            source_id: format!("{title}-{artist}"),
            ext_id: None,
            title: title.into(),
            artists: vec![artist.into()],
            album: Some(album.into()),
            album_artist: None,
            year: Some("2003".into()),
            track_number: Some(3),
            disc_number: Some(1),
            duration_ms: Some(duration_ms),
            cover_thumb_url: None,
            cover_original_url: None,
        }
    }

    #[test]
    fn filename_fallback_parses_track_num_and_artist() {
        assert_eq!(
            parse_filename_fallback("03. 周杰伦 - 晴天.flac"),
            ("晴天".into(), "周杰伦".into())
        );
        assert_eq!(
            parse_filename_fallback("01 - 稻香.mp3"),
            ("稻香".into(), "".into())
        );
    }

    #[test]
    fn traditional_and_simplified_match_equally() {
        let q = build_query("晴天.flac", "晴天", "周杰倫", "葉惠美", Some(269_000));
        let ranked = rank_candidates(
            vec![
                sample("晴天 (Live)", "周杰伦", "演唱会", 310_000),
                sample("晴天", "周杰伦", "叶惠美", 269_500),
                sample("晴天", "某翻唱", "翻唱集", 269_000),
            ],
            &q,
        );
        assert_eq!(ranked[0].candidate.album.as_deref(), Some("叶惠美"));
        assert!(ranked[0].tags.best);
        assert!(passes_batch_threshold(
            &ranked[0],
            MatchStrictness::Standard
        ));
        assert!(ranked[2].duration_far);
    }
}
