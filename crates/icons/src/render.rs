//! 线条图标的出图：颜色写进 SVG，按（图标, 尺寸, 颜色）缓存；绑定读当前
//! 外观，切换深浅色时换图。SVG 声明的宽高就是显示尺寸：光栅化按自然尺寸出图
//! 再缩放会糊边、粗细不匀（Windows 上尤其明显）。

use std::cell::RefCell;
use std::collections::HashMap;

use nsh_core::element::Image;
use nsh_core::prelude::*;

use crate::glyph::Icon;

thread_local! {
    static CACHE: RefCell<HashMap<(Icon, u32, u32), SharedString>> = RefCell::new(HashMap::new());
}

fn hex(color: Hsla) -> (u32, String) {
    let c = color.to_rgb();
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    let (r, g, b, a) = (byte(c.r), byte(c.g), byte(c.b), byte(c.a));
    let key = (r << 24) | (g << 16) | (b << 8) | a;
    (key, format!("rgba({r},{g},{b},{:.3})", c.a))
}

pub(crate) fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b' '
            | b'='
            | b'/'
            | b':' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out.replace(' ', "%20")
}

/// `icon` in `color` as an image URI, drawn at `size` points.
pub fn icon_uri(icon: Icon, size: f32, color: Hsla) -> SharedString {
    let (key, css) = hex(color);
    CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry((icon, size.to_bits(), key))
            .or_insert_with(|| {
                let svg = format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="{size}" height="{size}" fill="none" stroke="{css}" color="{css}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
                    icon.body()
                );
                format!("data:image/svg+xml,{}", percent_encode(&svg)).into()
            })
            .clone()
    })
}

/// 一个 `size` 点的图标，颜色随 `tint`（可为绑定）。
pub fn icon(name: Icon, size: f32, tint: impl Fn() -> AdaptiveColor + 'static) -> Image {
    image(move || icon_uri(name, size, color(tint())))
        .size(px(size))
        .pointer_events(PointerEvents::None)
}

/// 图标名随状态变化（播放/暂停、静音）。
pub fn icon_dyn(
    name: impl Fn() -> Icon + 'static,
    size: f32,
    tint: impl Fn() -> AdaptiveColor + 'static,
) -> Image {
    image(move || icon_uri(name(), size, color(tint())))
        .size(px(size))
        .pointer_events(PointerEvents::None)
}
