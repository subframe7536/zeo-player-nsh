//! 全屏播放的左列：封面、曲名与「更多」、歌手 / 专辑、标签、细波形进度
//! 与音频信息、线框控制按钮、音量。

use nsh_base::{menu, menu_item, menu_separator};
use nsh_core::prelude::*;

use super::super::components::button::{on_dark, tipped};
use super::super::components::icons::{Icon, icon, icon_dyn};
use super::super::components::slider::{scrub, slider};
use super::super::store::{
    format_time, player, progress_fraction, seek_to, set_volume, shown_time, store, toggle_mute,
};
use super::super::theme::*;
use super::super::views::playlists;
use super::{REVEAL_LABEL, Swap, reveal};
use crate::engine::audio::LoopMode;
use crate::metadata::MetaInfo;

/// 左列宽度（= 封面边长）。
const COLUMN: f32 = 326.0;

/// 质量标签：Hi-Res（无损且高于 16-bit / 48 kHz）、Lossless、HQ（有损 ≥ 256 kbps）。
fn quality_tag(m: &MetaInfo) -> Option<String> {
    match m.quality.as_deref() {
        Some("Lossless")
            if m.bit_depth.unwrap_or(16) > 16 || m.sample_rate.unwrap_or(44_100) > 48_000 =>
        {
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
        parts.push(if khz.fract() == 0.0 {
            format!("{khz:.0}kHz")
        } else {
            format!("{khz:.1}kHz")
        });
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

pub(super) fn left_column(swap: &Swap) -> View {
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
        .child(when(
            move || artwork().is_some(),
            move || {
                image(move || artwork().unwrap_or_default())
                    .absolute()
                    .inset(px(0.0))
                    .resize_mode(ResizeMode::Cover)
            },
        ));

    let title_row = view()
        .h(px(28.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text(move || {
                s.track.with(|t| {
                    if t.title.is_empty() {
                        "Zeo Player".into()
                    } else {
                        t.title.clone()
                    }
                })
            })
            .flex_1()
            .number_of_lines(1)
            .text_size(px(20.0))
            .line_height(px(20.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(white_a(0.95)),
        )
        .child(more_menu());

    let meta_line = |glyph: Icon, value: fn(&super::super::store::NowPlaying) -> String| {
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
                    .line_height(px(13.0))
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
            let genres = crate::config::split_genres(&m.genres, &separators);
            out.extend(genres.into_iter().take(3).map(|g| (g, false)));
            if let Some(codec) = &m.codec {
                out.push((codec.clone(), false));
            }
            out
        });
        (!list.is_empty()).then(|| {
            view()
                .flex_row()
                .gap(px(8.0))
                .children(list.into_iter().map(|(label, strong)| {
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
                                .line_height(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(white_a(0.85)),
                        )
                }))
        })
    });

    // 与播放条一致：两侧是循环模式与喜欢（最小），中间是上一首 / 播放 /
    // 下一首，播放键最大。
    let skip = |glyph: Icon, label: &'static str, press: fn()| {
        tipped(
            on_dark(44.0)
                .accessibility_label(label)
                .on_press(move |_: &()| press())
                .child(icon(glyph, 18.0, || white_a(0.85))),
            label,
        )
    };
    let side = |glyph: Box<dyn Fn() -> Icon>,
                label: &'static str,
                tip: &'static str,
                tint: Box<dyn Fn() -> AdaptiveColor>,
                press: fn()| {
        tipped(
            on_dark(36.0)
                .accessibility_label(label)
                .on_press(move |_: &()| press())
                .child(icon_dyn(glyph, 16.0, tint)),
            tip,
        )
    };
    let play = on_dark(56.0)
        .accessibility_label("播放/暂停")
        .active(|st| st.scale(0.92))
        .transition_transform()
        .duration(120.0)
        .on_press(|_: &()| player().toggle())
        .child(super::super::components::icons::play_pause(
            34.0,
            move || s.playback.with(|p| p.playing),
            || white_a(0.95),
        ));
    let mode_icon = move || match s.playback.with(|p| p.loop_mode) {
        LoopMode::List => Icon::Repeat,
        LoopMode::Single => Icon::RepeatOne,
        LoopMode::Random => Icon::Shuffle,
    };
    let controls = view()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(18.0))
        .child(side(
            Box::new(mode_icon),
            "循环模式（顺序 / 单曲 / 随机）",
            "循环模式",
            Box::new(move || {
                white_a(if s.playback.with(|p| p.loop_mode) == LoopMode::List {
                    0.55
                } else {
                    0.95
                })
            }),
            super::super::store::cycle_loop_mode,
        ))
        .child(skip(Icon::PrevLine, "上一首", || player().previous()))
        .child(play)
        .child(skip(Icon::NextLine, "下一首", || player().next()))
        .child(side(
            Box::new(|| {
                if super::super::store::current_liked() {
                    Icon::HeartFilled
                } else {
                    Icon::Heart
                }
            }),
            "喜欢",
            "喜欢",
            Box::new(like_red),
            super::super::store::toggle_like_current,
        ));

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
                .child(
                    view()
                        .mt(px(14.0))
                        .h(px(20.0))
                        .overflow_hidden()
                        .child(tags),
                ),
        )
        .child(view().pt(px(28.0)).child(waveform_seek()))
        .child(view().pt(px(20.0)).child(controls))
        .child(view().pt(px(18.0)).child(volume_row()))
}

