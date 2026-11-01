//! 封面：图片或占位（图标 + 柔和底色）。

use nsh_core::prelude::*;

use super::super::theme::*;
use super::icons::{Icon, icon};

/// 封面：图片或占位（音符 + 柔和底色）。`size` 为 `None` 时铺满宽度、正方形。
pub fn cover(
    src: impl Fn() -> Option<SharedString> + Copy + 'static,
    size: Option<f32>,
    radius: f32,
) -> View {
    cover_frame(src, size, radius, Icon::Note)
}

/// 固定大小的封面，占位图标为 `placeholder`。
pub fn cover_with(
    src: impl Fn() -> Option<SharedString> + Copy + 'static,
    size: f32,
    radius: f32,
    placeholder: Icon,
) -> View {
    cover_frame(src, Some(size), radius, placeholder)
}

fn cover_frame(
    src: impl Fn() -> Option<SharedString> + Copy + 'static,
    size: Option<f32>,
    radius: f32,
    placeholder: Icon,
) -> View {
    let frame = view()
        .rounded(px(radius))
        .overflow_hidden()
        .bg(muted())
        .items_center()
        .justify_center();
    let frame = match size {
        Some(size) => frame.size(px(size)),
        None => frame.w_full().aspect_square(),
    };
    let glyph = size.map_or(40.0, |s| (s * 0.38).clamp(14.0, 96.0));
    frame.child(icon(placeholder, glyph, faint)).child(when(
        move || src().is_some(),
        move || {
            image(move || src().unwrap_or_default())
                .absolute()
                .inset(px(0.0))
                .resize_mode(ResizeMode::Cover)
        },
    ))
}
