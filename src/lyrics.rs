//! LRC 歌词解析，按 plan.md「audio-playground 实现规格」移植：
//! 时间戳、卡拉 OK 碎片清理、排序、元数据头合并、翻译行、空行策略、
//! 无时间戳回退。解析算法只在此处实现一份（Rust），TSX 直接消费结果。

/// 一行歌词（LrcObj）。
#[derive(Debug, Clone, PartialEq)]
pub struct LrcLine {
    /// 行序号；无时间戳回退（多行）时为 -1。
    pub index: i64,
    /// 秒；无时间信息时为 -1。
    pub time: f64,
    /// 原文。
    pub raw_content: String,
    /// 翻译（相邻同时间行合并而来）。
    pub trans_content: Option<String>,
    /// 开头元数据头合并行（不参与跟唱高亮的特殊样式标记）。
    pub metadata: bool,
}

/// 解析结果。
#[derive(Debug, Clone, PartialEq)]
pub struct LyricsOutput {
    pub lines: Vec<LrcLine>,
    /// 是否存在带时间戳的行（跟唱模式）。
    pub timed: bool,
}

/// 解析选项；默认与参考站加载一致。
#[derive(Debug, Clone, Copy)]
pub struct LyricOptions {
    /// 跳过短空行。
    pub ignore_short_empty_lines: bool,
    /// 空行至少要持续的秒数才保留。
    pub min_empty_line_duration: f64,
}

impl Default for LyricOptions {
    fn default() -> Self {
        LyricOptions { ignore_short_empty_lines: true, min_empty_line_duration: 3.0 }
    }
}

/// 解析 LRC 文本。
pub fn parse_lyric(text: &str, options: LyricOptions) -> LyricsOutput {
    let mut timed_entries: Vec<(f64, String)> = Vec::new();
    let mut untimed: Vec<String> = Vec::new();

    for line in text.lines() {
        let line = line.trim_end_matches(['\r', '\n']);
        let (times, content) = split_timestamps(line);
        if times.is_empty() {
            // 纯元数据标签行（[ti:…] 等）整体丢弃，其余作为无时间文本保留。
            if !is_meta_tag_line(line) {
                untimed.push(clean_content(line).trim().to_owned());
            }
            continue;
        }
        let content = clean_content(&content).trim().to_owned();
        for t in times {
            timed_entries.push((t, content.clone()));
        }
    }

    if timed_entries.is_empty() {
        // 无时间戳回退：单行 → time=0；多行 → time=-1、index=-1（仅展示）。
        let texts: Vec<String> = untimed.into_iter().filter(|c| !c.is_empty()).collect();
        return match texts.len() {
            0 => LyricsOutput { lines: Vec::new(), timed: false },
            1 => LyricsOutput {
                lines: vec![LrcLine {
                    index: 0,
                    time: 0.0,
                    raw_content: texts.into_iter().next().unwrap(),
                    trans_content: None,
                    metadata: false,
                }],
                timed: true,
            },
            _ => LyricsOutput {
                lines: texts
                    .into_iter()
                    .map(|c| LrcLine {
                        index: -1,
                        time: -1.0,
                        raw_content: c,
                        trans_content: None,
                        metadata: false,
                    })
                    .collect(),
                timed: false,
            },
        };
    }

    // 按时间升序（稳定：同时间保持原文顺序）。
    timed_entries.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // 中间夹带的无时间行属于噪声，丢弃（有带时间行的文件里）。
    let mut entries: Vec<Entry> = timed_entries
        .into_iter()
        .map(|(time, content)| Entry { time, content, trans: None, metadata: false })
        .collect();

    merge_leading_metadata(&mut entries);
    merge_translations(&mut entries);
    apply_empty_line_policy(&mut entries, options);

    let lines = entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| LrcLine {
            index: i as i64,
            time: e.time,
            raw_content: e.content,
            trans_content: e.trans,
            metadata: e.metadata,
        })
        .collect();
    LyricsOutput { lines, timed: true }
}

#[derive(Debug, Clone)]
struct Entry {
    time: f64,
    content: String,
    trans: Option<String>,
    metadata: bool,
}

/// 提取行首全部 `[mm:ss.xx]` / `[mm:ss.xxx]` 时间戳；返回（时间列表，剩余正文）。
fn split_timestamps(line: &str) -> (Vec<f64>, String) {
    let mut times = Vec::new();
    let mut rest = line;
    while rest.starts_with('[') {
        let Some(tag_end) = rest.find(']') else { break };
        if tag_end < 2 {
            break; // "[]" 空标签
        }
        // '[' 与 ']' 均为 ASCII，边界安全。
        let inner = &rest[1..tag_end];
        let Some(seconds) = parse_time_tag(inner) else { break };
        times.push(seconds);
        rest = &rest[tag_end + 1..];
    }
    (times, rest.to_owned())
}

