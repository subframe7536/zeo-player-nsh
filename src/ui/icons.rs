//! 线条图标（24 × 24 SVG，data: URI）。颜色写进 SVG，按（图标, 颜色）缓存；
//! 绑定读当前外观，切换深浅色时换图。

use std::cell::RefCell;
use std::collections::HashMap;

use nsh_core::element::Image;
use nsh_core::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
    Volume,
    Mute,
    Folder,
    Mic,
    Disc,
    Tag,
    Settings,
    ChevronDown,
    ChevronRight,
    ChevronLeft,
    Close,
    Expand,
    Note,
    Plus,
    Person,
    Refresh,
    Wave,
    PlayLine,
    PauseLine,
    PrevLine,
    NextLine,
    More,
    Reveal,
    Copy,
    Queue,
}

impl Icon {
    fn body(self) -> &'static str {
        match self {
            Icon::Play => r#"<path d="M8 5.2v13.6a1 1 0 0 0 1.5.86l11-6.8a1 1 0 0 0 0-1.72l-11-6.8A1 1 0 0 0 8 5.2z" fill="currentColor" stroke="none"/>"#,
            Icon::Pause => r#"<rect x="6" y="4.5" width="4" height="15" rx="1.2" fill="currentColor" stroke="none"/><rect x="14" y="4.5" width="4" height="15" rx="1.2" fill="currentColor" stroke="none"/>"#,
            Icon::Prev => r#"<path d="M18.5 5.6v12.8a.8.8 0 0 1-1.25.66L8.6 12.66a.8.8 0 0 1 0-1.32l8.65-6.4a.8.8 0 0 1 1.25.66z" fill="currentColor" stroke="none"/><rect x="5" y="5" width="2.4" height="14" rx="1.2" fill="currentColor" stroke="none"/>"#,
            Icon::Next => r#"<path d="M5.5 5.6v12.8a.8.8 0 0 0 1.25.66l8.65-6.4a.8.8 0 0 0 0-1.32L6.75 4.94a.8.8 0 0 0-1.25.66z" fill="currentColor" stroke="none"/><rect x="16.6" y="5" width="2.4" height="14" rx="1.2" fill="currentColor" stroke="none"/>"#,
            Icon::Volume => r#"<path d="M4 9.5v5h3.5L12 18V6L7.5 9.5z" fill="currentColor" stroke="none"/><path d="M15.5 9a4.2 4.2 0 0 1 0 6M18 6.5a8 8 0 0 1 0 11"/>"#,
            Icon::Mute => r#"<path d="M4 9.5v5h3.5L12 18V6L7.5 9.5z" fill="currentColor" stroke="none"/><path d="M16 9.5l5 5M21 9.5l-5 5"/>"#,
            Icon::Folder => r#"<path d="M3.5 7a1.5 1.5 0 0 1 1.5-1.5h4l2 2.5h8A1.5 1.5 0 0 1 20.5 9.5V17a1.5 1.5 0 0 1-1.5 1.5H5A1.5 1.5 0 0 1 3.5 17z"/>"#,
            Icon::Mic => r#"<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21M8.5 21h7"/>"#,
            Icon::Disc => r#"<circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="2.2" fill="currentColor" stroke="none"/>"#,
            Icon::Tag => r#"<path d="M3.5 11V4.5A1 1 0 0 1 4.5 3.5H11l9.5 9.5-7 7z"/><circle cx="7.8" cy="7.8" r="1.3" fill="currentColor" stroke="none"/>"#,
            Icon::Settings => r#"<path d="M4 7h10M18 7h2M4 12h4M12 12h8M4 17h13"/><circle cx="16" cy="7" r="2"/><circle cx="10" cy="12" r="2"/><circle cx="19" cy="17" r="2"/>"#,
            Icon::ChevronDown => r#"<path d="M6 9.5l6 6 6-6"/>"#,
            Icon::ChevronRight => r#"<path d="M9.5 6l6 6-6 6"/>"#,
            Icon::ChevronLeft => r#"<path d="M14.5 6l-6 6 6 6"/>"#,
            Icon::Close => r#"<path d="M6 6l12 12M18 6L6 18"/>"#,
            Icon::Expand => r#"<path d="M15 4h5v5M9 20H4v-5M20 4l-6.5 6.5M4 20l6.5-6.5"/>"#,
            Icon::Note => r#"<path d="M9 17.5V5l10-2v11.5"/><circle cx="6.5" cy="17.5" r="2.5"/><circle cx="16.5" cy="14.5" r="2.5"/>"#,
            Icon::Person => r#"<circle cx="12" cy="8" r="3.5"/><path d="M5 20a7 7 0 0 1 14 0"/>"#,
            Icon::Plus => r#"<path d="M12 5v14M5 12h14"/>"#,
            Icon::Refresh => r#"<path d="M20 11a8 8 0 0 0-14.3-4.9L4 8M4 4v4h4M4 13a8 8 0 0 0 14.3 4.9L20 16M20 20v-4h-4"/>"#,
            Icon::PlayLine => r#"<path d="M7.5 5.3v13.4a.9.9 0 0 0 1.37.77l10.7-6.7a.9.9 0 0 0 0-1.54L8.87 4.53A.9.9 0 0 0 7.5 5.3z"/>"#,
            Icon::PauseLine => r#"<path d="M8.5 5v14M15.5 5v14"/>"#,
            Icon::PrevLine => r#"<path d="M6 5.5v13M18 6.2v11.6a.7.7 0 0 1-1.1.57L9.4 12.57a.7.7 0 0 1 0-1.14l7.5-5.8a.7.7 0 0 1 1.1.57z"/>"#,
            Icon::NextLine => r#"<path d="M18 5.5v13M6 6.2v11.6a.7.7 0 0 0 1.1.57l7.5-5.8a.7.7 0 0 0 0-1.14L7.1 5.63A.7.7 0 0 0 6 6.2z"/>"#,
            Icon::More => r#"<circle cx="5.5" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="18.5" cy="12" r="1.4" fill="currentColor" stroke="none"/>"#,
            Icon::Reveal => r#"<path d="M3.5 7a1.5 1.5 0 0 1 1.5-1.5h4l2 2.5h8A1.5 1.5 0 0 1 20.5 9.5V17a1.5 1.5 0 0 1-1.5 1.5H5A1.5 1.5 0 0 1 3.5 17z"/><path d="M12 11v5M9.5 13.5 12 11l2.5 2.5"/>"#,
            Icon::Queue => r#"<path d="M4 6.5h12M4 11.5h12M4 16.5h7"/><path d="M15.5 14.5v5l4-2.5z" fill="currentColor" stroke="none"/>"#,
            Icon::Copy => r#"<rect x="8.5" y="8.5" width="11" height="11" rx="2"/><path d="M15.5 8.5V6a1.5 1.5 0 0 0-1.5-1.5H6A1.5 1.5 0 0 0 4.5 6v8A1.5 1.5 0 0 0 6 15.5h2.5"/>"#,
            Icon::Wave => r#"<path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 11v2"/>"#,
        }
    }
}

