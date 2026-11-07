//! 设置 →「播放」分类的各节。

use nsh_core::prelude::*;

use super::super::super::components::button::{Size, Variant, text_button};
use super::super::super::components::form::{input_field, select};
use super::super::super::components::slider::{SliderStyle, slider};
use super::super::super::store::store;
use super::super::super::theme::*;
use super::{pref_row, setting_section};

/// 播放：淡入淡出时长（毫秒，数字输入；0 为关闭）。
pub(super) fn playback_section() -> View {
    let s = store();
    setting_section(
        "淡入淡出",
        "播放、暂停、拖动进度和切歌时，音量在这段时间里渐变，不会突然响起或戛然而止。一首自然播完接下一首时无缝接上。",
        None,
    )
    .child(int_row(
        "淡入淡出时长",
        "0 为关闭，最大 5000 毫秒。",
        "淡入淡出时长",
        "毫秒",
        5000,
        move || s.library.with(|l| l.config.fade_ms),
        super::super::super::store::set_fade_ms,
    ))
}

/// 整数输入行：标题 + 说明 | 草稿制数字输入 + 单位（非法回退，提交钳到 `max`）。
fn int_row(
    title: &'static str,
    desc: &'static str,
    label: &'static str,
    unit: &'static str,
    max: u32,
    get: impl Fn() -> u32 + Copy + 'static,
    set: impl Fn(u32) + Copy + 'static,
) -> View {
    let draft = signal(format!("{}", get()));
    // 外部改了配置（另一个实例、测试）时同步草稿；computed 拦掉无关的
    // s.library 写，不顶掉输入中的草稿。
    let lib_v = computed(move || get());
    effect(move || draft.set(lib_v.get().to_string()));
    let commit = move || match draft.get().trim().parse::<u32>() {
        Ok(v) => {
            let v = v.min(max);
            set(v);
            draft.set(v.to_string());
        }
        Err(_) => draft.set(format!("{}", get())),
    };
    let field = input_field(true)
        .w(px(88.0))
        .label(label)
        .keyboard_type(KeyboardType::Number)
        .value(move || draft.get())
        .on_change(move |t: &str| draft.set(t.to_owned()))
        .on_submit(move |_: &str| commit())
        .on_blur(move || commit());
    view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(16.0))
        .child(
            view()
                .flex_1()
                .min_w(px(200.0))
                .gap(px(2.0))
                .child(
                    text(title)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg()),
                )
                .child(
                    text(desc)
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(field)
        .child(
            text(unit)
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
}

/// 播放统计：听过多少算一次播放（用于首页的统计与排行）。
pub(super) fn stats_section() -> View {
    let s = store();
    setting_section(
        "播放统计",
        "听过多少才算一次播放（用于首页的统计与排行）。两个条件任一满足即计；填 0 表示不用该条件；都是 0 则开始播放即计一次。",
        None,
    )
    .child(int_row(
        "最少播放百分比",
        "听到整首的这个比例就算播放过一次。",
        "最少播放百分比",
        "%",
        100,
        move || s.library.with(|l| l.config.stats_min_percent),
        move |v| {
            let secs = s.library.with(|l| l.config.stats_min_secs);
            super::super::super::store::set_stats_thresholds(v, secs)
        },
    ))
    .child(int_row(
        "最少播放时长",
        "实际播出这么多秒就算播放过一次（拖动进度条不计入）。",
        "最少播放时长",
        "秒",
        3600,
        move || s.library.with(|l| l.config.stats_min_secs),
        move |v| {
            let percent = s.library.with(|l| l.config.stats_min_percent);
            super::super::super::store::set_stats_thresholds(percent, v)
        },
    ))
}

/// 输出设备：拔下耳机或切换系统默认输出设备时的行为。
pub(super) fn device_section() -> View {
    let s = store();
    setting_section("输出设备", "拔下耳机或切换系统默认输出设备时的行为。", None).child(pref_row(
        "设备变化时暂停",
        "关闭后会在新的默认设备上继续播放。设备全部断开时总会暂停。",
        move || s.library.with(|l| l.config.pause_on_device_change),
        super::super::super::store::set_pause_on_device_change,
    ))
}

/// dB 输入行：标题 + 说明 | 草稿制数字输入 + `dB`（−15…+15，非法回退）。
fn db_row(
    title: &'static str,
    desc: &'static str,
    label: &'static str,
    get: impl Fn() -> f32 + Copy + 'static,
    set: impl Fn(f32) + Copy + 'static,
) -> View {
    let draft = signal(format!("{}", get()));
    // 外部改了配置（另一个实例、测试）时同步草稿；同 fade 输入——
    // computed 拦掉无关的 s.library 写，不顶掉输入中的草稿。
    let lib_db = computed(move || get());
    effect(move || draft.set(format!("{}", lib_db.get())));
    let commit = move || match draft.get().trim().parse::<f32>() {
        Ok(db) => {
            let db = db.clamp(-15.0, 15.0);
            set(db);
            draft.set(format!("{db}"));
        }
        Err(_) => draft.set(format!("{}", get())),
    };
    let field = input_field(true)
        .w(px(88.0))
        .label(label)
        .keyboard_type(KeyboardType::Number)
        .value(move || draft.get())
        .on_change(move |t: &str| draft.set(t.to_owned()))
        .on_submit(move |_: &str| commit())
        .on_blur(move || commit());
    view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(16.0))
        .child(
            view()
                .flex_1()
                .min_w(px(200.0))
                .gap(px(2.0))
                .child(
                    text(title)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg()),
                )
                .child(
                    text(desc)
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(field)
        .child(
            text("dB")
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(muted_fg()),
        )
}

/// 改 ReplayGain 配置：读当前值、应用修改、立即生效并保存。
fn edit_replaygain(f: impl Fn(&mut crate::engine::audio::ReplayGainConfig)) {
    let mut next = store().library.with(|l| l.config.replaygain);
    f(&mut next);
    super::super::super::store::set_replaygain(next);
}

/// ReplayGain：按标签把不同曲目 / 专辑拉到统一响度（默认关闭）。
pub(super) fn replaygain_section() -> View {
    use crate::engine::audio::ReplayGainMode;
    let s = store();
    let cfg = move || s.library.with(|l| l.config.replaygain);
    const MODES: &[(ReplayGainMode, &str)] = &[
        (ReplayGainMode::Off, "关闭"),
        (ReplayGainMode::Track, "按曲目"),
        (ReplayGainMode::Album, "按专辑"),
    ];
    let on = move || cfg().mode != ReplayGainMode::Off;
    setting_section(
        "ReplayGain",
        "按曲目或专辑里的 ReplayGain 标签调整音量，让不同专辑听起来一样响。只读取已有标签，不会扫描计算。",
        None,
    )
    .child(
        super::super::super::components::radio::group_row().children(MODES.iter().map(
            |&(mode, label)| {
                super::super::super::components::radio::item(
                    move || cfg().mode == mode,
                    label,
                    move || edit_replaygain(|c| c.mode = mode),
                )
            },
        )),
    )
    .child(when(on, move || {
        view()
            .gap(px(16.0))
            .child(db_row(
                "预放大",
                "在标签增益上叠加的固定增益。",
                "ReplayGain 预放大",
                move || s.library.with(|l| l.config.replaygain.preamp_db),
                move |db| edit_replaygain(|c| c.preamp_db = db),
            ))
            .child(db_row(
                "无标签曲目",
                "没有 ReplayGain 标签的曲目使用这个增益。",
                "ReplayGain 无标签增益",
                move || s.library.with(|l| l.config.replaygain.fallback_db),
                move |db| edit_replaygain(|c| c.fallback_db = db),
            ))
            .child(pref_row(
                "防止削波",
                "按标签里的峰值限制增益，避免爆音。",
                move || s.library.with(|l| l.config.replaygain.clip_protect),
                move |on| edit_replaygain(|c| c.clip_protect = on),
            ))
    }))
}

// --- 均衡器 -----------------------------------------------------------------

/// 段名（与 `engine::eq::BANDS` 顺序一致）。
const EQ_LABELS: [&str; 10] = [
    "31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k",
];
/// 各段滑块的无障碍标签（与 `EQ_LABELS` 同序）。
const EQ_A11Y: [&str; 10] = [
    "均衡器 31",
    "均衡器 62",
    "均衡器 125",
    "均衡器 250",
    "均衡器 500",
    "均衡器 1k",
    "均衡器 2k",
    "均衡器 4k",
    "均衡器 8k",
    "均衡器 16k",
];

/// 改均衡器配置：读当前值、应用修改、立即生效（持久化在 store 里防抖）。
fn edit_eq(f: impl Fn(&mut crate::engine::eq::EqConfig)) {
    let mut next = store().library.with(|l| l.config.eq);
    f(&mut next);
    super::super::super::store::set_eq(next);
}

/// 拖推子时的均衡器写入：只跟引擎与内存侧（一把互斥 + 一次克隆，
/// 每个 move 都很便宜），`s.library` 的整库广播留到抬起 `edit_eq`
/// 一次——它有几十个订阅点，拖动中周期广播的突发开销会让滚动区
/// 里的推子顿挫；抬起前 `library_store` 一直拿着最新值。
fn apply_eq_live(f: impl Fn(&mut crate::engine::eq::EqConfig)) {
    let mut next = crate::config::library_store().eq();
    f(&mut next);
    super::super::super::store::player().set_eq(next);
    crate::config::library_store().set_eq(next);
}

/// 「启用均衡器」开关的回调（`pref_row` 要 `fn` 指针，不能捕获）。
fn eq_enabled_set(on: bool) {
    edit_eq(|c| c.enabled = on);
}

/// 吸附到 0.5 dB。
fn snap_db(db: f32) -> f32 {
    (db * 2.0).round() / 2.0
}

/// 当前各段值匹配的预设名；都不匹配时是「自定义」。
fn eq_preset_name(bands: &[f32; 10]) -> String {
    crate::engine::eq::PRESETS
        .iter()
        .find(|(_, b)| b == bands)
        .map(|(name, _)| (*name).to_owned())
        .unwrap_or_else(|| "自定义".to_owned())
}

/// 推子滑杆的高度与每列的拖动区宽。
const EQ_FADER_H: f32 = 148.0;
const EQ_FADER_W: f32 = 48.0;

/// 一条推子列：竖推子 + 标签 + 当前值（`+3.5 dB`）。拖动走
/// `apply_eq_live`（只写引擎与内存），抬起走 `edit_eq`（全量收尾）。
fn eq_fader(
    label: &'static str,
    a11y: &'static str,
    get: impl Fn() -> f32 + Copy + 'static,
    set_db: impl Fn(&mut crate::engine::eq::EqConfig, f32) + Copy + 'static,
) -> View {
    let db = move |f: f64| snap_db((f * 24.0 - 12.0) as f32);
    // 吸附后 0.5 dB 一档，平滑拖动里大多数 move 算出的值不变——
    // 值没变就不写引擎/内存（NaN 哨兵保证每次手势第一次必写）。
    let last = std::rc::Rc::new(std::cell::Cell::new(f32::NAN));
    let lm = last.clone();
    // 拖动中标签显示已应用的吸附值（本地信号，s.library 拖动期不广播）。
    let live_db = signal(None::<f32>);
    view()
        .flex_1()
        .min_w(px(0.0))
        .items_center()
        .gap(px(5.0))
        .child(
            slider(
                SliderStyle::FADER,
                a11y,
                move || {
                    ((get() + crate::engine::eq::MAX_DB) / (2.0 * crate::engine::eq::MAX_DB)) as f64
                },
                input(),
                muted_fg(),
                fg(),
                move |f| {
                    let v = db(f);
                    if v != lm.get() {
                        lm.set(v);
                        live_db.set(Some(v));
                        apply_eq_live(|c| set_db(c, v));
                    }
                },
                move |f| {
                    edit_eq(|c| set_db(c, db(f)));
                    last.set(f32::NAN); // 下一次手势的第一次 move 必写
                    live_db.set(None);
                },
            )
            .w(px(EQ_FADER_W))
            .h(px(EQ_FADER_H)),
        )
        .child(
            text(label)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg()),
        )
        .child(
            text(move || format!("{:+.1} dB", live_db.get().unwrap_or_else(get)))
                .text_size(px(10.0))
                .line_height(px(10.0))
                .tabular_nums()
                .text_color(faint()),
        )
}

