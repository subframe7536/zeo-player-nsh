//! 封面：图片或占位（图标 / 首字 + 柔和底色）。

use nsh_core::prelude::*;

use super::icons::{Icon, icon};
use zeo_ui_store::theme::*;

/// 封面：图片或占位（音符 + 柔和底色）。`size` 为 `None` 时铺满宽度、正方形。
pub fn cover(
    src: impl Fn() -> Option<SharedString> + Copy + 'static,
    size: Option<f32>,
    radius: f32,
) -> View {
    cover_frame(src, size, radius, move |g| icon(Icon::Note, g, faint))
}

/// 封面：图片或首字占位（`letter` 给标题 / 文件名的首字符）。
/// `size` 为 `None` 时铺满宽度、正方形。
pub fn cover_initial(
    src: impl Fn() -> Option<SharedString> + Copy + 'static,
    letter: impl Fn() -> String + 'static,
    size: Option<f32>,
    radius: f32,
) -> View {
    cover_frame(src, size, radius, move |g| {
        text(letter)
            .text_size(px(g))
            .line_height(px(g))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(muted_fg())
    })
}

/// 首字占位取哪个字符：标题首字符；标题为空退回文件名（去扩展名末段）首字符。
pub fn initial_letter(title: &str, path: &str) -> String {
    title
        .chars()
        .next()
        .map(String::from)
        .or_else(|| {
            std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.chars().next())
                .map(String::from)
        })
        .unwrap_or_default()
}

fn cover_frame<E: IntoElement>(
    src: impl Fn() -> Option<SharedString> + Copy + 'static,
    size: Option<f32>,
    radius: f32,
    glyph: impl FnOnce(f32) -> E + 'static,
) -> View {
    let frame = view()
        .test_id("cover")
        .rounded(px(radius))
        .overflow_hidden()
        .bg(muted())
        .items_center()
        .justify_center();
    let frame = match size {
        Some(size) => frame.size(px(size)),
        None => frame.w_full().aspect_square(),
    };
    let g = size.map_or(40.0, |s| (s * 0.38).clamp(14.0, 96.0));
    frame.child(glyph(g)).child(when(
        move || src().is_some(),
        move || {
            image(move || src().unwrap_or_default())
                .test_id("cover-img")
                .absolute()
                .inset(px(0.0))
                .object_fit(ResizeMode::Cover)
        },
    ))
}