/// `[mm:ss.xx]` 内部 → 秒。mm 一到三位，ss 两位，小数一到三位（宽容）。
fn parse_time_tag(inner: &str) -> Option<f64> {
    let (mins, secs) = inner.split_once(':')?;
    if mins.is_empty() || mins.len() > 3 || !mins.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (int, frac) = match secs.split_once('.') {
        Some((int, frac)) => (int, Some(frac)),
        None => (secs, None),
    };
    if int.len() != 2 || !int.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if let Some(frac) = frac {
        if frac.is_empty() || frac.len() > 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let minutes: f64 = mins.parse().ok()?;
    let seconds: f64 = int.parse().ok()?;
    let fraction = match frac {
        Some(frac) => {
            let digits: f64 = frac.parse().ok()?;
            digits / 10f64.powi(frac.len() as i32)
        }
        None => 0.0,
    };
    Some(minutes * 60.0 + seconds + fraction)
}

/// 去掉正文中残留的尖括号/方括号卡拉 OK 碎片时间标签（`<00:12.30>`、`[00:12.30]`）。
fn clean_content(content: &str) -> String {
    let chars: Vec<char> = content.chars().collect();
    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    while i < chars.len() {
        let open = chars[i];
        if open == '<' || open == '[' {
            let close = if open == '<' { '>' } else { ']' };
            if let Some(end) = chars[i + 1..].iter().position(|&c| c == close) {
                let inner: String = chars[i + 1..i + 1 + end].iter().collect();
                if parse_time_tag(&inner).is_some() {
                    i += 1 + end + 1; // 整段丢弃
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `[ti:…]` `[ar:…]` 等 LRC 元数据头标签行。
fn is_meta_tag_line(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('[') else { return false };
    let Some((key, value)) = rest.split_once(':') else { return false };
    if !value.ends_with(']') || value.len() < 2 {
        return false;
    }
    matches!(
        key.to_ascii_lowercase().as_str(),
        "ti" | "ar" | "al" | "by" | "offset" | "re" | "ve" | "length" | "au"
    )
}

/// 前 ≥3 条同一 time：合并为一条 rawContent（换行拼接），视为 leading metadata。
fn merge_leading_metadata(entries: &mut Vec<Entry>) {
    if entries.len() < 3 {
        return;
    }
    let time = entries[0].time;
    let run = entries.iter().take_while(|e| e.time == time).count();
    if run < 3 {
        return;
    }
    let joined = entries[..run]
        .iter()
        .map(|e| e.content.as_str())
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    entries.splice(
        0..run,
        Some(Entry { time, content: joined, trans: None, metadata: true }),
    );
}

/// 相邻两条 time 完全相同且都有 content：前者 rawContent、后者 transContent。
fn merge_translations(entries: &mut Vec<Entry>) {
    let mut i = 0;
    while i + 1 < entries.len() {
        let same = entries[i].time == entries[i + 1].time
            && !entries[i].content.is_empty()
            && !entries[i + 1].content.is_empty()
            && entries[i].trans.is_none();
        if same {
            let trans = entries.remove(i + 1).content;
            entries[i].trans = Some(trans);
        }
        i += 1;
    }
}

/// 空行策略：短空行跳过，连续空行合并（以第一条的时长到下一个非空行为准）。
fn apply_empty_line_policy(entries: &mut Vec<Entry>, options: LyricOptions) {
    if !options.ignore_short_empty_lines {
        return;
    }
    let mut kept: Vec<Entry> = Vec::with_capacity(entries.len());
    let mut i = 0;
    while i < entries.len() {
        if !entries[i].content.is_empty() {
            kept.push(entries[i].clone());
            i += 1;
            continue;
        }
        // 一段连续空行：时长从第一条量到下一个非空行。
        let start = i;
        let run_end = entries[i..].iter().position(|e| !e.content.is_empty()).map_or(entries.len(), |n| i + n);
        let next_time = entries.get(run_end).map(|e| e.time);
        let keep = match next_time {
            Some(next) => next - entries[start].time >= options.min_empty_line_duration,
            None => false, // 结尾的空行丢弃
        };
        if keep {
            kept.push(entries[start].clone());
        }
        i = run_end;
    }
    *entries = kept;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times(out: &LyricsOutput) -> Vec<f64> {
        out.lines.iter().map(|l| l.time).collect()
    }

    #[test]
    fn basic_timestamps() {
        let out = parse_lyric("[00:01.00]hello\n[00:05.50]world", LyricOptions::default());
        assert_eq!(times(&out), vec![1.0, 5.5]);
        assert_eq!(out.lines[0].raw_content, "hello");
        assert!(out.timed);
    }

    #[test]
    fn multiple_timestamps_share_content() {
        let out = parse_lyric("[00:01.00][00:10.00]chorus", LyricOptions::default());
        assert_eq!(times(&out), vec![1.0, 10.0]);
        assert_eq!(out.lines[0].raw_content, "chorus");
        assert_eq!(out.lines[1].raw_content, "chorus");
    }

    #[test]
    fn three_digit_fraction() {
        let out = parse_lyric("[01:02.345]x", LyricOptions::default());
        assert!((times(&out)[0] - 62.345).abs() < 1e-9);
    }

    #[test]
    fn sorts_by_time() {
        let out = parse_lyric("[00:09.00]b\n[00:02.00]a", LyricOptions::default());
        assert_eq!(times(&out), vec![2.0, 9.0]);
    }

    #[test]
    fn karaoke_fragments_removed() {
        let out = parse_lyric("[00:01.00]你<00:01.50>好[00:02.00]呀", LyricOptions::default());
        assert_eq!(out.lines[0].raw_content, "你好呀");
    }

    #[test]
    fn leading_metadata_merged() {
        let text = "[00:00.00]作词：A\n[00:00.00]作曲：B\n[00:00.00]编曲：C\n[00:01.00]第一句";
        let out = parse_lyric(text, LyricOptions::default());
        assert_eq!(out.lines.len(), 2);
        assert!(out.lines[0].metadata);
        assert_eq!(out.lines[0].raw_content, "作词：A\n作曲：B\n编曲：C");
        assert_eq!(out.lines[0].time, 0.0);
        assert_eq!(out.lines[1].raw_content, "第一句");
    }

    #[test]
    fn translation_lines() {
        let text = "[00:01.00]hello\n[00:01.00]你好\n[00:05.00]bye";
        let out = parse_lyric(text, LyricOptions::default());
        assert_eq!(out.lines.len(), 2);
        assert_eq!(out.lines[0].raw_content, "hello");
        assert_eq!(out.lines[0].trans_content.as_deref(), Some("你好"));
        assert_eq!(out.lines[1].raw_content, "bye");
    }

    #[test]
    fn short_empty_lines_dropped() {
        let text = "[00:01.00]a\n[00:02.00]\n[00:03.00]b";
        let out = parse_lyric(text, LyricOptions::default());
        assert_eq!(out.lines.len(), 2);
    }

    #[test]
    fn long_empty_kept_and_consecutive_merged() {
        // 第一条空行到下一个非空行 5 秒 → 保留一条；第二条并入。
        let text = "[00:01.00]a\n[00:02.00]\n[00:04.00]\n[00:07.00]b";
        let out = parse_lyric(text, LyricOptions::default());
        assert_eq!(out.lines.len(), 3);
        assert_eq!(out.lines[1].time, 2.0);
        assert_eq!(out.lines[1].raw_content, "");
    }

    #[test]
    fn consecutive_short_empties_all_dropped() {
        let text = "[00:01.00]a\n[00:02.00]\n[00:03.00]\n[00:04.00]b";
        let out = parse_lyric(text, LyricOptions::default());
        assert_eq!(out.lines.len(), 2);
    }

    #[test]
    fn trailing_empty_dropped() {
        let text = "[00:01.00]a\n[00:30.00]";
        let out = parse_lyric(text, LyricOptions::default());
        assert_eq!(out.lines.len(), 1);
    }

    #[test]
    fn untimed_single_line() {
        let out = parse_lyric("only line", LyricOptions::default());
        assert_eq!(out.lines.len(), 1);
        assert_eq!(out.lines[0].time, 0.0);
        assert_eq!(out.lines[0].index, 0);
    }

    #[test]
    fn untimed_multi_line() {
        let out = parse_lyric("line1\nline2", LyricOptions::default());
        assert_eq!(out.lines.len(), 2);
        assert!(out.lines.iter().all(|l| l.time == -1.0 && l.index == -1));
        assert!(!out.timed);
    }

    #[test]
    fn meta_tag_lines_dropped() {
        let out = parse_lyric("[ti:Song]\n[ar:Artist]\n[00:01.00]a", LyricOptions::default());
        assert_eq!(out.lines.len(), 1);
    }

    #[test]
    fn indexes_sequential() {
        let out = parse_lyric("[00:01.00]a\n[00:02.00]b\n[00:03.00]c", LyricOptions::default());
        assert_eq!(out.lines.iter().map(|l| l.index).collect::<Vec<_>>(), vec![0, 1, 2]);
    }
}
