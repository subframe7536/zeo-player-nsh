use super::*;

#[derive(PartialEq)]
pub(crate) enum SortKey {
    Empty,
    Number(f64),
    Text { lower: String, orig: String },
}

pub(crate) fn compare_sort_keys(a: &SortKey, b: &SortKey, ascending: bool) -> Ordering {
    match (a, b) {
        (SortKey::Empty, SortKey::Empty) => return Ordering::Equal,
        (SortKey::Empty, _) => return Ordering::Greater,
        (_, SortKey::Empty) => return Ordering::Less,
        _ => {}
    }
    let ord = match (a, b) {
        (SortKey::Number(x), SortKey::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (
            SortKey::Text {
                lower: lx,
                orig: ox,
            },
            SortKey::Text {
                lower: ly,
                orig: oy,
            },
        ) => lx.cmp(ly).then_with(|| ox.cmp(oy)),
        _ => Ordering::Equal,
    };
    if ascending { ord } else { ord.reverse() }
}

/// `displayed` memo 的就地排序（decorate-sort：预解析键与索引，
/// 比较循环内 0 堆分配，重排移动原值避免全量克隆 TrackEntry）。
pub(crate) fn sort_entries(
    list: &mut Vec<TrackEntry>,
    key: &str,
    ascending: bool,
    rows: &HashMap<String, Arc<MetaRow>>,
) {
    let mut keyed: Vec<(SortKey, usize)> = list
        .iter()
        .enumerate()
        .map(|(idx, t)| {
            let row = rows.get(t.path.as_str()).map(|r| r.as_ref());
            let val = cell_value(key, t, row);
            let sort_key = if val.is_empty() {
                SortKey::Empty
            } else if NUMERIC.contains(&key) {
                match val.trim_end_matches(" dB").parse::<f64>().ok() {
                    Some(n) => SortKey::Number(n),
                    None => SortKey::Text {
                        lower: val.to_lowercase(),
                        orig: val.into_owned(),
                    },
                }
            } else {
                SortKey::Text {
                    lower: val.to_lowercase(),
                    orig: val.into_owned(),
                }
            };
            (sort_key, idx)
        })
        .collect();

    keyed.sort_by(|(a, _), (b, _)| compare_sort_keys(a, b, ascending));

    let old = std::mem::take(list);
    let mut old_opts: Vec<Option<TrackEntry>> = old.into_iter().map(Some).collect();
    list.reserve(old_opts.len());
    for (_, idx) in keyed {
        if let Some(entry) = old_opts[idx].take() {
            list.push(entry);
        }
    }
}
