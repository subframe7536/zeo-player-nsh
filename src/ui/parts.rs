//! 共享部件：封面、按钮、页头、曲目行、空状态、拖动条。

use std::rc::Rc;

use nsh_core::prelude::*;

use super::icons::{icon, icon_dyn, Icon};
use super::store::{format_time, store};
use super::theme::*;
use crate::library::TrackEntry;

/// 封面：图片或占位（音符 + 柔和底色）。`size` 为 `None` 时铺满宽度、正方形。
pub fn cover(src: impl Fn() -> Option<SharedString> + Copy + 'static, size: Option<f32>, radius: f32) -> View {
    cover_frame(src, size, radius, Icon::Note)
}

/// 固定大小的封面，占位图标为 `placeholder`。
pub fn cover_with(src: impl Fn() -> Option<SharedString> + Copy + 'static, size: f32, radius: f32, placeholder: Icon) -> View {
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
    frame
        .child(icon(placeholder, glyph, faint))
        .child(when(move || src().is_some(), move || {
            image(move || src().unwrap_or_default())
                .absolute()
                .inset(px(0.0))
                .resize_mode(ResizeMode::Cover)
        }))
}

/// 方形图标按钮（工具栏、播放条）。
pub fn icon_button(
    name: impl Fn() -> Icon + 'static,
    size: f32,
    label: &'static str,
    tint: impl Fn() -> AdaptiveColor + 'static,
    on_press: impl Fn() + 'static,
) -> View {
    view()
        .size(px(size + 16.0))
        .items_center()
        .justify_center()
        .rounded(px(8.0))
        .role(AccessibilityRole::Button)
        .label(label)
        .hover(|s| s.bg(hover()))
        .active(|s| s.opacity(0.6))
        .on_press(on_press)
        .child(icon_dyn(name, size, tint))
}

/// 主操作按钮（胶囊）：「播放」。
pub fn pill_button(name: Icon, label: &'static str, on_press: impl Fn() + 'static) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(36.0))
        .px(px(18.0))
        .rounded_full()
        .bg(primary())
        .role(AccessibilityRole::Button)
        .label(label)
        .hover(|s| s.opacity(0.92))
        .active(|s| s.opacity(0.75))
        .on_press(on_press)
        .child(icon(name, 14.0, primary_fg))
        .child(
            text(label)
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(primary_fg()),
        )
}

/// 次要按钮（描边）。
pub fn outline_button(name: Icon, label: &'static str, on_press: impl Fn() + 'static) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(36.0))
        .px(px(16.0))
        .rounded_full()
        .border_1()
        .border_color(border())
        .role(AccessibilityRole::Button)
        .label(label)
        .hover(|s| s.bg(hover()))
        .active(|s| s.opacity(0.7))
        .on_press(on_press)
        .child(icon(name, 14.0, fg))
        .child(text(label).text_size(px(14.0)).font_weight(FontWeight::MEDIUM).text_color(fg()))
}

/// 页头：可选返回、大标题、副标题、右侧操作；整条可拖动窗口。
pub fn page_header(
    title: impl Fn() -> String + 'static,
    subtitle: impl Fn() -> String + 'static,
    back: Option<Rc<dyn Fn()>>,
    actions: Option<AnyElement>,
) -> View {
    let back_button = back.map(|back| {
        view()
            .flex_row()
            .items_center()
            .gap(px(2.0))
            .self_start()
            .h(px(28.0))
            .pl(px(4.0))
            .pr(px(10.0))
            .ml(px(-8.0))
            .rounded(px(8.0))
            .role(AccessibilityRole::Button)
            .label("返回")
            .window_drag(false)
            .hover(|s| s.bg(hover()))
            .active(|s| s.opacity(0.6))
            .on_press(move || back())
            .child(icon(Icon::ChevronLeft, 16.0, muted_fg))
            .child(text("返回").text_size(px(13.0)).text_color(muted_fg()))
    });
    view()
        .flex_row()
        .items_end()
        .gap(px(16.0))
        .px(px(32.0))
        .pt(px(TITLE_BAR - 8.0))
        .pb(px(16.0))
        .window_drag(true)
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(4.0))
                .child(back_button)
                .child(
                    text(title)
                        .number_of_lines(1)
                        .text_size(px(28.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(fg()),
                )
                .child(text(subtitle).number_of_lines(1).text_size(px(13.0)).text_color(muted_fg())),
        )
        .child(actions.map(|a| view().flex_row().gap(px(8.0)).window_drag(false).child(a)))
}

