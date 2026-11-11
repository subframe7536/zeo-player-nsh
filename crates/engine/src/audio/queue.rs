use super::*;

/// 按循环模式生成播放顺序：随机洗牌 `base` 的副本（原序保持不动），
/// 其余模式就是 `base` 本身（共享同一个 `Arc`）。
pub(crate) fn ordered_queue(base: &Arc<Vec<String>>, mode: LoopMode) -> Arc<Vec<String>> {
    match mode {
        LoopMode::Random => Arc::new(shuffled(base)),
        _ => base.clone(),
    }
}

/// 随机队列（参考 audio0 的 `createSmartShuffle`）：同一歌手的曲目按
/// 黄金比例近似均匀摊开——连续撞上一个歌手的概率大降；组内纯随机
/// （权重先按 audio0 的默认固定为 3，不按播放次数加权）。歌手从曲库
/// 索引一次取好（不在索引里的归空组，不读文件标签）。
pub(crate) fn shuffled(base: &[String]) -> Vec<String> {
    let tracks = zeo_library::config::library_store().tracks();
    let artist_of: std::collections::HashMap<&str, &str> = tracks
        .iter()
        .map(|t| {
            let a = if t.artist.is_empty() {
                t.album_artist.as_str()
            } else {
                t.artist.as_str()
            };
            (t.path.as_str(), a)
        })
        .collect();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let order = spread_order(
        base.len(),
        |i| artist_of.get(base[i].as_str()).copied().unwrap_or(""),
        |_| 3.0,
        seed,
    );
    order.into_iter().map(|i| base[i].clone()).collect()
}

/// 黄金比例（audio0 smart shuffle 同款常数）。
pub(crate) const GOLDEN: f64 = 0.618033988749895;

pub(crate) fn frac(x: f64) -> f64 {
    x - x.floor()
}

/// audio0 的字符串散列：`h = h*31 + c` 按 int32 溢出滚动，归一到 0..1。
pub(crate) fn string_hash(s: &str) -> f64 {
    let mut h: i32 = 0;
    for c in s.chars() {
        h = h.wrapping_mul(31).wrapping_add(c as i32);
    }
    (h as i64).unsigned_abs() as f64 / 2147483648.0
}

/// 把 `0..n` 重排成「同组摊开」的顺序：每组（歌手）按
/// `散列(组名)×黄金比例` 定环上偏移，组内第 j 个落在
/// `偏移 + j/组大小 + 抖动`（模 1），全体按落点排序即得队列——同一
/// 组自然近似均匀摊开（audio0 `createSmartShuffle`）。组内顺序按
/// `rand × (score+1)` 加权（audio0 `weightedShuffle`，+1 让零播放
/// 曲目也有机会）。同 `seed` 同输入必同输出（组键先排序再遍历）。
pub(crate) fn spread_order<'a>(
    n: usize,
    group: impl Fn(usize) -> &'a str,
    score: impl Fn(usize) -> f64,
    seed: u64,
) -> Vec<usize> {
    let mut groups: std::collections::HashMap<&'a str, Vec<usize>> =
        std::collections::HashMap::new();
    for i in 0..n {
        groups.entry(group(i)).or_default().push(i);
    }
    let mut keys: Vec<&&str> = groups.keys().collect();
    keys.sort();
    let mut rng = seed | 0x9e37_79b9_7f4a_7c15;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 11) as f64 / ((1u64 << 53) as f64)
    };
    let global_offset = frac((seed as f64).sin().abs() * 10000.0);
    let mut positions = vec![0.0_f64; n];
    for key in keys {
        let items = &groups[key];
        let len = items.len();
        let density = len as f64 / n.max(1) as f64;
        let base = frac(frac(string_hash(key) + global_offset) * GOLDEN) % (1.0 - density + 0.001);
        // 组内加权序：`rand × (score+1)` 降序，得分高的靠前占位。
        let mut order: Vec<(usize, f64)> = items
            .iter()
            .map(|&i| (i, next() * (1.0 + score(i))))
            .collect();
        order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let len_f = len as f64;
        for (j, &(i, _)) in order.iter().enumerate() {
            let jitter = next() * 0.1 / len_f;
            positions[i] = frac(base + j as f64 / len_f + jitter);
        }
    }
    let mut out: Vec<usize> = (0..n).collect();
    out.sort_by(|a, b| {
        positions[*a]
            .partial_cmp(&positions[*b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// 「听过多少算一次播放」：实际播出 `listened` 秒、整首 `duration` 秒时，
/// 百分比阈值（`min_percent`，0 = 不用）或时长阈值（`min_secs`，0 = 不用）
/// 任一满足即算一次。两个都是 0 时开始播放就算。`duration` 缺失 / 为 0
/// 时百分比条件不生效（秒数条件仍可用）。
pub(crate) fn counts_as_play(
    listened: f64,
    duration: f64,
    min_percent: u32,
    min_secs: u32,
) -> bool {
    if min_percent == 0 && min_secs == 0 {
        return true;
    }
    let by_percent =
        min_percent > 0 && duration > 0.0 && listened >= duration * f64::from(min_percent) / 100.0;
    let by_seconds = min_secs > 0 && listened >= f64::from(min_secs);
    by_percent || by_seconds
}

/// 下标 `index` 后去哪一首（`forward` 上一首 / 下一首）：单曲停在原地
/// （重播当前），其余模式前进 / 后退一步并在端点回绕。
pub(crate) fn advance(index: i64, len: usize, mode: LoopMode, forward: bool) -> Option<i64> {
    if len == 0 || index < 0 {
        return None;
    }
    match mode {
        LoopMode::Single => Some(index),
        _ => Some((index + if forward { 1 } else { -1 }).rem_euclid(len as i64)),
    }
}
