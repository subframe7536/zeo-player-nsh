//! 「设置」弹窗：左栏是搜索框与分类，右栏是所选分类的各节（曲库 / 播放 /
//! 歌词 / 窗口 / 其他 / 外观）。各节无卡片包装，靠间距分开。

mod catalog;
mod nav;
#[cfg(test)]
mod tests;
/// 右栏：当前分类的标题与关闭按钮，下面是它的各节；有搜索词时只显示
/// 命中的节，所选分类没命中时落到第一个有命中的分类。
mod widgets;

pub(crate) use catalog::*;
pub(crate) use nav::*;
pub(crate) use widgets::*;

mod appearance;
mod covers;
mod library;
mod lyrics;
mod other;
mod playback;
mod window;

use nsh_core::element::input;
use nsh_core::prelude::*;

use zeo_ui_components::button::icon_button;
use zeo_ui_components::icons::{Icon, icon};
pub(crate) use zeo_ui_store::Pane;
use zeo_ui_store::store;
use zeo_ui_store::theme::*;

/// 设置里的一节：标题、说明、右上角操作，然后是内容（无卡片包装，
/// 节与节之间靠页面间距分开）。

fn content_column(pane: Signal<Pane>, query: Signal<String>) -> View {
    let s = store();
    let shown = move || {
        let q = query.get().trim().to_lowercase();
        let hits: Vec<Pane> = Pane::all().filter(|p| pane_hits(&q, *p)).collect();
        let cur = pane.get();
        hits.iter()
            .find(|&&p| p == cur)
            .or(hits.first())
            .copied()
            .map(|p| (p, q))
    };
    view()
        .flex_1()
        .min_w(px(0.0))
        .test_id("settings-content")
        .child(dynamic(move || {
            let Some((p, q)) = shown() else {
                return view()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .p(px(32.0))
                    .child(icon(Icon::Search, 24.0, faint))
                    .child(
                        text("没有匹配的设置")
                            .text_size(px(14.0))
                            .line_height(px(14.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg()),
                    )
                    .child(
                        text("换个关键词，或清空搜索看全部分类。")
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .text_color(muted_fg()),
                    )
                    .into_element();
            };
            // 节的构建放进 untracked：build 里有顺手读信号初始化草稿的
            // （如 playback 的 fade_ms / replaygain 的 db_row），被 dynamic
            // 记成依赖后任何 `s.library` 写都会整栏重建——拖动中的推子节点
            // 被换掉、responder 随之丢失（均衡器推子曾因此在滚动区里拖不动）。
            untracked(move || {
                view()
                    .flex_1()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .h(px(52.0))
                            .pl(px(24.0))
                            .pr(px(14.0))
                            .border_b_1()
                            .border_b_color(border())
                            .child(
                                text(p.title())
                                    .text_size(px(15.0))
                                    .line_height(px(15.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(fg()),
                            )
                            .child(view().flex_1())
                            .child(icon_button(
                                || Icon::Close,
                                14.0,
                                "关闭设置",
                                muted_fg,
                                move || s.settings.set(false),
                            )),
                    )
                    .child(
                        scroll().flex_1().child(
                            view()
                                .px(px(24.0))
                                .pt(px(20.0))
                                .pb(px(28.0))
                                .gap(px(36.0))
                                .children(
                                    SECTIONS
                                        .iter()
                                        .filter(|sec| sec.pane == p && section_hits(&q, sec))
                                        .map(|sec| (sec.build)()),
                                ),
                        ),
                    )
                    .into_element()
            })
        }))
}

/// 设置弹窗（nsh-base `dialog`，Esc / 点遮罩关闭）。
pub fn dialog() -> AnyElement {
    let pane = global::<Pane>();
    let query = signal(String::new());
    let content = view()
        .w(px(920.0))
        .max_w(move || px((window().width.0 - 32.0).max(400.0)))
        .h(px(640.0))
        .max_h(move || px((window().height.0 - 60.0).max(400.0)))
        .flex_row()
        .overflow_hidden()
        .rounded(px(zeo_ui_components::form::RADIUS_LG))
        .border_1()
        .border_color(border())
        .bg(move || bg())
        .transition_colors()
        .duration(300.0)
        .shadow_lg()
        // 点在内容上不会穿到背后的遮罩。
        .on_press(|| {})
        .child(nav_column(pane, query))
        .child(view().w(px(1.0)).bg(border()).flex_shrink_0())
        .child(content_column(pane, query));
    zeo_ui_components::form::modal(content, move || store().settings.set(false))
}

/// 打开设置弹窗并选中 `pane`（侧栏「设置」只置 `Store::settings`，保留
/// 上次的选择；空曲库的「添加文件夹…」要落在曲库分类所以走这里）。
pub fn open(pane: Pane) {
    zeo_ui_store::open_settings(pane)
}