/// 空状态（居中图标 + 标题 + 说明）。
pub fn empty_state(name: Icon, title: &'static str, body: &'static str) -> View {
    view()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .p(px(32.0))
        .pb(px(BAR_SPACE))
        .child(
            view()
                .size(px(64.0))
                .rounded(px(18.0))
                .bg(muted())
                .items_center()
                .justify_center()
                .mb(px(8.0))
                .child(icon(name, 28.0, muted_fg)),
        )
        .child(text(title).text_size(px(16.0)).font_weight(FontWeight::SEMIBOLD).text_color(fg()))
        .child(text(body).text_size(px(13.0)).text_color(muted_fg()).text_center().max_w(px(360.0)))
}

/// 曲目列表显示哪些列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    pub cover: bool,
    pub album: bool,
    pub size: bool,
}

impl Columns {
    pub const ALL: Columns = Columns { cover: true, album: true, size: false };
}

/// 列宽（列头、曲目行、文件夹行共用，保证对齐）。
pub const W_NUMBER: f32 = 28.0;
pub const W_COVER: f32 = 40.0;
pub const W_ALBUM: f32 = 220.0;
pub const W_SIZE: f32 = 72.0;
pub const W_TIME: f32 = 56.0;

/// 内容区宽度（窗口减侧栏）。读取即订阅窗口大小。
fn content_width() -> f32 {
    window().width.0 - SIDEBAR
}

/// 窄窗口里依次收起：专辑列（< 760 pt）、大小列（< 600 pt）、序号列（< 420 pt）。
pub fn fits_album() -> bool {
    content_width() >= 760.0
}
pub fn fits_size() -> bool {
    content_width() >= 600.0
}
pub fn fits_number() -> bool {
    content_width() >= 420.0
}

/// 字节数 → 1.2 GB / 34.5 MB / 820 KB。
pub fn format_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.2} GB", b / 1024.0 / 1024.0 / 1024.0)
    } else if b >= 1024.0 * 1024.0 {
        format!("{:.1} MB", b / 1024.0 / 1024.0)
    } else {
        format!("{} KB", (b / 1024.0).round() as u64)
    }
}

/// 右对齐的数值列。
pub fn value_cell(width: f32, value: impl Fn() -> String + 'static) -> View {
    view().w(px(width)).items_end().child(
        text(value)
            .number_of_lines(1)
            .text_size(px(13.0))
            .tabular_nums()
            .text_color(muted_fg()),
    )
}

/// 曲目列表的列头。
pub fn track_columns(cols: Columns) -> View {
    let label = |s: &'static str| {
        text(s)
            .text_size(px(11.0))
            .font_weight(FontWeight::SEMIBOLD)
            .letter_spacing(px(0.6))
            .text_color(faint())
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .h(px(32.0))
        .px(px(12.0))
        .mx(px(20.0))
        .border_b_1()
        .border_b_color(border())
        .child(when(fits_number, move || view().w(px(W_NUMBER)).items_center().child(label("#"))))
        .child(cols.cover.then(|| view().w(px(W_COVER))))
        .child(view().flex_1().min_w(px(60.0)).child(label("标题")))
        .child(cols.album.then(|| when(fits_album, move || view().w(px(W_ALBUM)).child(label("专辑")))))
        .child(cols.size.then(|| when(fits_size, move || view().w(px(W_SIZE)).items_end().child(label("大小")))))
        .child(view().w(px(W_TIME)).items_end().child(label("时长")))
}

/// 曲目行高。
pub const ROW: f32 = 56.0;

