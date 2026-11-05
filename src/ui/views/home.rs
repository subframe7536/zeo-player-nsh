//! 「首页」：时段问候头部、随机推荐、热门艺术家、新添加、最近播放。
//! 「播放了多少」按引擎实际播出的帧数（输出回调推进）算——拖动进度条
//! 不算听过；阈值（默认 60% 或 30 秒，任一满足）在设置 →「播放」→
//! 「播放统计」里调。

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use nsh_base::{ButtonElement, button};
use nsh_core::prelude::*;

use super::super::components::button::icon_button;
use super::super::components::cover::{cover, cover_initial, initial_letter};
use super::super::components::icons::{Icon, icon};
use super::super::components::page::empty_state;
use super::super::components::tracks::content_width;
use super::super::store::{cover_for, covers_on, play_list, player, queue_add, store};
use super::super::theme::*;
use super::super::Nav;
use super::{browse::avatar, no_library, playlists::track_menu_with};
use crate::config::TrackEntry;

/// 千分位（页头与统计卡的大数）。
fn grouped(n: u64) -> String {
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
fn pick(tracks: &[TrackEntry], n: usize) -> Vec<TrackEntry> {
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
fn greeting(hour: u32) -> &'static str {
    match hour {
        5..=10 => "早上好",
        11..=13 => "中午好",
        14..=17 => "下午好",
        18..=22 => "晚上好",
        _ => "夜深了",
    }
}

/// 问候下的副文案，跟着时段换。
fn tagline(hour: u32) -> &'static str {
    match hour {
        5..=10 => "新的一天，从一首喜欢的歌开始",
        11..=13 => "歇一会儿，听点轻松的",
        14..=17 => "下午困倦的时候，来点提神的",
        18..=22 => "戴上耳机，让世界安静一会儿",
        _ => "适合听点慢的，别熬太晚",
    }
}

/// 头部指标：大数字 + 小单位，下面一行 muted 标签。
fn metric(
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
fn hero(stats: Signal<crate::db::PlayStats>, now: Signal<u64>) -> View {
    let hour = move || {
        crate::config::local_hour(now.get(), crate::config::local_offset_seconds())
    };
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
fn section_title(title: &'static str) -> Text {
    text(title)
        .text_size(px(15.0))
        .line_height(px(15.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg())
}

/// 推荐卡：封面 + 曲名 + 歌手；按下以这一批为队列从本卡开始播。
/// `min_w` 给换行点、`max_w` 防末行独苗卡被 `flex_1` 拉成整行宽。
fn card(t: &TrackEntry, index: usize, five: Rc<Vec<TrackEntry>>) -> ButtonElement {
    // `src` / `letter` 进 `when` 的响应闭包：要 `Copy`，用 `stored` 持有。
    let path = stored(t.path.clone());
    let title = stored(t.title.clone());
    let src = move || cover_for(&path.get(), 140.0);
    let letter = move || initial_letter(&title.get(), &path.get());
    button()
        .flex_1()
        .min_w(px(200.0))
        .gap(px(8.0))
        .items_center()
        .p(px(10.0))
        .rounded(px(14.0))
        .accessibility_label(SharedString::from(t.title.clone()))
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.75))
        .on_press(move |_: &()| play_list(&five, index))
        .child(when(covers_on, move || cover_initial(src, letter, Some(140.0), 12.0)))
        .child(
            view()
                .px(px(2.0))
                .items_center()
                .gap(px(2.0))
                .child(
                    text(t.title.clone())
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg()),
                )
                .child(
                    text(t.artist.clone())
                        .number_of_lines(1)
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(muted_fg()),
                ),
        )
}

