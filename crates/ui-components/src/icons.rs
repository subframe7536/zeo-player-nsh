//! 线条图标（24 × 24 SVG，data: URI）：基础图标用 Lucide（见 `Icon::body`），
//! 播放 / 暂停键是自己画的形变图标（`play_pause`），Windows 窗口按钮按系统
//! 按钮的尺寸画。颜色写进 SVG，按（图标, 尺寸, 颜色）
//! 缓存；绑定读当前外观，切换深浅色时换图。SVG 声明的宽高就是显示尺寸：
//! 光栅化按自然尺寸出图再缩放会糊边、粗细不匀（Windows 上尤其明显）。

use std::cell::RefCell;
use std::collections::HashMap;

use nsh_core::element::Image;
use nsh_core::prelude::*;

/// 形变时长与缓动（easeOutQuart 的近似）。
const MORPH_MS: f32 = 320.0;
const MORPH_EASE: Easing = Easing::Bezier(0.25, 1.0, 0.5, 1.0);
use zeo_icons::{MORPH_STROKE, PAUSE_SHAPES, PLAY_SHAPES};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Play,
    Volume,
    Mute,
    Home,
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
    Minus,
    Person,
    Refresh,
    Wave,
    PrevLine,
    NextLine,
    More,
    Reveal,
    Copy,
    Queue,
    Search,
    Repeat,
    RepeatOne,
    Shuffle,
    Playlist,
    Smart,
    Trash,
    Edit,
    Heart,
    /// 实心的心形（已喜欢）。
    HeartFilled,
    Sun,
    Moon,
    Monitor,
    Check,
    /// 关于页的「i」圆标。
    Info,
    Keyboard,
    /// Windows 窗口按钮（`ui::caption`）：字形占 24 格里的 4.5–19.5，按
    /// 16 pt 显示时为 10 pt 见方、线宽 1 pt，与系统按钮一致。
    #[cfg(target_os = "windows")]
    WinMinimize,
    #[cfg(target_os = "windows")]
    WinMaximize,
    #[cfg(target_os = "windows")]
    WinRestore,
    #[cfg(target_os = "windows")]
    WinClose,
}