/// 一行曲目：序号（当前曲目为音波）、封面、曲名 + 歌手、专辑、时长。
/// `track` 随单元格复用而变化。
pub fn track_row(
    track: Memo<Option<TrackEntry>>,
    number: Memo<String>,
    cols: Columns,
    on_play: impl Fn() + 'static,
) -> View {
    let s = store();
    let current = computed(move || {
        track.with(|t| {
            let Some(t) = t else { return false };
            s.track.with(|now| now.path.as_deref() == Some(t.path.as_str()))
        })
    });
    let playing = move || current.get() && s.playback.with(|p| p.playing);
    let field = move |f: fn(&TrackEntry) -> String| move || track.with(|t| t.as_ref().map(f).unwrap_or_default());
    view()
        .h(px(ROW))
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .px(px(12.0))
        .rounded(px(10.0))
        .role(AccessibilityRole::Button)
        .label(field(|t| t.title.clone()))
        .style_when(move || current.get(), |st| st.bg(hover()))
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.7))
        .on_press(on_play)
        .child(when(fits_number, move || {
            view()
                .w(px(W_NUMBER))
                .items_center()
                .justify_center()
                .child(
                    when(move || current.get(), move || {
                        icon_dyn(move || if playing() { Icon::Wave } else { Icon::Play }, 14.0, primary)
                    })
                    .otherwise(move || {
                        text(move || number.get())
                            .text_size(px(13.0))
                            .tabular_nums()
                            .text_color(faint())
                    }),
                )
        }))
        .child(cols.cover.then(|| {
            let src = move || track.with(|t| t.as_ref().and_then(|t| super::store::cover_of(&t.path)));
            cover(src, Some(40.0), 6.0)
        }))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    text(field(|t| t.title.clone()))
                        .number_of_lines(1)
                        .text_size(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || if current.get() { primary() } else { fg() }),
                )
                .child(
                    text(field(|t| t.artist.clone()))
                        .number_of_lines(1)
                        .text_size(px(12.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(cols.album.then(|| when(fits_album, move || {
            view().w(px(W_ALBUM)).child(
                text(field(|t| t.album.clone()))
                    .number_of_lines(1)
                    .text_size(px(13.0))
                    .text_color(muted_fg()),
            )
        })))
        .child(cols.size.then(|| when(fits_size, move || value_cell(W_SIZE, field(|t| format_bytes(t.size_bytes))))))
        .child(value_cell(W_TIME, field(|t| format_time(t.duration))))
}

/// 横向拖动取比例（0..1）：按下/移动调 `on_move`，抬起调 `on_release`。
pub fn scrub(
    area: View,
    on_move: impl Fn(f64) + 'static,
    on_release: impl Fn(f64) + 'static,
) -> View {
    let width = signal(1.0_f32);
    let fraction = move |x: f32| {
        let w = width.get_untracked();
        if w > 0.0 { (x / w).clamp(0.0, 1.0) as f64 } else { 0.0 }
    };
    let on_move = Rc::new(on_move);
    let grant = on_move.clone();
    area.on_layout(move |frame: &Bounds<Points>| width.set(frame.size.width.0))
        .claim_touches(TouchClaim::on_start())
        .keep_touches(true)
        .on_responder_grant(move |e: &ResponderEvent| grant(fraction(e.location.x.0)))
        .on_responder_move(move |e: &ResponderEvent| on_move(fraction(e.location.x.0)))
        .on_responder_release(move |e: &ResponderEvent| on_release(fraction(e.location.x.0)))
}

/// 细进度条（`fraction` 0..1），轨道与填充颜色。
pub fn thin_bar(
    fraction: impl Fn() -> f64 + 'static,
    track: AdaptiveColor,
    fill: AdaptiveColor,
) -> View {
    view()
        .h(px(18.0))
        .justify_center()
        .group()
        .child(
            view()
                .h(px(4.0))
                .w_full()
                .rounded_full()
                .bg(track)
                .overflow_hidden()
                .pointer_events(PointerEvents::None)
                .child(
                    view()
                        .h_full()
                        .rounded_full()
                        .bg(fill)
                        .w(move || relative(fraction().clamp(0.0, 1.0) as f32)),
                ),
        )
}

/// 当前播放进度（拖动时为预览）。
pub fn shown_time() -> f64 {
    let s = store();
    s.seek_preview.get().unwrap_or_else(|| s.playback.with(|p| p.time))
}

/// 当前进度比例。
pub fn progress_fraction() -> f64 {
    let s = store();
    let duration = s.playback.with(|p| p.duration);
    if duration > 0.0 { (shown_time() / duration).min(1.0) } else { 0.0 }
}