/// 「⋯」：添加到歌单、在访达中显示、拷贝文件路径。
fn more_menu() -> AnyElement {
    let s = store();
    let path = move || s.track.with_untracked(|t| t.path.clone());
    let item = |label: &'static str, glyph: Icon, on_select: Box<dyn Fn()>| {
        menu_item()
            .on_select(on_select)
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .hover(|st| st.bg(hover()))
            .active(|st| st.bg(hover()))
            .child(icon(glyph, 15.0, muted_fg))
            .child(
                text(label)
                    .text_size(px(13.0))
                    .line_height(px(13.0))
                    .text_color(fg()),
            )
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
        .child(playlists::add_to_submenu(move || {
            path().into_iter().collect()
        }))
        .child(menu_separator().h(px(1.0)).my(px(4.0)).bg(border()))
        .child(item(
            REVEAL_LABEL,
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

/// 细波形进度：[`crate::metadata::waveform::BARS`] 根圆头竖条，已播亮、未播暗；
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
            let fallback = [crate::metadata::waveform::WaveformOptions::default().min;
                crate::metadata::waveform::BARS];
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
                    p = std::mem::take(&mut p)
                        .fill_color(dim)
                        .begin_path()
                        .round_rect(rect, radius)
                        .fill();
                }
                if played >= bar {
                    p = std::mem::take(&mut p)
                        .fill_color(lit)
                        .begin_path()
                        .round_rect(rect, radius)
                        .fill();
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
    let small = |t: Text| {
        t.text_size(px(12.0))
            .line_height(px(12.0))
            .tabular_nums()
            .text_color(white_a(0.75))
    };
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
                    text(move || {
                        s.meta
                            .with(|m| m.as_ref().map(|m| audio_line(m)).unwrap_or_default())
                    })
                    .flex_1()
                    .text_center()
                    .number_of_lines(1)
                    .text_size(px(10.5))
                    .line_height(px(10.5))
                    .text_color(white_a(0.45)),
                )
                .child(
                    small(text(move || format_time(duration())))
                        .w(px(48.0))
                        .text_right(),
                ),
        )
}

fn volume_row() -> View {
    let s = store();
    let volume = move || s.playback.with(|p| p.volume);
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(tipped(
            on_dark(28.0)
                .accessibility_label("静音")
                .on_press(|_: &()| toggle_mute())
                .child(icon_dyn(
                    move || {
                        if volume() <= 0.0 {
                            Icon::Mute
                        } else {
                            Icon::Volume
                        }
                    },
                    15.0,
                    || white_a(0.7),
                )),
            "静音",
        ))
        .child(
            slider(
                volume,
                white_a(0.2),
                white_a(0.9),
                white_a(1.0),
                set_volume,
                set_volume,
            )
            .flex_1(),
        )
        .child(
            text(move || format!("{}", (volume() * 100.0).round()))
                .w(px(26.0))
                .text_right()
                .text_size(px(11.0))
                .line_height(px(11.0))
                .tabular_nums()
                .text_color(white_a(0.55)),
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
        let hires = MetaInfo {
            bit_depth: Some(24),
            sample_rate: Some(96_000),
            bit_rate: Some(2304),
            ..meta()
        };
        assert_eq!(audio_line(&hires), "24-bit / 96kHz / 2304kbps / Stereo");
    }

    #[test]
    fn quality_tags() {
        assert_eq!(quality_tag(&meta()), None, "160 kbps 有损不标");
        assert_eq!(
            quality_tag(&MetaInfo {
                bit_rate: Some(320),
                ..meta()
            })
            .as_deref(),
            Some("HQ")
        );
        let flac = MetaInfo {
            quality: Some("Lossless".into()),
            bit_depth: Some(16),
            ..meta()
        };
        assert_eq!(quality_tag(&flac).as_deref(), Some("Lossless"));
        let hires = MetaInfo {
            bit_depth: Some(24),
            ..flac
        };
        assert_eq!(quality_tag(&hires).as_deref(), Some("Hi-Res"));
    }
}
