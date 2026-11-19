//! 歌手名链接：曲目行副标题、播放条副标题与全屏播放页共用的可点歌手名——
//! 点名字进「艺术家」详情，" / " 分隔符不可点。

use nsh_core::prelude::*;

use zeo_ui_store::open_artist;
use zeo_ui_store::theme::*;

/// 歌手链接行（主题色版）：单行、`flex_row`、`min_w(0)`、超出裁剪；
/// 名字 muted、悬停提亮、按下进艺术家页。空列表画 "Unknown Artist"。
pub fn artist_links(artists: impl Fn() -> Vec<String> + 'static, size: f32) -> AnyElement {
    artist_links_colored(artists, size, muted_fg, fg)
}

/// 同 [`artist_links`]，文字色自定（全屏播放页用半透明白）。
pub fn artist_links_colored(
    artists: impl Fn() -> Vec<String> + 'static,
    size: f32,
    normal: impl Fn() -> AdaptiveColor + Copy + 'static,
    hover_color: impl Fn() -> AdaptiveColor + Copy + 'static,
) -> AnyElement {
    dynamic(move || {
        let names = artists();
        if names.is_empty() {
            return text("Unknown Artist")
                .text_size(px(size))
                .line_height(px(size))
                .text_color(normal())
                .into_element();
        }
        let mut row = view()
            .flex_row()
            .items_center()
            .min_w(px(0.0))
            .overflow_hidden();
        for (i, name) in names.iter().enumerate() {
            if i > 0 {
                row = row.child(
                    text("/")
                        .text_size(px(size))
                        .line_height(px(size))
                        .text_color(move || normal().opacity(0.45))
                        .flex_shrink_0()
                        .mx(px(4.0)),
                );
            }
            let hov = signal(false);
            let press = name.clone();
            let label = name.clone();
            row = row.child(
                view()
                    .min_w(px(0.0))
                    .cursor(Cursor::Pointer)
                    .role(AccessibilityRole::Button)
                    .label(SharedString::from(name.clone()))
                    .on_mouse_enter(move |_: &MouseEvent| hov.set(true))
                    .on_mouse_leave(move |_: &MouseEvent| hov.set(false))
                    .on_press(move |_: &()| open_artist(&press))
                    .child(
                        text(label)
                            .number_of_lines(1)
                            .text_size(px(size))
                            .line_height(px(size))
                            .text_color(move || if hov.get() { hover_color() } else { normal() }),
                    ),
            );
        }
        row.into_element()
    })
    .into_element()
}
