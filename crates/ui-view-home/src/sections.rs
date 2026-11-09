use super::*;

/// 热门艺术家：播放次数前 8 的歌手，大圆头像 + 名；单行横排、放不
/// 下横向滚动（够宽时 `flex_1` 仍摊满整行）；点到「歌手」页详情。
pub(crate) fn hot_artists(stats: Signal<zeo_db::PlayStats>) -> AnyElement {
    let s = store();
    dynamic(move || {
        let names: Vec<String> = stats.with(|v| {
            v.top_artists
                .iter()
                .map(|(n, _)| n.clone())
                .take(8)
                .collect()
        });
        if names.is_empty() {
            return view().into_element();
        }
        let tracks = s.tracks.get();
        let nav = global::<Nav>();
        let sel = s.artist_sel;
        let shelf = Shelf::new("home-artists-shelf", names.len());
        let strip = shelf.strip(|mut strip| {
            for name in names {
                // 头像取该歌手第一张有封面的曲目（没有则首字占位）。
                let cover_path = stored(
                    tracks
                        .iter()
                        .find(|t| t.artist == name && t.has_art)
                        .map(|t| t.path.clone()),
                );
                let art_name = stored(name.clone());
                strip = strip.child(
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
                            avatar(
                                move || cover_path.get(),
                                move || art_name.get(),
                                Some(140.0),
                            )
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
            strip
        });
        view()
            .gap(px(12.0))
            .child(shelf.title("热门艺术家"))
            .child(strip)
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
            .gap(px(12.0))
            .child(section_title("新添加"))
            .child(table)
            .into_element()
    })
    .into_element()
}

/// 最近播放：随机推荐同款卡，单行横排、放不下横向滚动；按播放时间
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
        let shelf = Shelf::new("home-recent-shelf", queue.len());
        let strip = shelf.strip(|mut strip| {
            for (i, t) in queue.iter().enumerate() {
                strip = strip.child(card(t, i, queue.clone()));
            }
            strip
        });
        view()
            .gap(px(12.0))
            .child(shelf.title("最近播放"))
            .child(strip)
            .into_element()
    })
    .into_element()
}
