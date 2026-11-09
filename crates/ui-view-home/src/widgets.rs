use super::*;

pub(crate) fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// 抽 `n` 首：Fisher–Yates 洗牌取前 n（xorshift64 种子，同
/// `engine::audio`，不引 rand）。
pub(crate) fn pick(tracks: &[TrackEntry], n: usize) -> Vec<TrackEntry> {
    let mut out = tracks.to_vec();
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        | 0x9e37_79b9_7f4a_7c15;
    for i in (1..out.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed % (i as u64 + 1)) as usize;
        out.swap(i, j);
    }
    out.truncate(n);
    out
}

/// 时段问候：按本地小时分五档。
pub(crate) fn greeting(hour: u32) -> &'static str {
    match hour {
        5..=10 => "早上好",
        11..=13 => "中午好",
        14..=17 => "下午好",
        18..=22 => "晚上好",
        _ => "夜深了",
    }
}

/// 问候下的副文案，跟着时段换。
pub(crate) fn tagline(hour: u32) -> &'static str {
    match hour {
        5..=10 => "新的一天，从一首喜欢的歌开始",
        11..=13 => "歇一会儿，听点轻松的",
        14..=17 => "下午困倦的时候，来点提神的",
        18..=22 => "戴上耳机，让世界安静一会儿",
        _ => "适合听点慢的，别熬太晚",
    }
}

/// 头部指标：大数字 + 小单位，下面一行 muted 标签。
pub(crate) fn metric(
    value: impl Fn() -> String + 'static,
    unit: &'static str,
    label: &'static str,
) -> View {
    view()
        .gap(px(4.0))
        .child(
            view()
                .flex_row()
                .items_end()
                .gap(px(2.0))
                .child(
                    text(value)
                        .text_size(px(26.0))
                        .line_height(px(26.0))
                        .font_weight(FontWeight::BOLD)
                        .tabular_nums()
                        .text_color(fg()),
                )
                .child(
                    text(unit)
                        .text_size(px(13.0))
                        .line_height(px(18.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(
            text(label)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg()),
        )
}

/// 头部：时段问候 + 本周时长 / 新增收藏 / 连续收听。原 page_header
/// 兼任窗口拖动区，这里带上 `window_drag`。
pub(crate) fn hero(stats: Signal<zeo_db::PlayStats>, now: Signal<u64>) -> View {
    let hour =
        move || zeo_types::config::local_hour(now.get(), zeo_types::config::local_offset_seconds());
    view()
        .window_drag(true)
        .flex_row()
        .items_center()
        .px(px(32.0))
        .pt(px(28.0))
        .pb(px(24.0))
        .child(
            view()
                .gap(px(8.0))
                .child(
                    text(move || greeting(hour()))
                        .text_size(px(28.0))
                        .line_height(px(28.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(fg()),
                )
                .child(
                    text(move || tagline(hour()))
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(view().flex_1())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(36.0))
                .child(metric(
                    move || format!("{:.1}", stats.with(|v| v.week_secs) / 3600.0),
                    "h",
                    "本周时长",
                ))
                .child(metric(
                    move || grouped(stats.with(|v| v.week_likes)),
                    "首",
                    "新增收藏",
                ))
                .child(metric(
                    move || grouped(stats.with(|v| v.streak)),
                    "天",
                    "连续收听",
                )),
        )
}

/// 节标题。
pub(crate) fn section_title(title: &'static str) -> Text {
    text(title)
        .text_size(px(15.0))
        .line_height(px(15.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg())
}

/// 卡条统一的最小卡宽与间距（翻页范围按它算）。
pub(crate) const STRIP_MIN_W: f32 = 200.0;
pub(crate) const STRIP_GAP: f32 = 14.0;
