//! 全屏播放（参照设计稿）：模糊封面铺底、整体压暗；左半区居中一列——
//! 封面、曲名与「更多」、歌手 / 专辑、标签、细波形进度与音频信息、线框
//! 控制按钮、音量；右半区居中的歌词，当前行亮、其余按距离变暗变模糊。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use nsh_base::{menu, menu_item, menu_separator};
use nsh_core::prelude::*;

use super::icons::{icon, icon_dyn, Icon};
use super::parts::{progress_fraction, scrub, shown_time};
use super::store::{format_time, player, seek_to, set_volume, store, toggle_mute};
use super::theme::*;
use crate::metadata::MetaInfo;

/// 左列宽度（= 封面边长）。
const COLUMN: f32 = 326.0;
/// 歌词列最大宽度。
const LYRICS: f32 = 520.0;
/// 当前歌词行停在视口从上往下的这个比例处。
const ANCHOR: f32 = 0.34;

/// 全屏播放页。`fade` 为 0→1 的开合进度；`on_close` 收起。
pub fn full_player(fade: AnimatedValue, on_close: Rc<dyn Fn()>) -> View {
    let s = store();
    let backdrop = move || s.track.with(|t| t.backdrop.clone());

    let close = on_close.clone();
    on_key("escape", move |_| close());
    on_key("left", |_| player().seek_by(-5.0));
    on_key("right", |_| player().seek_by(5.0));
    on_key("up", move |_| set_volume(s.playback.with_untracked(|p| p.volume) + 0.05));
    on_key("down", move |_| set_volume(s.playback.with_untracked(|p| p.volume) - 0.05));

    let swap = Swap::install();

    // 底：深灰，封面放大模糊，再压一层暗色，保证白字对比。
    let backdrop = view()
        .absolute()
        .inset(px(0.0))
        .bg(AdaptiveColor::rgb(0x2a2a2c, 0x2a2a2c))
        .child(
            // 背景图是预先模糊好的 64 px 小图（covers::backdrop），拉伸铺满：
            // 解码后只有十几 KB，不在窗口尺寸上实时模糊。
            view().absolute().inset(px(0.0)).bind_opacity(&swap.backdrop).child(when(move || backdrop().is_some(), move || {
                image(move || backdrop().unwrap_or_default())
                    .absolute()
                    .inset(px(-40.0))
                    .resize_mode(ResizeMode::Cover)
                    .opacity(0.9)
            })),
        )
        .child(view().absolute().inset(px(0.0)).bg(black_a(0.5)));

    // 收起按钮在右上角，让开 Windows 的窗口按钮（它们占标题栏右侧）。
    let close_button = view()
        .absolute()
        .top(move || px(if controls_right() > 0.0 { top_inset() + 8.0 } else { 14.0 }))
        .right(px(18.0))
        .size(px(32.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .role(AccessibilityRole::Button)
        .label("收起")
        .window_drag(false)
        .hover(|st| st.bg(white_a(0.1)))
        .active(|st| st.opacity(0.6))
        .on_press(move || on_close())
        .child(icon(Icon::ChevronDown, 18.0, || white_a(0.7)));

    view()
        .absolute()
        .inset(px(0.0))
        .z_index(50)
        .overflow_hidden()
        .bind_opacity(&fade)
        .bind_scale(&fade.interpolate(&[0.0, 1.0], &[0.98, 1.0]))
        .child(backdrop)
        .child(
            view()
                .flex_1()
                .flex_row()
                .child(view().flex_1().items_center().justify_center().px(px(32.0)).child(left_column(&swap)))
                .child(view().flex_1().items_center().pl(px(16.0)).pr(px(28.0)).child(lyrics_column(&swap))),
        )
        .child(view().absolute().top(px(0.0)).left(px(0.0)).right(px(0.0)).h(move || px(top_inset())).window_drag(true))
        .child(close_button)
}

// --- 切歌过渡 -----------------------------------------------------------------

/// 切歌时的过渡：内容淡入并从切歌方向滑入（下一首从右、上一首从左），
/// 背景的模糊封面慢一些交叉淡入。系统开了「减弱动态效果」时不动画。
struct Swap {
    content: AnimatedValue,
    shift: AnimatedValue,
    lyrics_shift: AnimatedValue,
    backdrop: AnimatedValue,
}

/// 滑入距离（点）。
const SLIDE: f32 = 36.0;

/// 切歌方向：队列位置变小为上一首（-1），否则为下一首（1）。
fn direction(before: i64, now: i64) -> f32 {
    if now < before { -1.0 } else { 1.0 }
}

impl Swap {
    fn install() -> Swap {
        let s = store();
        let swap = Swap {
            content: animated_value(1.0),
            shift: animated_value(0.0),
            lyrics_shift: animated_value(0.0),
            backdrop: animated_value(1.0),
        };
        let (content, shift, lyrics_shift, backdrop) =
            (swap.content.clone(), swap.shift.clone(), swap.lyrics_shift.clone(), swap.backdrop.clone());
        let key = computed(move || s.track.with(|t| (t.path.clone(), t.index)));
        let previous = RefCell::new(None::<(Option<String>, i64)>);
        effect(move || {
            let now = key.get();
            let before = previous.replace(Some(now.clone()));
            // 打开全屏时不动画；只在曲目真的换了时动画。
            let Some(before) = before else { return };
            if before.0 == now.0 || now.0.is_none() || untracked(reduce_motion) {
                return;
            }
            let dir = direction(before.1, now.1);
            let ease = Easing::Bezier(0.22, 1.0, 0.36, 1.0);
            content.set(0.0);
            shift.set(dir * SLIDE);
            lyrics_shift.set(dir * SLIDE * 0.6);
            backdrop.set(0.35);
            let _ = content.timing(1.0, 320.0, Easing::EaseOut);
            let _ = shift.timing(0.0, 420.0, ease);
            let _ = lyrics_shift.timing(0.0, 480.0, ease);
            let _ = backdrop.timing(1.0, 700.0, Easing::EaseOut);
        });
        swap
    }
}

// --- 左列 ---------------------------------------------------------------------

/// 质量标签：Hi-Res（无损且高于 16-bit / 48 kHz）、Lossless、HQ（有损 ≥ 256 kbps）。
fn quality_tag(m: &MetaInfo) -> Option<String> {
    match m.quality.as_deref() {
        Some("Lossless") if m.bit_depth.unwrap_or(16) > 16 || m.sample_rate.unwrap_or(44_100) > 48_000 => {
            Some("Hi-Res".into())
        }
        Some("Lossless") => Some("Lossless".into()),
        Some(_) if m.bit_rate.unwrap_or(0) >= 256 => Some("HQ".into()),
        _ => None,
    }
}

/// 进度条下方的小字：24-bit / 96kHz / 2304kbps / Stereo。
fn audio_line(m: &MetaInfo) -> String {
    let mut parts = Vec::new();
    if let Some(depth) = m.bit_depth.filter(|d| *d > 0) {
        parts.push(format!("{depth}-bit"));
    }
    if let Some(rate) = m.sample_rate {
        let khz = rate as f64 / 1000.0;
        parts.push(if khz.fract() == 0.0 { format!("{khz:.0}kHz") } else { format!("{khz:.1}kHz") });
    }
    if let Some(rate) = m.bit_rate {
        parts.push(format!("{rate}kbps"));
    }
    match m.channels {
        Some(1) => parts.push("Mono".into()),
        Some(2) => parts.push("Stereo".into()),
        Some(n) if n > 2 => parts.push(format!("{n}ch")),
        _ => {}
    }
    parts.join(" / ")
}

fn left_column(swap: &Swap) -> View {
    let s = store();
    let artwork = move || s.track.with(|t| t.artwork.clone());

    let art = view()
        .size(px(COLUMN))
        .rounded(px(10.0))
        .overflow_hidden()
        .bg(white_a(0.08))
        .shadow_xl()
        .items_center()
        .justify_center()
        .child(icon(Icon::Note, 96.0, || white_a(0.3)))
        .child(when(move || artwork().is_some(), move || {
            image(move || artwork().unwrap_or_default())
                .absolute()
                .inset(px(0.0))
                .resize_mode(ResizeMode::Cover)
        }));

    let title_row = view()
        .h(px(28.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text(move || s.track.with(|t| if t.title.is_empty() { "Zeo".into() } else { t.title.clone() }))
                .flex_1()
                .number_of_lines(1)
                .text_size(px(20.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(white_a(0.95)),
        )
        .child(more_menu());

    let meta_line = |glyph: Icon, value: fn(&super::store::NowPlaying) -> String| {
        view()
            .h(px(18.0))
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .child(icon(glyph, 13.0, || white_a(0.6)))
            .child(
                text(move || s.track.with(value))
                    .flex_1()
                    .number_of_lines(1)
                    .text_size(px(13.0))
                    .text_color(white_a(0.7)),
            )
    };

    let tags = dynamic(move || {
        // 流派按设置里的分隔符拆开（和标签页一致），分隔符一改就跟着变。
        let separators = s.library.with(|l| l.separators.clone());
        let list: Vec<(String, bool)> = s.meta.with(|m| {
            let Some(m) = m else { return Vec::new() };
            let mut out = Vec::new();
            if let Some(q) = quality_tag(m) {
                out.push((q, true));
            }
            if let Some(year) = m.year {
                out.push((year.to_string(), false));
            }
            let genres = crate::library::split_genres(&m.genres, &separators);
            out.extend(genres.into_iter().take(3).map(|g| (g, false)));
            if let Some(codec) = &m.codec {
                out.push((codec.clone(), false));
            }
            out
        });
        (!list.is_empty()).then(|| {
            view().flex_row().gap(px(8.0)).children(list.into_iter().map(|(label, strong)| {
                view()
                    .h(px(20.0))
                    .px(px(7.0))
                    .justify_center()
                    .rounded(px(4.0))
                    .bg(if strong {
                        AdaptiveColor::rgb(0x7fae8a, 0x7fae8a).opacity(0.45)
                    } else {
                        white_a(0.14)
                    })
                    .child(
                        text(label)
                            .number_of_lines(1)
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(white_a(0.85)),
                    )
            }))
        })
    });

    let skip = |glyph: Icon, label: &'static str, press: fn()| {
        view()
            .size(px(44.0))
            .rounded_full()
            .items_center()
            .justify_center()
            .role(AccessibilityRole::Button)
            .label(label)
            .hover(|st| st.bg(white_a(0.08)))
            .active(|st| st.opacity(0.5))
            .on_press(press)
            .child(icon(glyph, 20.0, || white_a(0.85)))
    };
    let play = view()
        .size(px(56.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .role(AccessibilityRole::Button)
        .label("播放/暂停")
        .hover(|st| st.bg(white_a(0.08)))
        .active(|st| st.scale(0.92))
        .transition_transform()
        .duration(120.0)
        .on_press(|| player().toggle())
        .child(icon_dyn(
            move || if s.playback.with(|p| p.playing) { Icon::PauseLine } else { Icon::PlayLine },
            28.0,
            || white_a(0.95),
        ));
    let controls = view()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(26.0))
        .child(skip(Icon::PrevLine, "上一首", || player().previous()))
        .child(play)
        .child(skip(Icon::NextLine, "下一首", || player().next()));

    view()
        .w(px(COLUMN))
        .flex_shrink_0()
        // 换曲时封面与曲目信息一起淡入、从切歌方向滑入；进度与控制不动。
        .child(
            view()
                .bind_opacity(&swap.content)
                .bind_translate_x(&swap.shift)
                .child(art)
                .child(view().h(px(24.0)))
                .child(title_row)
                .child(
                    view()
                        .gap(px(6.0))
                        .pt(px(14.0))
                        .child(meta_line(Icon::Person, |t| t.artist.clone()))
                        .child(meta_line(Icon::Disc, |t| t.album.clone())),
                )
                // 标签只占一行（多了裁掉），没有标签时也留出这一行：下面的
                // 进度条位置不随元数据载入而跳动。
                .child(view().mt(px(14.0)).h(px(20.0)).overflow_hidden().child(tags)),
        )
        .child(view().pt(px(28.0)).child(waveform_seek()))
        .child(view().pt(px(20.0)).child(controls))
        .child(view().pt(px(18.0)).child(volume_row()))
}

/// 「⋯」：在访达中显示、拷贝文件路径。
fn more_menu() -> AnyElement {
    let s = store();
    let path = move || s.track.with_untracked(|t| t.path.clone());
    let item = |label: &'static str, glyph: Icon, on_select: Box<dyn Fn()>| {
        menu_item()
            .on_select(move || on_select())
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .hover(|st| st.bg(hover()))
            .active(|st| st.bg(hover()))
            .child(icon(glyph, 15.0, muted_fg))
            .child(text(label).text_size(px(13.0)).text_color(fg()))
    };
    menu()
        .offset(px(4.0))
        .popup_style(|st| {
            st.min_w(px(200.0))
                .p(px(4.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(border())
                .bg(nsh_base::theme().popover)
                .shadow_lg()
        })
        .trigger(
            view()
                .size(px(28.0))
                .rounded(px(6.0))
                .items_center()
                .justify_center()
                .label("更多")
                .hover(|st| st.bg(white_a(0.1)))
                .active(|st| st.opacity(0.6))
                .child(icon(Icon::More, 18.0, || white_a(0.7))),
        )
        .child(item(
            "在访达中显示",
            Icon::Reveal,
            Box::new(move || {
                if let Some(path) = path() {
                    reveal(&path);
                }
            }),
        ))
        .child(menu_separator().h(px(1.0)).my(px(4.0)).bg(border()))
        .child(item(
            "拷贝文件路径",
            Icon::Copy,
            Box::new(move || {
                if let Some(path) = path() {
                    set_clipboard(path);
                }
            }),
        ))
        .into_element()
}

/// 在系统文件管理器里选中文件。
fn reveal(path: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    // explorer 自己解析 `/select,"路径"`：带空格的路径须原样加引号，不能经
    // Rust 的参数转义。
    #[cfg(target_os = "windows")]
    let _ = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer").raw_arg(format!("/select,\"{path}\"")).spawn()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = std::process::Command::new("xdg-open")
        .arg(std::path::Path::new(path).parent().unwrap_or(std::path::Path::new("/")))
        .spawn();
}

/// 细波形进度：[`crate::waveform::BARS`] 根圆头竖条，已播亮、未播暗；
/// 播放头所在的那根和进度条一样只亮到播放头为止。可点可拖，抬起 seek。
/// 下方：已播时间、音频信息、总时长。
fn waveform_seek() -> View {
    let s = store();
    let width = signal(0.0_f32);
    let duration = move || s.playback.with(|p| p.duration);
    const H: f32 = 26.0;
    let painter = move || {
        let w = width.get();
        let head = progress_fraction() as f32 * w;
        let mut p = Painter::new();
        s.waveform.with(|bars| {
            // 载入中 / 解不开：全部是归一化的下限。
            let fallback = [crate::waveform::WaveformOptions::default().min; crate::waveform::BARS];
            let bars: &[f64] = match bars {
                Some(b) => b,
                None => &fallback,
            };
            let n = bars.len().max(1);
            let step = w / n as f32;
            let bar = (step * 0.45).clamp(1.5, 3.5);
            let dim = color(white_a(0.35));
            let lit = color(white_a(0.95));
            for (i, amp) in bars.iter().enumerate() {
                // 已归一化到 0.1..0.9（waveform.rs）：直接按比例画。
                let bh = ((*amp as f32).clamp(0.0, 1.0) * H).max(bar);
                let x = i as f32 * step + (step - bar) / 2.0;
                let rect = Bounds::new(point(px(x), px((H - bh) / 2.0)), size(px(bar), px(bh)));
                let radius = px(bar / 2.0);
                // 已播部分的宽度：0（未到）、bar（已过）或两者之间（播放头上）。
                let played = (head - x).clamp(0.0, bar);
                if played < bar {
                    p = std::mem::take(&mut p).fill_color(dim).begin_path().round_rect(rect, radius).fill();
                }
                if played >= bar {
                    p = std::mem::take(&mut p).fill_color(lit).begin_path().round_rect(rect, radius).fill();
                } else if played > 0.0 {
                    // 只亮到播放头：裁到左侧 `played` 宽，再画一根完整的亮条。
                    p = std::mem::take(&mut p)
                        .save()
                        .begin_path()
                        .rect(Bounds::new(point(px(x), px(0.0)), size(px(played), px(H))))
                        .clip()
                        .fill_color(lit)
                        .begin_path()
                        .round_rect(rect, radius)
                        .fill()
                        .restore();
                }
            }
        });
        p
    };
    let small = |t: Text| t.text_size(px(12.0)).tabular_nums().text_color(white_a(0.75));
    // 高度固定：波形 26 + 间距 10 + 时间行 16。
    view()
        .w_full()
        .h(px(H + 26.0))
        .gap(px(10.0))
        .child(scrub(
            view()
                .w_full()
                .h(px(H))
                .on_layout(move |frame: &Bounds<Points>| width.set(frame.size.width.0))
                .child(canvas(painter).w_full().h(px(H))),
            move |f| s.seek_preview.set(Some(f * duration())),
            move |f| seek_to(f * duration()),
        ))
        .child(
            view()
                .h(px(16.0))
                .flex_row()
                .items_center()
                .child(small(text(move || format_time(shown_time()))).w(px(48.0)))
                .child(
                    text(move || s.meta.with(|m| m.as_ref().map(|m| audio_line(m)).unwrap_or_default()))
                        .flex_1()
                        .text_center()
                        .number_of_lines(1)
                        .text_size(px(10.5))
                        .text_color(white_a(0.45)),
                )
                .child(small(text(move || format_time(duration()))).w(px(48.0)).text_right()),
        )
}

fn volume_row() -> View {
    let s = store();
    let volume = move || s.playback.with(|p| p.volume);
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            view()
                .size(px(26.0))
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .role(AccessibilityRole::Button)
                .label("静音")
                .hover(|st| st.bg(white_a(0.08)))
                .active(|st| st.opacity(0.6))
                .on_press(toggle_mute)
                .child(icon_dyn(move || if volume() <= 0.0 { Icon::Mute } else { Icon::Volume }, 15.0, || white_a(0.7))),
        )
        .child(scrub(
            view().flex_1().h(px(18.0)).justify_center().child(
                view()
                    .h(px(3.0))
                    .w_full()
                    .rounded_full()
                    .bg(white_a(0.2))
                    .overflow_hidden()
                    .pointer_events(PointerEvents::None)
                    .child(view().h_full().rounded_full().bg(white_a(0.9)).w(move || relative(volume() as f32))),
            ),
            set_volume,
            set_volume,
        ))
        .child(
            text(move || format!("{}", (volume() * 100.0).round()))
                .w(px(26.0))
                .text_right()
                .text_size(px(11.0))
                .tabular_nums()
                .text_color(white_a(0.55)),
        )
}

// --- 歌词 ---------------------------------------------------------------------

/// 右半区：有歌词时滚动列表，否则一行提示。
fn lyrics_column(swap: &Swap) -> View {
    let s = store();
    let has_lines = move || s.lyrics.with(|l| l.as_ref().is_some_and(|l| !l.output.lines.is_empty()));
    view().w_full().h_full().bind_opacity(&swap.content).bind_translate_x(&swap.lyrics_shift).child(
        when(has_lines, lyrics_scroller).otherwise(move || {
            view().flex_1().items_center().justify_center().child(
                text(move || {
                    if s.lyrics.with(Option::is_none) && s.track.with(|t| t.path.is_some()) {
                        "正在载入歌词…"
                    } else {
                        "暂无歌词"
                    }
                })
                .text_size(px(15.0))
                .text_color(white_a(0.4)),
            )
        }),
    )
}

/// 行级歌词：跟随时间滚动（当前行停在 [`ANCHOR`] 处），用户滚动后 3 s
/// 恢复跟随；点击行 seek；距离当前行越远越暗越模糊。
fn lyrics_scroller() -> View {
    let s = store();
    let scroller = element_ref();
    let viewport = signal(0.0_f32);
    let laid = signal(0_usize);
    let follow = signal(true);
    let line_ys: Rc<RefCell<Vec<f32>>> = Rc::default();
    let target: Rc<RefCell<Option<(f32, std::time::Instant)>>> = Rc::default();
    let resume: Rc<RefCell<Option<Spawned<()>>>> = Rc::default();
    let last_followed = Rc::new(RefCell::new(-1_i64));
    let prev_time = Rc::new(RefCell::new(-1.0_f64));
    let lines = untracked(move || s.lyrics.with(|l| l.as_ref().map(|l| l.output.clone())))
        .expect("lyrics_scroller is shown with lyrics");
    let timed = lines.timed;
    let count = lines.lines.len();

    let scroll_to_line = {
        let line_ys = line_ys.clone();
        let target = target.clone();
        move |index: i64| {
            let ys = line_ys.borrow();
            let Some(&y) = usize::try_from(index).ok().and_then(|i| ys.get(i)) else { return };
            let y = (y - viewport.get_untracked() * ANCHOR).max(0.0);
            *target.borrow_mut() = Some((y, std::time::Instant::now()));
            scroller.scroll_to(point(px(0.0), px(y)), true);
        }
    };

    // 时间推进：行变化或 seek 跳变时滚动跟随。
    {
        let last_followed = last_followed.clone();
        let prev_time = prev_time.clone();
        effect(move || {
            let ready = viewport.get() > 0.0 && laid.get() >= count;
            let time = s.playback.with(|p| p.time);
            let index = s.active_line.get();
            if !ready {
                return;
            }
            let jumped = {
                let mut prev = prev_time.borrow_mut();
                let jumped = *prev >= 0.0 && (time - *prev).abs() > 1.5;
                *prev = time;
                jumped
            };
            if !follow.get() || index < 0 || (index == *last_followed.borrow() && !jumped) {
                return;
            }
            *last_followed.borrow_mut() = index;
            scroll_to_line(index);
        });
    }

    let interrupt = {
        let target = target.clone();
        let resume = resume.clone();
        let last_followed = last_followed.clone();
        Rc::new(move || {
            *target.borrow_mut() = None;
            follow.set(false);
            if let Some(task) = resume.borrow_mut().take() {
                task.cancel();
            }
            let last_followed = last_followed.clone();
            *resume.borrow_mut() = Some(set_timeout(Duration::from_secs(3), move || {
                *last_followed.borrow_mut() = s.active_line.get_untracked();
                follow.set(true);
            }));
        })
    };

    let rows = lines.lines.iter().enumerate().map(|(index, line)| {
        let i = index as i64;
        let seekable = line.time >= 0.0;
        let time = line.time;
        let content = line.raw_content.trim().to_owned();
        let content = if content.is_empty() { "· · ·".to_owned() } else { content };
        let metadata = line.metadata;
        let distance = move || if timed { (i - s.active_line.get()).unsigned_abs() as f32 } else { 0.0 };
        let line_ys = line_ys.clone();
        let resume = resume.clone();
        let last_followed = last_followed.clone();
        let content_view = view()
            .self_center()
            .max_w(px(LYRICS))
            .items_center()
            .py(px(14.0))
            .px(px(20.0))
            .rounded(px(12.0))
            .gap(px(6.0))
            // 高亮只包住文字本身，触发区是整行（外层是 group）。
            .group_hover(|st| st.bg(white_a(0.07)))
            .transition_colors()
            .duration(150.0)
            .child(
                text(content)
                    .text_center()
                    .text_size(px(if metadata { 14.0 } else { 20.0 }))
                    .line_height(px(if metadata { 22.0 } else { 30.0 }))
                    .font_weight(if metadata { FontWeight::MEDIUM } else { FontWeight::BOLD })
                    .text_color(white_a(if metadata { 0.7 } else { 0.95 })),
            )
            .child(line.trans_content.clone().map(|trans| {
                text(trans)
                    .text_center()
                    .text_size(px(15.0))
                    .line_height(px(22.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(white_a(0.72))
            }));
        // 时间戳：浮在行的最右侧、垂直居中，只在这一行悬停时出现；不占布局，
        // 也不挡点击。
        let stamp = seekable.then(|| {
            view()
                .absolute()
                .top(px(0.0))
                .bottom(px(0.0))
                .right(px(0.0))
                .justify_center()
                .pointer_events(PointerEvents::None)
                .opacity(0.0)
                .group_hover(|st| st.opacity(1.0))
                .transition_opacity()
                .duration(150.0)
                .child(
                    view()
                        .h(px(22.0))
                        .px(px(8.0))
                        .justify_center()
                        .rounded(px(6.0))
                        .bg(AdaptiveColor::rgb(0x1c2230, 0x1c2230).opacity(0.75))
                        .child(
                            text(format_time(time))
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .tabular_nums()
                                .text_color(white_a(0.8)),
                        ),
                )
        });
        view()
            .w_full()
            .group()
            .role(AccessibilityRole::Button)
            .label(if seekable { format!("跳到 {}", format_time(time)) } else { String::new() })
            // 当前行清晰明亮；越远越暗、越模糊（最多 4 pt）。时间戳在
            // 这层之外，不跟着变暗变糊。
            .child(
                view()
                    .w_full()
                    .style_with(move |st| {
                        let d = distance();
                        let (opacity, blur) = if !timed {
                            (0.85, 0.0)
                        } else if d == 0.0 {
                            (1.0, 0.0)
                        } else {
                            ((0.42 - d * 0.035).max(0.16), (0.8 + d * 0.7).min(4.0))
                        };
                        st.opacity(opacity).blur(px(blur))
                    })
                    .transition_all()
                    .duration(300.0)
                    // 悬停时这一行也变清楚，便于看清再点。
                    .group_hover(|st| st.opacity(1.0).blur(px(0.0)))
                    .child(content_view),
            )
            .child(stamp)
            .on_layout(move |frame: &Bounds<Points>| {
                let mut ys = line_ys.borrow_mut();
                if ys.len() <= index {
                    ys.resize(index + 1, 0.0);
                }
                ys[index] = frame.origin.y.0;
                drop(ys);
                if index + 1 > laid.get_untracked() {
                    laid.set(index + 1);
                }
            })
            .on_press(move || {
                if seekable {
                    seek_to(time);
                    follow.set(true);
                    *last_followed.borrow_mut() = -1;
                    if let Some(task) = resume.borrow_mut().take() {
                        task.cancel();
                    }
                }
            })
    });

    let user_scroll = interrupt.clone();
    let drag = interrupt.clone();
    view()
        .flex_1()
        .w_full()
        .child((!timed).then(|| {
            text("歌词无时间信息")
                .text_center()
                .text_size(px(11.0))
                .letter_spacing(px(0.6))
                .text_color(white_a(0.4))
                .pt(px(TITLE_BAR))
        }))
        .child(
            scroll()
                .flex_1()
                .scrollbar(false)
                .element_ref(&scroller)
                .on_layout(move |frame: &Bounds<Points>| viewport.set(frame.size.height.0))
                .on_scroll(move |e: &ScrollEvent| {
                    let y = e.offset.y.0;
                    // 程序滚动：动画进行中（约 0.7 s 内）或已到目标附近。
                    let ours = target
                        .borrow()
                        .is_some_and(|(t, at)| at.elapsed().as_millis() < 700 || (y - t).abs() < 64.0);
                    if !ours && follow.get_untracked() {
                        user_scroll();
                    }
                })
                .on_scroll_drag_start(move |_: &Point<Points>| drag())
                .child(
                    view()
                        .w_full()
                        .child(view().h(move || px(viewport.get() * ANCHOR)))
                        .children(rows)
                        .child(view().h(move || px(viewport.get() * (1.0 - ANCHOR)))),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> MetaInfo {
        MetaInfo {
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            album_artist: String::new(),
            duration: 0.0,
            has_art: false,
            lyrics: None,
            bit_rate: Some(160),
            bit_depth: None,
            sample_rate: Some(44_100),
            channels: Some(2),
            codec: Some("MP3".into()),
            quality: Some("Lossy".into()),
            genres: Vec::new(),
            year: None,
            track: None,
            track_total: None,
            disk: None,
            disk_total: None,
            album_artists: Vec::new(),
            composers: Vec::new(),
        }
    }

    #[test]
    fn audio_line_matches_the_design() {
        assert_eq!(audio_line(&meta()), "44.1kHz / 160kbps / Stereo");
        let hires = MetaInfo { bit_depth: Some(24), sample_rate: Some(96_000), bit_rate: Some(2304), ..meta() };
        assert_eq!(audio_line(&hires), "24-bit / 96kHz / 2304kbps / Stereo");
    }

    #[test]
    fn swap_direction_follows_the_queue() {
        assert_eq!(direction(3, 4), 1.0, "下一首从右滑入");
        assert_eq!(direction(4, 3), -1.0, "上一首从左滑入");
        assert_eq!(direction(2, 2), 1.0, "队列不变（重新打开）当作下一首");
    }

    #[test]
    fn quality_tags() {
        assert_eq!(quality_tag(&meta()), None, "160 kbps 有损不标");
        assert_eq!(quality_tag(&MetaInfo { bit_rate: Some(320), ..meta() }).as_deref(), Some("HQ"));
        let flac = MetaInfo { quality: Some("Lossless".into()), bit_depth: Some(16), ..meta() };
        assert_eq!(quality_tag(&flac).as_deref(), Some("Lossless"));
        let hires = MetaInfo { bit_depth: Some(24), ..flac };
        assert_eq!(quality_tag(&hires).as_deref(), Some("Hi-Res"));
    }
}
