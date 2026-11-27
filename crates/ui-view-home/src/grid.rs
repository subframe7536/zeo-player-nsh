use super::*;

/// 网格容器宽：内容两侧各让 `PAGE_X - CARD_PAD`（封面与节标题同一条内容
/// 线，卡的悬停底色向外多出一圈）。
fn grid_width() -> f32 {
    content_break() - 2.0 * (PAGE_X - CARD_PAD)
}

/// 当前窗口宽度下的列数：每列至少 [`MIN_CARD_W`]，2–8 列。`content_break`
/// 跟着窗口宽度实时变（含拖动缩放途中），列数与卡宽随之重算。
fn grid_cols() -> f32 {
    (grid_width() / MIN_CARD_W).floor().clamp(2.0, 8.0)
}

/// 一张卡的宽：列宽取整并让出 1 pt，避免浮点累计误差把最后一列挤到下一行。
fn card_w() -> f32 {
    (grid_width() / grid_cols()).floor() - 1.0
}

/// 响应式卡片网格：卡宽按列数均分、一行放不下折到下一行（纵向增长，
/// 不横向滚动）。`mx(-CARD_PAD)` 抵掉卡的内边距，封面与节标题落在同一
/// 条内容线上。
pub(crate) fn card_grid(id: &'static str) -> View {
    view().flex_row().flex_wrap().mx(px(-CARD_PAD)).test_id(id)
}

/// 网格里的一张卡的外壳：宽随列数变（封面铺满、正方形）、悬停底色铺满
/// 整卡。
pub(crate) fn card_shell(label: String) -> ButtonElement {
    button()
        .w(move || px(card_w()))
        .flex_shrink_0()
        .gap(px(CARD_ART_GAP))
        .items_start()
        .p(px(CARD_PAD))
        .rounded(px(CARD_RADIUS))
        .accessibility_label(SharedString::from(label))
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.75))
}

/// 推荐卡：封面 + 曲名 + 歌手；按下以这一批为队列从本卡开始播。
pub(crate) fn card(t: &TrackEntry, index: usize, five: Rc<Vec<TrackEntry>>) -> ButtonElement {
    // `src` / `letter` 进 `when` 的响应闭包：要 `Copy`，用 `stored` 持有。
    let path = stored(t.path.clone());
    let title = stored(t.display_title().to_owned());
    let src = move || cover_for(&path.get(), CARD_ART);
    let letter = move || initial_letter(&title.get(), &path.get());
    card_shell(t.display_title().to_owned())
        .on_press(move |_: &()| play_list(&five, index))
        .child(when(covers_on, move || {
            cover_initial(src, letter, None, CARD_ART_RADIUS)
        }))
        .child(
            view()
                .w_full()
                .px(px(2.0))
                .gap(px(STACK_GAP))
                .child(
                    text(t.display_title().to_owned())
                        .number_of_lines(1)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg()),
                )
                .child(
                    text(t.display_artist().to_owned())
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
pub(crate) fn featured_card(featured: Signal<Rc<Vec<TrackEntry>>>) -> AnyElement {
    dynamic(move || {
        let list = featured.get();
        let Some(first) = list.first() else {
            return view().into_element();
        };
        let path = stored(first.path.clone());
        let art = move || cover_for(&path.get(), 400.0);
        let title = first.display_title().to_owned();
        let queue = list.clone();
        let reroll =
            move || featured.set(Rc::new(pick(&store().tracks.get_untracked(), FEATURED_N)));
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
            .style_when(move || content_break() < 880.0, |s| s.hidden());
        for (i, t) in list.iter().take(FEATURED_N).enumerate() {
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
                    .label(SharedString::from(t.display_title().to_owned()))
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
                        text(row_track.display_title().to_owned())
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
                        text(row_track.display_artist().to_owned())
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
        // 信息列 `flex_1`：右侧列表在窄窗口隐藏时它吃满余宽，卡里不留
        // 一大块空白；宽窗口下与列表平分，列表另有 40% 封顶。
        view()
            .flex_row()
            .items_center()
            .gap(px(28.0))
            .p(px(24.0))
            .rounded(px(20.0))
            .bg(primary().opacity(0.08))
            .child(when(covers_on, move || {
                cover(art, Some(176.0), 12.0).shadow_md()
            }))
            .child(
                view()
                    .flex_1()
                    .min_w(px(0.0))
                    .items_start()
                    .gap(px(12.0))
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
                            .number_of_lines(2)
                            .text_size(px(24.0))
                            .line_height(px(30.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(fg()),
                    )
                    .child(view().h(px(4.0)))
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
                                    .on_press(move |_: &()| queue_add(&featured.get_untracked()))
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
