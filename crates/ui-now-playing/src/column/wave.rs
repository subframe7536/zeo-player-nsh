use super::*;

/// 细波形进度：[`zeo_metadata::waveform::BARS`] 根圆头竖条，已播亮、未播暗；
/// 播放头所在的那根和进度条一样只亮到播放头为止。可点可拖，抬起 seek。
/// 下方：已播时间、音频信息、总时长。
pub(crate) fn waveform_seek() -> View {
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
            let fallback = [zeo_metadata::waveform::WaveformOptions::default().min;
                zeo_metadata::waveform::BARS];
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
            false,
            "播放进度",
            progress_fraction,
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