/// 随机推荐：一批随机曲目的小歌单卡——左封面 + 「本地曲库」徽标 +
/// 「随机翻到了 X」+ 精选首数 + 立即播放 / 添加到队列 / 换一批，
/// 右侧是这批曲目的编号列表（行即播）。
fn featured_card(featured: Signal<Rc<Vec<TrackEntry>>>) -> AnyElement {
    dynamic(move || {
        let list = featured.get();
        let Some(first) = list.first() else {
            return view().into_element();
        };
        let path = stored(first.path.clone());
        let art = move || cover_for(&path.get(), 400.0);
        let title = first.title.clone();
        let queue = list.clone();
        let reroll = move || featured.set(Rc::new(pick(&store().tracks.get_untracked(), 6)));
        // 列表行：序号 + 曲名 + 右对齐歌手；点行即以本批为队列从该行播。
        // 列直接当卡片的 flex 子项，属性直达——`when` 的 Region 夹在
        // 中间时内层 flex 属性够不到、行撑不满会留下右缘空档。整列
        // ≤ 卡宽 40%、垂直居中、贴右缘：`ml(auto)` 吃掉 `max_w` 封顶后
        // 剩下的自由空间把列顶到右缘，没封顶时 margin 为 0。宽度不够
        //（< 880）display:none。
        let mut rows = view()
            .flex_1()
            .min_w(px(0.0))
            .max_w(percentage(0.4))
            .ml(auto())
            .justify_center()
            .gap(px(2.0))
            .style_when(move || content_width() < 880.0, |s| s.hidden());
        for (i, t) in list.iter().take(6).enumerate() {
            let row_track = t.clone();
            let row_list = list.clone();
            rows = rows.child(
                view()
                    .h(px(36.0))
                    .flex_row()
                    .items_center()
                    .gap(px(12.0))
                    .px(px(12.0))
                    .rounded(px(8.0))
                    .role(AccessibilityRole::Button)
                    .label(SharedString::from(t.title.clone()))
                    .hover(|st| st.bg(hover()))
                    .on_press(move |_: &()| play_list(&row_list, i))
                    .child(
                        text(format!("{:02}", i + 1))
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .tabular_nums()
                            .text_color(faint()),
                    )
                    .child(
                        text(row_track.title.clone())
                            .flex_1()
                            .min_w(px(0.0))
                            .number_of_lines(1)
                            .text_size(px(14.0))
                            .line_height(px(14.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg()),
                    )
                    .child(
                        // 歌手先省略：封顶 240、收缩权重×2、允许
                        // 缩到 0——标题只在歌手让完位后才被截。
                        text(row_track.artist.clone())
                            .min_w(px(0.0))
                            .max_w(px(240.0))
                            .flex_shrink(2.0)
                            .number_of_lines(1)
                            .text_right()
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(muted_fg()),
                    ),
            );
        }
        view()
            .flex_row()
            .items_center()
            .gap(px(24.0))
            .p(px(20.0))
            .rounded(px(16.0))
            .bg(move || muted())
            .child(when(covers_on, move || cover(art, Some(180.0), 12.0)))
            .child(
                view()
                    .w(px(280.0))
                    .items_start()
                    .gap(px(10.0))
                    .child(
                        view()
                            .px(px(8.0))
                            .py(px(3.0))
                            .rounded(px(6.0))
                            .bg(move || bg())
                            .child(
                                text("本地曲库")
                                    .text_size(px(12.0))
                                    .line_height(px(12.0))
                                    .text_color(muted_fg()),
                            ),
                    )
                    .child(
                        text(format!("随机翻到了「{}」", title))
                            .number_of_lines(1)
                            .text_size(px(22.0))
                            .line_height(px(22.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(fg()),
                    )
                    .child(view().h(px(6.0)))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(10.0))
                            .child(
                                button()
                                    .h(px(38.0))
                                    .px(px(18.0))
                                    .rounded_full()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.0))
                                    .bg(move || primary())
                                    .hover(|st| st.opacity(0.85))
                                    .accessibility_label("立即播放")
                                    .on_press(move |_: &()| play_list(&queue, 0))
                                    .child(icon(Icon::Play, 14.0, move || primary_fg()))
                                    .child(
                                        text("立即播放")
                                            .text_size(px(14.0))
                                            .line_height(px(14.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(primary_fg()),
                                    ),
                            )
                            .child(
                                button()
                                    .h(px(38.0))
                                    .px(px(18.0))
                                    .rounded_full()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.0))
                                    .bg(move || nsh_base::theme().secondary)
                                    .hover(|st| st.opacity(0.85))
                                    .accessibility_label("添加到队列")
                                    .on_press(move |_: &()| {
                                        queue_add(&featured.get_untracked())
                                    })
                                    .child(icon(Icon::Plus, 14.0, move || fg()))
                                    .child(
                                        text("添加到队列")
                                            .text_size(px(14.0))
                                            .line_height(px(14.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(fg()),
                                    ),
                            )
                            .child(icon_button(
                                || Icon::Shuffle,
                                15.0,
                                "换一批",
                                muted_fg,
                                reroll,
                            )),
                    ),
            )
            .child(rows)
            .into_element()
    })
    .into_element()
}

/// 热门艺术家：播放次数前 8 的歌手，大圆头像 + 名；一行四个（宽度不
/// 够 `min_w` 自动换行收列）；点到「歌手」页详情。
fn hot_artists(stats: Signal<crate::db::PlayStats>) -> AnyElement {
    let s = store();
    dynamic(move || {
        let names: Vec<String> = stats.with(|v| {
            v.top_artists.iter().map(|(n, _)| n.clone()).take(8).collect()
        });
        if names.is_empty() {
            return view().into_element();
        }
        let tracks = s.tracks.get();
        let nav = global::<Nav>();
        let sel = s.artist_sel;
        let mut row = view().flex_row().flex_wrap().gap(px(14.0));
        for name in names {
            // 头像取该歌手第一张有封面的曲目（没有则首字占位）。
            let cover_path = stored(
                tracks
                    .iter()
                    .find(|t| t.artist == name && t.has_art)
                    .map(|t| t.path.clone()),
            );
            let art_name = stored(name.clone());
            row = row.child(
                button()
                    .flex_1()
                    .min_w(px(200.0))
                    .gap(px(8.0))
                    .items_center()
                    .p(px(10.0))
                    .rounded(px(14.0))
                    .accessibility_label(SharedString::from(name.clone()))
                    .hover(|st| st.bg(hover()))
                    .active(|st| st.opacity(0.75))
                    .on_press(move |_: &()| {
                        // 深链进「歌手」详情：选好后切导航，Artists 直接开详情。
                        sel.set(Some(art_name.get()));
                        nav.set(Nav::Artists);
                    })
                    .child(when(covers_on, move || {
                        avatar(move || cover_path.get(), move || art_name.get(), Some(140.0))
                    }))
                    .child(
                        text(name.clone())
                            .number_of_lines(1)
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(muted_fg()),
                    ),
            );
        }
        view()
            .gap(px(12.0))
            .child(section_title("热门艺术家"))
            .child(row)
            .into_element()
    })
    .into_element()
}

/// 「新添加」卡：方封面 + 曲名 / 歌手，悬停出播放圆钮与「⋯」（同右键菜单）。
/// 按下以本批为队列从本卡开始播。
fn added_card(t: &TrackEntry, index: usize, list: Rc<Vec<TrackEntry>>) -> AnyElement {
    let t = t.clone();
    let path = stored(t.path.clone());
    let title = stored(t.title.clone());
    let src = move || cover_for(&path.get(), 96.0);
    let letter = move || initial_letter(&title.get(), &path.get());
    let hovering = signal(false);
    let gate = super::super::store::menu_control();
    let play_row = { let list = list.clone(); move || play_list(&list, index) };
    let play_btn = { let list = list.clone(); move || play_list(&list, index) };
    let play_menu = { move || play_list(&list, index) };
    let row = view()
        .w_full()
        .h(px(64.0))
        .flex_row()
        .items_center()
        .rounded(px(12.0))
        .overflow_hidden()
        .role(AccessibilityRole::Button)
        .label(move || SharedString::from(title.get()))
        .bg(muted())
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.85))
        .on_mouse_enter(move |_: &MouseEvent| hovering.set(true))
        .on_mouse_leave(move |_: &MouseEvent| hovering.set(false))
        .on_press(move |_: &()| play_row())
        .child(when(covers_on, move || cover_initial(src, letter, Some(64.0), 0.0)))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .pl(px(12.0))
                .pr(px(88.0))
                .gap(px(4.0))
                .child(
                    text(t.title.clone())
                        .number_of_lines(1)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg()),
                )
                .child(
                    text(t.artist.clone())
                        .number_of_lines(1)
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(muted_fg()),
                ),
        )
        // 按钮浮在右侧不占位：插 flex 子项会改卡的固有宽、引起网格回流。
        .child(when(move || hovering.get(), move || {
            view()
                .absolute()
                .top(px(0.0))
                .bottom(px(0.0))
                .right(px(10.0))
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .child(
                    button()
                        .size(px(36.0))
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .accessibility_label("播放")
                        .bg(move || nsh_base::theme().secondary)
                        .hover(|st| st.opacity(0.85))
                        .on_press({
                            let play = play_btn.clone();
                            move |_: &()| play()
                        })
                        .child(icon(Icon::Play, 16.0, fg)),
                )
                .child(icon_button(
                    || Icon::More,
                    16.0,
                    "更多",
                    muted_fg,
                    move || gate.request_open(),
                ))
                .into_element()
        }));
    let track = computed(move || Some(t.clone()));
    // flex 尺寸挂在外层：context_menu 的根是包住 trigger 的 view，网格
    // 直接排的是它——尺寸写在卡上会被菜单包装吞掉，行宽乱掉。
    view()
        .flex_1()
        .min_w(px(300.0))
        .child(track_menu_with(gate, row, track, None, play_menu, None))
        .into_element()
}

