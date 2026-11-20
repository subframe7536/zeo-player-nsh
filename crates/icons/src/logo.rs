//! 应用图标（标题栏 logo）。

use nsh_core::element::Image;
use nsh_core::prelude::*;

use crate::render::percent_encode;

/// 应用图标的 SVG 版：直接用 resources/icon.svg，与应用图标同一份图形。
/// 源文件根元素声明 1024 见方；把根标签换成带 viewBox 的显示尺寸
/// （见 `render` 的文件头注释：声明尺寸即光栅化尺寸，按 1024 出图再缩到十几点会糊）。
fn logo_svg(size: f32) -> SharedString {
    let svg = include_str!("../../../resources/icon.svg").replacen(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1024\" height=\"1024\" fill=\"none\">",
        &format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1024 1024\" width=\"{size}\" height=\"{size}\" fill=\"none\">"),
        1,
    );
    format!("data:image/svg+xml,{}", percent_encode(&svg)).into()
}

/// 应用图标（标题栏）：`size` 点的 logo。
pub fn logo(size: f32) -> Image {
    image(logo_svg(size))
        .size(px(size))
        .pointer_events(PointerEvents::None)
}
