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
            // 换源（96→400 升档）时平台先清掉旧图、等新图解码完才画，
            // 中间露出底下的占位图。把上一个源设成 placeholder：它的解码
            // 结果还在内存缓存里（按路径取，不碰磁盘——小文件升档后已
            // 删），同步就画上，新图到了再盖过去。首次没有上一个源时用
            // 当前源本身（同一请求，加载器复用）。
            let last = stored(None::<SharedString>);
            let trail = computed(move || {
                let now = src().unwrap_or_default();
                let prev = last.replace(Some(now.clone()));
                prev.filter(|p| !p.is_empty()).unwrap_or(now)
            });
            image(move || src().unwrap_or_default())
                .placeholder(move || trail.get())
                .test_id("cover-img")
                .absolute()
                .inset(px(0.0))
                .object_fit(ResizeMode::Cover)
        },
    ))
}