/// 「新添加」：最近入库的曲目（最多十条），一行多张卡。
fn new_added(stats: Signal<crate::db::PlayStats>) -> AnyElement {
    let s = store();
    dynamic(move || {
        let st = stats.get();
        let tracks = s.tracks.get();
        let by_path: HashMap<&str, &TrackEntry> =
            tracks.iter().map(|t| (t.path.as_str(), t)).collect();
        let list: Vec<TrackEntry> = st
            .new_added
            .iter()
            .filter_map(|p| by_path.get(p.as_str()).map(|t| (*t).clone()))
            .collect();
        if list.is_empty() {
            return view().into_element();
        }
        let queue = Rc::new(list);
        let mut grid = view().flex_row().flex_wrap().gap(px(12.0));
        for (i, t) in queue.iter().enumerate() {
            grid = grid.child(added_card(t, i, queue.clone()));
        }
        view()
            .gap(px(12.0))
            .child(section_title("新添加"))
            .child(grid)
            .into_element()
    })
    .into_element()
}

/// 最近播放：随机推荐同款卡，多行 grid；按播放时间倒序（零播放的按
/// 入库时间排在末）。整节同一个队列，点卡从本卡开始播。空榜出空态。
fn recent(stats: Signal<crate::db::PlayStats>) -> AnyElement {
    let s = store();
    dynamic(move || {
        let st = stats.get();
        let tracks = s.tracks.get();
        let by_path: HashMap<&str, &TrackEntry> =
            tracks.iter().map(|t| (t.path.as_str(), t)).collect();
        let list: Vec<TrackEntry> = st
            .recent
            .iter()
            .take(8)
            .filter_map(|p| by_path.get(p.as_str()).map(|t| (*t).clone()))
            .collect();
        if list.is_empty() {
            return view()
                .h(px(240.0))
                .child(empty_state(
                    Icon::Wave,
                    "还没有播放记录",
                    "听过的歌会在这里统计：默认听到 60% 或 30 秒算一次（设置 →「播放」→「播放统计」里可调）。拖动进度条不算听过。",
                ))
                .into_element();
        }
        let queue = Rc::new(list);
        let mut grid = view().flex_row().flex_wrap().gap(px(14.0));
        for (i, t) in queue.iter().enumerate() {
            grid = grid.child(card(t, i, queue.clone()));
        }
        view()
            .gap(px(12.0))
            .child(section_title("最近播放"))
            .child(grid)
            .into_element()
    })
    .into_element()
}

