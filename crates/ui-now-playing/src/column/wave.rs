use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use nsh_core::canvas::{FillRule, Num, PathOp};

use super::*;

/// 竖条长高补间的时长（秒）。
const GROW_SECS: f32 = 0.45;

/// 排一帧长高补间：`grow` 按 ease-out-cubic 0→1；`generation` 代次不
/// 等了（有更新的波形变化）旧补间就停手。`Spawned` 丢弃不取消任务
///（`spawn_future` 本就 detach），任务随 owner 结束。
fn schedule_grow(grow: Signal<f32>, generation: Rc<Cell<u64>>, my: u64, start: Instant) {
    request_animation_frame(move |now: Instant| {
        let t = (now.duration_since(start).as_secs_f32() / GROW_SECS).min(1.0);
        grow.set(1.0 - (1.0 - t).powi(3));
        if t < 1.0 && generation.get() == my {
            schedule_grow(grow, generation, my, start);
        }
    });
}

/// 细波形进度：[`zeo_metadata::waveform::BARS`] 根圆头竖条，已播亮、未播暗；
/// 播放头所在的那根和进度条一样只亮到播放头为止。可点可拖，抬起 seek。
/// 下方：已播时间、音频信息、总时长。
pub(crate) fn waveform_seek() -> View {
    let s = store();
    let width = signal(0.0_f32);
    // 竖条从下限高度长到目标：进页与每次 `s.waveform` 变化（换曲、波形
    // 载入）都重播 ~450ms ease-out；代次号让新变化顶掉还在跑的旧补间。
    let grow = signal(0.0_f32);
    let grow_gen = Rc::new(Cell::new(0u64));
    effect(move || {
        let _ = s.waveform.get();
        let my = grow_gen.get() + 1;
        grow_gen.set(my);
        grow.set(0.0);
        schedule_grow(grow, grow_gen.clone(), my, Instant::now());
    });
    let duration = move || s.playback.with(|p| p.duration);
    const H: f32 = 26.0;
    // 播放头横坐标（点）：画进 canvas 的是读它的槽位，原生侧逐帧取用——
    // 拖动时 `on_move` 直写它，已播部分当帧跟上指针，不用等 `seek_preview`
    // 通知 painter 重画再交回原生侧的往返（那会让亮条拖一拍）。平时由
    // effect 跟随进度（100 ms 轮询、拖动预览与列宽都在源里）。
    let head = animated_value(0.0);
    effect(move || {
        let w = width.get();
        head.set(progress_fraction() as f32 * w);
    });
    let painter = move || {
        let w = width.get();
        let grow_v = grow.get();
        let mut p = Painter::new();
        // 亮条裁切矩形的宽：槽位编号在录制时定下，值由原生侧画时读取。
        let head_n = p.num(&head);
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
            // 下限是归一化的 `WaveformOptions::min`：grow=0 时全部贴
            // 下限（即最小条高），grow=1 时回到目标高度。
            let floor = zeo_metadata::waveform::WaveformOptions::default().min as f32;
            let radius = px(bar / 2.0);
            let mut rects = Vec::with_capacity(n);
            for (i, amp) in bars.iter().enumerate() {
                // 已归一化到 0.1..0.9（waveform.rs）：floor→amp 按 grow 插值。
                let a = floor + ((*amp as f32).clamp(0.0, 1.0) - floor) * grow_v;
                let bh = (a * H).max(bar);
                let x = i as f32 * step + (step - bar) / 2.0;
                rects.push(Bounds::new(
                    point(px(x), px((H - bh) / 2.0)),
                    size(px(bar), px(bh)),
                ));
            }
            // 已播部分 = 亮条裁到 [0, head]：头左侧全亮、头下的那根只亮
            // 到头、右侧全暗，与逐根算 `played` 同效，但裁切宽随槽位原生侧
            // 变化，painter 本身不用因进度重录。
            for &rect in &rects {
                p = std::mem::take(&mut p)
                    .fill_color(dim)
                    .begin_path()
                    .round_rect(rect, radius)
                    .fill();
            }
            let clip: Arc<[PathOp]> = Arc::new([PathOp::Rect([
                Num::Const(0.0),
                Num::Const(0.0),
                head_n,
                Num::Const(H),
            ])]);
            p = std::mem::take(&mut p)
                .save()
                .clip_path(&clip, FillRule::NonZero);
            for &rect in &rects {
                p = std::mem::take(&mut p)
                    .fill_color(lit)
                    .begin_path()
                    .round_rect(rect, radius)
                    .fill();
            }
            p = std::mem::take(&mut p).restore();
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
            move |f| {
                head.set(f as f32 * width.get_untracked());
                s.seek_preview.set(Some(f * duration()));
            },
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