thread_local! {
    static CACHE: RefCell<HashMap<(Icon, u32), SharedString>> = RefCell::new(HashMap::new());
}

fn hex(color: Hsla) -> (u32, String) {
    let c = color.to_rgb();
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    let (r, g, b, a) = (byte(c.r), byte(c.g), byte(c.b), byte(c.a));
    let key = (r << 24) | (g << 16) | (b << 8) | a;
    (key, format!("rgba({r},{g},{b},{:.3})", c.a))
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b' ' | b'=' | b'/' | b':' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out.replace(' ', "%20")
}

/// `icon` in `color` as an image URI.
pub fn icon_uri(icon: Icon, color: Hsla) -> SharedString {
    let (key, css) = hex(color);
    CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry((icon, key))
            .or_insert_with(|| {
                let svg = format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="48" height="48" fill="none" stroke="{css}" color="{css}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
                    icon.body()
                );
                format!("data:image/svg+xml,{}", percent_encode(&svg)).into()
            })
            .clone()
    })
}

/// 一个 `size` 点的图标，颜色随 `tint`（可为绑定）。
pub fn icon(name: Icon, size: f32, tint: impl Fn() -> AdaptiveColor + 'static) -> Image {
    image(move || icon_uri(name, color(tint())))
        .size(px(size))
        .pointer_events(PointerEvents::None)
}

/// 图标名随状态变化（播放/暂停、静音）。
pub fn icon_dyn(
    name: impl Fn() -> Icon + 'static,
    size: f32,
    tint: impl Fn() -> AdaptiveColor + 'static,
) -> Image {
    image(move || icon_uri(name(), color(tint())))
        .size(px(size))
        .pointer_events(PointerEvents::None)
}
