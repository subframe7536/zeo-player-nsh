use super::*;

/// 横向卡条的翻页状态：`el` 指到条子本体，`offset` 随 `on_scroll`、
/// `viewport` 随 `on_layout` 刷新。`min_content` 是所有固定宽卡排开
/// 的内容宽——它不超过视口时整行放得下、条子不滚。
#[derive(Clone, Copy)]
pub(crate) struct Shelf {
    pub(crate) el: ElementRef,
    pub(crate) offset: Signal<f32>,
    pub(crate) viewport: Signal<f32>,
    pub(crate) min_content: f32,
    pub(crate) id: &'static str,
}

impl Shelf {
    pub(crate) fn new(id: &'static str, cards: usize) -> Self {
        Self {
            el: element_ref(),
            offset: signal(0.0),
            viewport: signal(0.0),
            min_content: cards as f32 * (STRIP_MIN_W + STRIP_GAP) - STRIP_GAP,
            id,
        }
    }

    /// 滚得到的最大 offset；≤ 0 即整行放得下。
    pub(crate) fn max(&self) -> f32 {
        (self.min_content - self.viewport.get()).max(0.0)
    }

    /// 节标题行：左标题，右侧 ◀ ▶ 翻页钮（只在滚得动时出现，到边
    /// 禁用）；点一下滚近一屏、留一截衔接。竖向滚轮不归它——嵌套
    /// 滚动按轴选，滚轮仍是整页滚动，翻页钮是鼠标够到后半截的
    /// 唯一入口（横向手势 / 触控板横滑也能滚）。
    pub(crate) fn title(&self, title: &'static str) -> View {
        let this = *self;
        view()
            .flex_row()
            .items_center()
            .child(section_title(title))
            .child(view().flex_1())
            .child(when(
                move || this.max() > 0.5,
                move || {
                    let page = move || this.viewport.get() * 0.9;
                    view()
                        .flex_row()
                        .gap(px(2.0))
                        .child(
                            icon_button(
                                || Icon::ChevronLeft,
                                14.0,
                                "往前翻",
                                muted_fg,
                                move || {
                                    let x = (this.offset.get() - page()).max(0.0);
                                    this.el.scroll_to(point(Points(x), Points::ZERO), true);
                                },
                            )
                            .disabled(move || this.offset.get() <= 0.5),
                        )
                        .child(
                            icon_button(
                                || Icon::ChevronRight,
                                14.0,
                                "往后翻",
                                muted_fg,
                                move || {
                                    let x = (this.offset.get() + page()).min(this.max());
                                    this.el.scroll_to(point(Points(x), Points::ZERO), true);
                                },
                            )
                            .disabled(move || this.offset.get() >= this.max() - 0.5),
                        )
                },
            ))
    }

    /// 横向 Scroll：一行横排、藏滚动条，固定宽的卡从左排起。`mx(-CARD_PAD)`
    /// 抵掉卡的内边距，封面与节标题落在同一条内容线上（悬停底色向外
    /// 多出一圈）。`fill` 把卡塞进来。
    pub(crate) fn strip(&self, fill: impl FnOnce(Scroll) -> Scroll) -> Scroll {
        let this = *self;
        fill(
            scroll()
                .horizontal()
                .scrollbar(false)
                .mx(px(-CARD_PAD))
                .gap(px(STRIP_GAP))
                .element_ref(&this.el)
                .test_id(this.id)
                .on_layout(move |b: &Bounds<Points>| this.viewport.set(b.size.width.0))
                .on_scroll(move |e: &ScrollEvent| this.offset.set(e.offset.x.0)),
        )
    }
}

/// 卡条里的一张卡的外壳：固定宽（封面边长 + 内边距）、左对齐、悬停
/// 底色铺满整卡。卡条从左排起，放不下横向滚动。
pub(crate) fn card_shell(label: String) -> ButtonElement {
    button()
        .w(px(STRIP_MIN_W))
        .flex_shrink_0()
        .gap(px(10.0))
        .items_start()
        .p(px(CARD_PAD))
        .rounded(px(12.0))
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
            cover_initial(src, letter, Some(CARD_ART), 8.0)
        }))
        .child(
            view()
                .w_full()
                .px(px(2.0))
                .gap(px(6.0))
                .child(
                    text(t.display_title().to_owned())
                        .number_of_lines(1)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
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
            .style_when(move || content_break() < 880.0, |s| s.hidden());
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
