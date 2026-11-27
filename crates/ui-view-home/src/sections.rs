use super::*;

/// `artist_plays`（歌手标签原文、播放次数、最近播放时间）按艺术家分隔符
/// 拆开合并：同名累计次数、取最近时间，按次数降序、时间降序、名字升序
/// 取前 `n` 个名。
pub(crate) fn rank_artists(
    rows: &[(String, u64, u64)],
    separators: &[String],
    n: usize,
) -> Vec<String> {
    let mut map: HashMap<String, (u64, u64)> = HashMap::new();
    for (raw, plays, last) in rows {
        for name in zeo_library::config::split_artists(raw, separators) {
            let e = map.entry(name).or_insert((0, 0));
            e.0 += plays;
            e.1 = e.1.max(*last);
        }
    }
    let mut out: Vec<(String, u64, u64)> = map
        .into_iter()
        .map(|(name, (plays, last))| (name, plays, last))
        .collect();
    out.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.0.cmp(&b.0))
    });
    out.truncate(n);
    out.into_iter().map(|(name, _, _)| name).collect()
}

/// 热门艺术家：`artist_plays` 拆分合并后播放多的前 8，卡上是居中头像 +
/// 名 +「N 首」（含该歌手的曲目数）；固定宽卡排成网格、放不下折行；
/// 点卡进「艺术家」详情。
pub(crate) fn hot_artists(stats: Signal<zeo_db::PlayStats>) -> AnyElement {
    let s = store();
    dynamic(move || {
        let names = stats.with(|v| {
            let seps = s.artist_seps.get();
            rank_artists(&v.artist_plays, &seps, 8)
        });
        if names.is_empty() {
            return view().into_element();
        }
        let tracks = s.tracks.get();
        let grid = card_grid("home-artists-grid").children(names.into_iter().map(|name| {
            let mine = |t: &&TrackEntry| t.artists.contains(&name);
            // 头像取含该歌手且有封面的第一首曲目（没有则首字占位）。
            let cover_path = stored(
                tracks
                    .iter()
                    .find(|t| mine(t) && t.has_art)
                    .map(|t| t.path.clone()),
            );
            let count = tracks.iter().filter(|t| mine(t)).count();
            let art_name = stored(name.clone());
            let press_name = name.clone();
            card_shell(name.clone())
                .items_center()
                .on_press(move |_: &()| zeo_ui_store::open_artist(&press_name))
                .child(when(covers_on, move || {
                    avatar(move || cover_path.get(), move || art_name.get(), None)
                }))
                .child(
                    view()
                        .w_full()
                        .items_center()
                        .gap(px(STACK_GAP))
                        .child(
                            text(name.clone())
                                .number_of_lines(1)
                                .text_size(px(14.0))
                                .line_height(px(14.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(fg()),
                        )
                        .child(
                            text(format!("{count} 首"))
                                .number_of_lines(1)
                                .text_size(px(12.0))
                                .line_height(px(12.0))
                                .text_color(muted_fg()),
                        ),
                )
        }));
        view()
            .gap(px(16.0))
            .child(section_title("热门艺术家"))
            .child(grid)
            .into_element()
    })
    .into_element()
}

/// 「新添加」：最近入库的曲目（最多十条），与曲库页相同的曲目列表
/// （列头 + `track_row` 行，双击播放、右键菜单相同）。节在 px(32) 栏
/// 里，`mx(-12)` 抵消出 20 的行距——与曲库页 `mx(20)` + 行内
/// `px(12)` 对齐到同一内容线。
pub(crate) fn new_added(stats: Signal<zeo_db::PlayStats>) -> AnyElement {
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
        // `mx(-12)` 把行距抵成 20（与曲库页 `mx(20)` + 行内 `px(12)`
        // 同一内容线）；列头用它覆盖自带的 `mx(20)`。
        let mut table = view().child(track_columns(Columns::ALL).mx(px(-12.0)));
        for (i, t) in queue.iter().enumerate() {
            let track = {
                let t = t.clone();
                computed(move || Some(t.clone()))
            };
            let number = computed(move || (i + 1).to_string());
            let play = Rc::new({
                let queue = queue.clone();
                move || play_list(&queue, i)
            });
            let row = track_row(track, number, Columns::ALL, {
                let play = play.clone();
                move || play()
            })
            .mx(px(-12.0));
            table = table.child(track_menu(row, track, None, move || play(), None));
        }
        view()
            .gap(px(16.0))
            .child(section_title("新添加"))
            .child(table)
            .into_element()
    })
    .into_element()
}

/// 最近播放：随机推荐同款卡，排成网格、放不下折行；按播放时间
/// 倒序（零播放的按入库时间排在末）。整节同一个队列，点卡从本卡开
/// 始播。空榜出空态。
pub(crate) fn recent(stats: Signal<zeo_db::PlayStats>) -> AnyElement {
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
        let grid = card_grid("home-recent-grid").children(
            queue
                .iter()
                .enumerate()
                .map(|(i, t)| card(t, i, queue.clone())),
        );
        view()
            .gap(px(16.0))
            .child(section_title("最近播放"))
            .child(grid)
            .into_element()
    })
    .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(v: &[(&str, u64, u64)]) -> Vec<(String, u64, u64)> {
        v.iter()
            .map(|(a, c, l)| ((*a).to_owned(), *c, *l))
            .collect()
    }

    #[test]
    fn rank_artists_merges_split_names() {
        let seps = vec![";".to_owned()];
        // "A; B" 的两次播放同时计给 A 与 B；"A" 再加 2 次、最近 20。
        let r = rows(&[("A; B", 3, 10), ("A", 2, 20), ("B", 1, 5)]);
        assert_eq!(rank_artists(&r, &seps, 8), vec!["A", "B"]);
    }

    #[test]
    fn rank_artists_tie_breaks() {
        // 次数相同比最近时间，再比名字。
        let r = rows(&[("B", 2, 5), ("A", 2, 5), ("C", 2, 9)]);
        assert_eq!(rank_artists(&r, &[], 8), vec!["C", "A", "B"]);
    }

    #[test]
    fn rank_artists_skips_empty_and_limits() {
        let seps = vec![";".to_owned()];
        let r = rows(&[("", 5, 3), (" ; ", 4, 2), ("A", 1, 1), ("B", 1, 2)]);
        assert_eq!(rank_artists(&r, &seps, 1), vec!["B"]);
        assert_eq!(rank_artists(&r, &seps, 8), vec!["B", "A"]);
    }
}