impl Icon {
    /// 图标的 SVG 内容。基础图标取自 Lucide（lucide-static 1.66.0，ISC
    /// 许可，<https://lucide.dev>），按原样使用：24 格、描边 2、圆头圆角。
    /// 括号里是 Lucide 的图标名。
    fn body(self) -> &'static str {
        match self {
            // play
            Icon::Play => {
                r#"<path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/>"#
            }
            // volume-2
            Icon::Volume => {
                r#"<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z"/><path d="M16 9a5 5 0 0 1 0 6"/><path d="M19.364 18.364a9 9 0 0 0 0-12.728"/>"#
            }
            // volume-x
            Icon::Mute => {
                r#"<path d="M11 4.702a.7.7 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.7.7 0 0 0 11 19.298z"/><path d="m16.5 14.5 5-5"/><path d="m16.5 9.5 5 5"/>"#
            }
            // house
            Icon::Home => {
                r#"<path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/><path d="M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#
            }
            // folder
            Icon::Folder => {
                r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>"#
            }
            // mic
            Icon::Mic => {
                r#"<path d="M12 19v3"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><rect x="9" y="2" width="6" height="13" rx="3"/>"#
            }
            // disc-3
            Icon::Disc => {
                r#"<circle cx="12" cy="12" r="10"/><path d="M6 12c0-1.7.7-3.2 1.8-4.2"/><circle cx="12" cy="12" r="2"/><path d="M18 12c0 1.7-.7 3.2-1.8 4.2"/>"#
            }
            // tag
            Icon::Tag => {
                r#"<path d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z"/><circle cx="7.5" cy="7.5" r=".5" fill="currentColor"/>"#
            }
            // settings
            Icon::Info => {
                r#"<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>"#
            }
            Icon::Settings => {
                r#"<path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915"/><circle cx="12" cy="12" r="3"/>"#
            }
            // chevron-down / chevron-right / chevron-left
            Icon::ChevronDown => r#"<path d="m6 9 6 6 6-6"/>"#,
            Icon::ChevronRight => r#"<path d="m9 18 6-6-6-6"/>"#,
            Icon::ChevronLeft => r#"<path d="m15 18-6-6 6-6"/>"#,
            // x
            Icon::Close => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
            // keyboard
            Icon::Keyboard => {
                r#"<rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01"/><path d="M10 10h.01"/><path d="M14 10h.01"/><path d="M18 10h.01"/><path d="M6 14h.01"/><path d="M18 14h.01"/><path d="M10 14h4"/>"#
            }
            // maximize-2
            Icon::Expand => {
                r#"<path d="M15 3h6v6"/><path d="m21 3-7 7"/><path d="m3 21 7-7"/><path d="M9 21H3v-6"/>"#
            }
            // music
            Icon::Note => {
                r#"<path d="M21 16V3a1 1 0 0 0-1.28-.96l-10 2.917a1 1 0 0 0-.72.96V19"/><circle cx="18" cy="16" r="3"/><circle cx="6" cy="19" r="3"/>"#
            }
            // user
            Icon::Person => {
                r#"<path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>"#
            }
            // plus / minus
            Icon::Plus => r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#,
            Icon::Minus => r#"<path d="M5 12h14"/>"#,
            // refresh-cw
            Icon::Refresh => {
                r#"<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>"#
            }
            // skip-back
            Icon::PrevLine => {
                r#"<path d="M17.971 4.285A2 2 0 0 1 21 6v12a2 2 0 0 1-3.029 1.715l-9.997-5.998a2 2 0 0 1-.003-3.432z"/><path d="M3 20V4"/>"#
            }
            // skip-forward
            Icon::NextLine => {
                r#"<path d="M21 4v16"/><path d="M6.029 4.285A2 2 0 0 0 3 6v12a2 2 0 0 0 3.029 1.715l9.997-5.998a2 2 0 0 0 .003-3.432z"/>"#
            }
            // ellipsis
            Icon::More => {
                r#"<circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/>"#
            }
            // folder-up
            Icon::Reveal => {
                r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/><path d="M12 10v6"/><path d="m9 13 3-3 3 3"/>"#
            }
            // list-end
            Icon::Queue => {
                r#"<path d="M16 5H3"/><path d="M16 12H3"/><path d="M9 19H3"/><path d="m16 16-3 3 3 3"/><path d="M21 5v12a2 2 0 0 1-2 2h-6"/>"#
            }
            // copy
            Icon::Copy => {
                r#"<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>"#
            }
            // audio-lines
            Icon::Wave => {
                r#"<path d="M2 10v3"/><path d="M6 6v11"/><path d="M10 3v18"/><path d="M14 8v7"/><path d="M18 5v13"/><path d="M22 10v3"/>"#
            }
            // search
            Icon::Search => r#"<path d="m21 21-4.34-4.34"/><circle cx="11" cy="11" r="8"/>"#,
            // repeat / repeat-1 / shuffle
            Icon::Repeat => {
                r#"<path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/>"#
            }
            Icon::RepeatOne => {
                r#"<path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/><path d="M11 10h1v4"/>"#
            }
            Icon::Shuffle => {
                r#"<path d="m18 14 4 4-4 4"/><path d="m18 2 4 4-4 4"/><path d="M2 18h1.973a4 4 0 0 0 3.3-1.7l5.454-8.6a4 4 0 0 1 3.3-1.7H22"/><path d="M2 6h1.972a4 4 0 0 1 3.6 2.2"/><path d="M22 18h-6.041a4 4 0 0 1-3.3-1.8l-.359-.45"/>"#
            }
            // list-music
            Icon::Playlist => {
                r#"<path d="M16 5H3"/><path d="M11 12H3"/><path d="M11 19H3"/><path d="M21 16V5"/><circle cx="18" cy="16" r="3"/>"#
            }
            // sparkles
            Icon::Smart => {
                r#"<path d="M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594z"/><path d="M20 2v4"/><path d="M22 4h-4"/><circle cx="4" cy="20" r="2"/>"#
            }
            // trash-2
            Icon::Trash => {
                r#"<path d="M10 11v6"/><path d="M14 11v6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>"#
            }
            // pencil
            Icon::Edit => {
                r#"<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/><path d="m15 5 4 4"/>"#
            }
            // heart（实心版同一路径加填充）
            Icon::Heart => {
                r#"<path d="M2 9.5a5.5 5.5 0 0 1 9.591-3.676.56.56 0 0 0 .818 0A5.49 5.49 0 0 1 22 9.5c0 2.29-1.5 4-3 5.5l-5.492 5.313a2 2 0 0 1-3 .019L5 15c-1.5-1.5-3-3.2-3-5.5"/>"#
            }
            Icon::HeartFilled => {
                r#"<path d="M2 9.5a5.5 5.5 0 0 1 9.591-3.676.56.56 0 0 0 .818 0A5.49 5.49 0 0 1 22 9.5c0 2.29-1.5 4-3 5.5l-5.492 5.313a2 2 0 0 1-3 .019L5 15c-1.5-1.5-3-3.2-3-5.5" fill="currentColor"/>"#
            }
            // sun
            Icon::Sun => {
                r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>"#
            }
            // moon
            Icon::Moon => r#"<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>"#,
            // monitor
            Icon::Monitor => {
                r#"<rect width="20" height="14" x="2" y="3" rx="2"/><line x1="8" x2="16" y1="21" y2="21"/><line x1="12" x2="12" y1="17" y2="21"/>"#
            }
            // check
            Icon::Check => r#"<path d="M20 6 9 17l-5-5"/>"#,
            // Windows 窗口按钮：不是 Lucide，按系统按钮的尺寸画（见上）。
            #[cfg(target_os = "windows")]
            Icon::WinMinimize => {
                r#"<path d="M4.5 12h15" stroke-width="1.5" stroke-linecap="butt"/>"#
            }
            #[cfg(target_os = "windows")]
            Icon::WinMaximize => {
                r#"<rect x="5.25" y="5.25" width="13.5" height="13.5" rx="1.5" stroke-width="1.5"/>"#
            }
            #[cfg(target_os = "windows")]
            Icon::WinRestore => {
                r#"<rect x="5.25" y="8.25" width="10.5" height="10.5" rx="1.5" stroke-width="1.5"/><path d="M8.25 5.25h9a1.5 1.5 0 0 1 1.5 1.5v9" stroke-width="1.5"/>"#
            }
            #[cfg(target_os = "windows")]
            Icon::WinClose => {
                r#"<path d="M5.25 5.25l13.5 13.5M18.75 5.25l-13.5 13.5" stroke-width="1.5"/>"#
            }
        }
    }
}

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