#[component]
pub fn Home() -> impl IntoElement {
    let s = store();

    // 随机推荐的一批：进页与曲库成员变化时各抽一次（批里的曲目还在
    // 曲库就不动），卡上 Shuffle 钮手动重抽。
    let featured = signal(Rc::new(Vec::<TrackEntry>::new()));
    effect(move || {
        let tracks = s.tracks.get();
        let stale = featured.with(|b| {
            b.is_empty() != tracks.is_empty()
                || b.iter().any(|t| !tracks.iter().any(|x| x.path == t.path))
        });
        if stale {
            featured.set(Rc::new(pick(&tracks, 6)));
        }
    });

    // 播放统计：进页先读一次；引擎每记一次播放（plays_gen +1）重读。
    // 两秒轮询代次——记一次的开销远小于每次整表重查。同一轮询顺手
    // 刷新 `now`：问候语跨整点、指标跨天都自动换。
    let stats = signal(crate::db::PlayStats::default());
    let now = signal(crate::config::unix_now());
    let reload = move || {
        spawn(async move {
            let now = crate::config::unix_now();
            let offset = crate::config::local_offset_seconds();
            let since = now.saturating_sub(7 * 86400);
            let week_start = crate::config::local_week_start(now, offset);
            if let Ok(v) = blocking(move || {
                crate::db::play_stats(since, week_start, offset, 10)
            })
            .await
            {
                stats.set(v);
            }
        });
    };
    reload();
    spawn(async move {
        let mut seen = player().plays_generation();
        let mut seen_added = crate::db::added_generation();
        loop {
            timer(Duration::from_secs(2)).await;
            now.set(crate::config::unix_now());
            let n = player().plays_generation();
            let a = crate::db::added_generation();
            if n != seen || a != seen_added {
                (seen, seen_added) = (n, a);
                reload();
            }
        }
    });

    view().flex_1().child(dynamic(move || {
        if s.tracks.with(|t| t.is_empty()) {
            return no_library().into_element();
        }
        view()
            .flex_1()
            .child(hero(stats, now))
            .child(scroll().flex_1().child(
                view()
                    .px(px(32.0))
                    .pb(px(BAR_SPACE))
                    .gap(px(28.0))
                    .child(featured_card(featured))
                    .child(recent(stats))
                    .child(hot_artists(stats))
                    .child(new_added(stats)),
            ))
            .into_element()
    }))
}
