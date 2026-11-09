use serde::{Deserialize, Serialize};

use crate::{Field, FieldType, Op, Sort, parse_number};
use zeo_types::config::TrackEntry;

/// 智能歌单的规则。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// 全部条件都满足（`true`）还是满足任一（`false`）。
    #[serde(default = "yes")]
    pub match_all: bool,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub sort: Sort,
    /// 只取排序后的前 N 首（`None`：不限）。
    #[serde(default)]
    pub limit: Option<usize>,
}

fn yes() -> bool {
    true
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            match_all: true,
            conditions: vec![Condition::default()],
            sort: Sort::default(),
            limit: None,
        }
    }
}

/// 一个条件：字段、比较方式、值（文本；数值字段按字段的单位解析）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub field: Field,
    pub op: Op,
    #[serde(default)]
    pub value: String,
}

impl Default for Condition {
    fn default() -> Self {
        Condition {
            field: Field::Artist,
            op: Op::Contains,
            value: String::new(),
        }
    }
}

impl Condition {
    /// 值为空或解析不了的条件不参与筛选（编辑到一半时列表不清空）。
    pub fn is_complete(&self) -> bool {
        match self.field.kind() {
            FieldType::Text => !self.value.trim().is_empty(),
            FieldType::Number | FieldType::Date => parse_number(self.field, &self.value).is_some(),
        }
    }

    /// `now`：当前 Unix 秒（修改时间条件用）。
    pub fn matches(&self, t: &TrackEntry, now: u64) -> bool {
        match self.field.kind() {
            FieldType::Text => {
                let want = self.value.trim().to_lowercase();
                let values: Vec<String> = self
                    .field
                    .texts(t)
                    .into_iter()
                    .map(|v| v.to_lowercase())
                    .collect();
                let any = |f: &dyn Fn(&str) -> bool| values.iter().any(|v| f(v));
                match self.op {
                    Op::Contains => any(&|v| v.contains(&want)),
                    Op::NotContains => !any(&|v| v.contains(&want)),
                    Op::Is => any(&|v| v == want),
                    Op::IsNot => !any(&|v| v == want),
                    Op::StartsWith => any(&|v| v.starts_with(&want)),
                    Op::EndsWith => any(&|v| v.ends_with(&want)),
                    _ => false,
                }
            }
            FieldType::Number => {
                let (Some(want), Some(have)) =
                    (parse_number(self.field, &self.value), self.field.number(t))
                else {
                    return false;
                };
                // 「是」按显示精度比较：时长差 1 秒内、其它差 0.5% 内都算相等
                // （采样率 44.1 与 44100 Hz、文件大小 3.2 MB 这类）。
                let close = match self.field {
                    Field::Duration => (have - want).abs() < 1.0,
                    Field::Year | Field::Channels => (have - want).abs() < 0.5,
                    _ => (have - want).abs() <= want.abs() * 0.005 + 1e-9,
                };
                match self.op {
                    Op::Is => close,
                    Op::IsNot => !close,
                    Op::Greater => have > want,
                    Op::Less => have < want,
                    _ => false,
                }
            }
            FieldType::Date => {
                let Some(days) = parse_number(self.field, &self.value) else {
                    return false;
                };
                let age = now.saturating_sub(t.modified) as f64 / 86_400.0;
                match self.op {
                    Op::InLast => age <= days,
                    Op::NotInLast => age > days,
                    _ => false,
                }
            }
        }
    }
}

impl Rule {
    /// 筛选、排序、截断。没有完整条件时匹配整个曲库。
    pub fn select(&self, library: &[TrackEntry], now: u64) -> Vec<TrackEntry> {
        let active: Vec<&Condition> = self.conditions.iter().filter(|c| c.is_complete()).collect();
        let mut out: Vec<TrackEntry> = library
            .iter()
            .filter(|t| {
                if active.is_empty() {
                    return true;
                }
                if self.match_all {
                    active.iter().all(|c| c.matches(t, now))
                } else {
                    active.iter().any(|c| c.matches(t, now))
                }
            })
            .cloned()
            .collect();
        let text = |a: &str, b: &str| a.to_lowercase().cmp(&b.to_lowercase());
        match self.sort {
            Sort::Library => {}
            Sort::Title => out.sort_by(|a, b| text(&a.title, &b.title)),
            Sort::Artist => out.sort_by(|a, b| text(&a.artist, &b.artist)),
            Sort::Album => out.sort_by(|a, b| {
                text(&a.album, &b.album).then((a.disk, a.track).cmp(&(b.disk, b.track)))
            }),
            Sort::YearDesc => out.sort_by_key(|t| std::cmp::Reverse(t.year)),
            Sort::ModifiedDesc => out.sort_by_key(|t| std::cmp::Reverse(t.modified)),
            Sort::DurationDesc => out.sort_by(|a, b| b.duration.total_cmp(&a.duration)),
            Sort::BitRateDesc => out.sort_by_key(|t| std::cmp::Reverse(t.bit_rate)),
        }
        if let Some(limit) = self.limit {
            out.truncate(limit);
        }
        out
    }
}