fn percent_encode(text: &str) -> String {
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

/// 应用图标的 SVG 版：直接用 resources/icon.svg，与应用图标同一份图形。
/// 源文件根元素声明 1024 见方；把根标签换成带 viewBox 的显示尺寸
/// （文件头注释：声明尺寸即光栅化尺寸，按 1024 出图再缩到十几点会糊）。
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

/// 播放 / 暂停键的图标：`playing` 为真时显示暂停，切换时顶点逐个移动
/// 过去。顶点绑定在动画值上，由平台在绘制时读取，形变不重跑界面代码。
/// 系统开了「减弱动态效果」时直接切换。
pub fn play_pause(
    size: f32,
    playing: impl Fn() -> bool + 'static,
    tint: impl Fn() -> AdaptiveColor + 'static,
) -> CanvasView {
    let target = |on: bool| if on { 1.0 } else { 0.0 };
    let morph = animated_value(target(untracked(&playing)));
    effect(move || {
        let to = target(playing());
        if untracked(reduce_motion) {
            morph.set(to);
        } else {
            let _ = morph.timing(to, MORPH_MS, MORPH_EASE);
        }
    });
    let lerp = move |a: f32, b: f32| morph.interpolate(&[0.0, 1.0], &[a, b]);
    let points: [[(AnimatedNode, AnimatedNode); 4]; 2] = std::array::from_fn(|s| {
        std::array::from_fn(|i| {
            let (play, pause) = (PLAY_SHAPES[s][i], PAUSE_SHAPES[s][i]);
            (lerp(play.0, pause.0), lerp(play.1, pause.1))
        })
    });
    let scale = size / 24.0;
    canvas(move || {
        let c = color(tint());
        let mut p = Painter::new()
            .scale(scale, scale)
            .fill_color(c)
            .stroke_color(c)
            .line_width(MORPH_STROKE)
            .line_join(nsh_core::canvas::LineJoin::Round)
            .begin_path();
        for [a, b, c, d] in &points {
            p = p
                .move_to(&a.0, &a.1)
                .line_to(&b.0, &b.1)
                .line_to(&c.0, &c.1)
                .line_to(&d.0, &d.1)
                .close_path();
        }
        p.fill().stroke()
    })
    .size(px(size))
    .pointer_events(PointerEvents::None)
}
