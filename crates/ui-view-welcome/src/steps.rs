//! 欢迎页的前三步：欢迎、添加音乐文件夹、主题色（功能一览在 `features`）。

use nsh_core::prelude::*;

use crate::step_header;
use zeo_icons::{Icon, icon, logo};
use zeo_library::config::library_store;
use zeo_ui_components::button::{icon_button, outline_button};
use zeo_ui_store::theme::*;
use zeo_ui_store::{scan, store};
use zeo_ui_views_base::{add_dropped, last_segment, url_to_path};

/// 第 1 步（首屏大字）的标题字号。
const HERO_TITLE: f32 = 32.0;

/// 第 1 步：大 logo、标题、一句话介绍。
pub(crate) fn welcome() -> View {
    view()
        .items_center()
        .gap(px(16.0))
        .child(logo(104.0))
        .child(
            text("欢迎使用 Zeo Player")
                .text_size(px(HERO_TITLE))
                .line_height(px(HERO_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg())
                .mt(px(12.0)),
        )
        .child(
            text("本地音乐播放器：扫描你电脑上的音乐文件夹，离线播放，整理标签与歌词，数据都留在本机。")
                .text_size(px(16.0))
                .text_color(muted_fg())
                .text_center()
                .max_w(px(480.0)),
        )
}

/// 第 2 步：音乐文件夹。列表、移除、选择器与拖放同「设置 › 曲库」的
/// 「音乐文件夹」节；`message` / `picking` 由页面层持有。
pub(crate) fn folders(message: Signal<String>, picking: Signal<bool>) -> View {
    let s = store();
    let dragging = signal(false);

    let pick = move || {
        if picking.get_untracked() {
            return;
        }
        picking.set(true);
        spawn(async move {
            if let Some(result) = zeo_ui_store::pick_folders().await {
                message.set(result);
            }
            picking.set(false);
        });
    };

    let rows = dynamic(move || {
        let roots = s.library.with(|l| l.config.roots.clone());
        if roots.is_empty() {
            return view()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .h(px(136.0))
                .rounded(px(12.0))
                .border_2()
                .border_dashed()
                .border_color(move || if dragging.get() { primary() } else { border() })
                .bg(move || {
                    if dragging.get() {
                        hover()
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .child(icon(Icon::Folder, 24.0, faint))
                .child(
                    text(move || {
                        if dragging.get() {
                            "松开以添加"
                        } else {
                            "还没有文件夹。点「添加文件夹…」，或把文件夹拖到这里"
                        }
                    })
                    .text_size(px(13.0))
                    .line_height(px(13.0))
                    .text_color(muted_fg()),
                )
                .into_element();
        }
        scroll()
            .max_h(px(224.0))
            .child(
                view()
                    .gap(px(8.0))
                    .children(roots.into_iter().map(|root| root_row(root, message))),
            )
            .into_element()
    });

    let add = view()
        .flex_row()
        .style_when(move || picking.get(), |st| st.opacity(0.5))
        .child(outline_button(Icon::Plus, "添加文件夹…", pick));

    let result = dynamic(move || {
        let now = message.get();
        (!now.is_empty()).then(|| text(now).text_size(px(12.0)).text_color(muted_fg()))
    });

    view()
        .gap(px(20.0))
        .on_file_drag_enter(move || dragging.set(true))
        .on_file_drag_leave(move || dragging.set(false))
        .on_file_drop(move |event: &FileDropEvent| {
            dragging.set(false);
            let mut added = false;
            let results: Vec<String> = event
                .uris
                .iter()
                .map(|uri| {
                    let dropped = add_dropped(&url_to_path(uri));
                    added |= dropped.added;
                    dropped.note
                })
                .collect();
            if added {
                scan();
            }
            message.set(results.join("\n"));
        })
        .child(step_header(
            "添加音乐文件夹",
            "Zeo Player 会扫描这些文件夹里的 MP3、FLAC、WAV、AIFF、M4A（AAC / ALAC）、OGG，包括子文件夹。",
        ))
        .child(
            // 拖放目标的描边：平时透明，文件拖入时亮主题色。
            view()
                .rounded(px(12.0))
                .border_1()
                .border_color(move || {
                    if dragging.get() {
                        primary()
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .transition_colors()
                .duration(300.0)
                .child(rows),
        )
        .child(add)
        .child(result)
        .child(
            text("也可以稍后在 设置 › 曲库 中添加，或把文件夹拖到「文件夹」页。")
                .text_size(px(12.0))
                .text_color(faint()),
        )
}

/// 一行已添加的文件夹：名字、完整路径与移除钮（同设置里的做法：摘除后重扫）。
fn root_row(root: String, message: Signal<String>) -> View {
    let remove = root.clone();
    let name = last_segment(&root).to_owned();
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .h(px(48.0))
        .pl(px(12.0))
        .pr(px(6.0))
        .rounded(px(10.0))
        .bg(primary().opacity(0.08))
        .child(icon(Icon::Folder, 18.0, primary).flex_shrink_0())
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    text(name)
                        .number_of_lines(1)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg()),
                )
                .child(
                    text(root)
                        .number_of_lines(1)
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(icon_button(
            || Icon::Close,
            14.0,
            "移除",
            muted_fg,
            move || {
                library_store().remove_root(&remove);
                message.set(format!("已移除 {remove}"));
                scan();
            },
        ))
}

/// 第 3 步：主题色。选择器复用设置里的 `accent_section`（出厂默认是
/// 跟随封面）。
pub(crate) fn accent() -> View {
    view()
        .gap(px(24.0))
        .child(step_header(
            "选择主题色",
            "默认跟随当前曲目封面：换歌时按钮与选中态的色调跟着变。也可以固定为黑白或自定义颜色，之后随时能在 设置 › 外观 里改。",
        ))
        .child(zeo_ui_view_settings::accent_section())
}