/// 左侧 dB 标尺：数值对齐推子行程（不画刻度线）。
fn db_scale() -> View {
    const LABEL_H: f32 = 10.0;
    let mut col = view().w(px(36.0)).h(px(EQ_FADER_H)).flex_shrink_0();
    for db in [12_i32, 6, 0, -6, -12] {
        // 行程比例（0 底 1 顶）→ 标签顶边：标签中心落在行程点上。
        let top = (1.0 - (db + 12) as f32 / 24.0) * (EQ_FADER_H - LABEL_H);
        col = col.child(
            view().absolute().top(px(top)).right(px(0.0)).child(
                text(if db == 0 {
                    "0 dB".to_owned()
                } else {
                    format!("{db:+} dB")
                })
                .text_size(px(LABEL_H))
                .line_height(px(LABEL_H))
                .text_color(faint()),
            ),
        );
    }
    col
}

/// 均衡器：前级 + 10 段竖推子（31–16k，±12 dB，拖动立即生效）。
pub(super) fn eq_section() -> View {
    let s = store();
    let cfg = move || s.library.with(|l| l.config.eq);
    let on = move || cfg().enabled;
    setting_section("均衡器", "按频段提升或衰减音量。拖动立即生效。", None)
        .child(pref_row(
            "启用均衡器",
            "对正在播放的音频实时应用下面的频段增益。",
            move || s.library.with(|l| l.config.eq.enabled),
            eq_enabled_set,
        ))
        .child(when(on, move || {
            let mut faders = view().flex_1().flex_row().child(eq_fader(
                "前级",
                "均衡器 前级",
                move || s.library.with(|l| l.config.eq.preamp_db),
                |c, db| c.preamp_db = db,
            ));
            for (i, label) in EQ_LABELS.iter().enumerate() {
                faders = faders.child(eq_fader(
                    label,
                    EQ_A11Y[i],
                    move || s.library.with(|l| l.config.eq.bands[i]),
                    move |c, db| c.bands[i] = db,
                ));
            }
            let options: Vec<(Option<&'static str>, &'static str, Box<dyn Fn()>)> =
                crate::engine::eq::PRESETS
                    .iter()
                    .map(|&(name, bands)| {
                        (
                            None,
                            name,
                            Box::new(move || edit_eq(|c| c.bands = bands)) as Box<dyn Fn()>,
                        )
                    })
                    .collect();
            view()
                .gap(px(16.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .child(select(
                            move || eq_preset_name(&s.library.with(|l| l.config.eq.bands)),
                            "预设",
                            options,
                            160.0,
                        ))
                        .child(view().flex_1())
                        .child(text_button(
                            Variant::Ghost,
                            Size::Sm,
                            None,
                            "重置",
                            || {
                                edit_eq(|c| {
                                    c.preamp_db = 0.0;
                                    c.bands = [0.0; 10];
                                })
                            },
                        )),
                )
                .child(
                    view()
                        .flex_row()
                        .w_full()
                        .gap(px(10.0))
                        .child(db_scale())
                        .child(faders),
                )
        }))
}
