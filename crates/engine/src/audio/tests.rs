use super::*;

#[test]
fn fade_ramps_over_the_configured_time() {
    // 200 ms @ 48 kHz：9600 帧从 0 到 1。
    let step = fade_step(200, 48_000);
    assert!((1.0 / step - 9600.0).abs() < 0.5);
    assert_eq!(fade_step(0, 48_000), 1.0, "关闭时一步到位");
    assert_eq!(fade_curve(0.0), 0.0);
    assert_eq!(fade_curve(1.0), 1.0);
    assert!((fade_curve(0.5) - 0.5).abs() < 1e-6);
    let shared = Shared {
        playing: AtomicBool::new(true),
        loading: AtomicBool::new(false),
        ready: AtomicBool::new(true),
        volume: AtomicU32::new(1f32.to_bits()),
        played: AtomicU64::new(0),
        listened: AtomicU64::new(0),
        dev_rate: AtomicU32::new(48_000),
        ring: {
            let ring = Ring::new();
            ring.init(48_000 * 2);
            assert_eq!(ring.push(&vec![1.0_f32; 48_000 * 2], 2), 48_000 * 2);
            ring
        },
        track: Mutex::new(Arc::new(TrackPublic::default())),
        fade_ms: AtomicU32::new(200),
        fade_target: AtomicU32::new(1f32.to_bits()),
        gain: AtomicU32::new(0f32.to_bits()),
        loop_mode: AtomicU8::new(0),
        cover_gen: AtomicU64::new(0),
        pause_on_device_change: AtomicBool::new(true),
        stats_min_percent: AtomicU32::new(DEFAULT_STATS_MIN_PERCENT),
        stats_min_secs: AtomicU32::new(DEFAULT_STATS_MIN_SECS),
        plays_gen: AtomicU64::new(0),
        rg_factor: AtomicU32::new(1f32.to_bits()),
        rg_config: Mutex::new(ReplayGainConfig::default()),
        eq_gen: AtomicU64::new(0),
        eq_config: Mutex::new(crate::eq::EqConfig::default()),
    };
    // 100 ms 后到一半（smoothstep(0.5) = 0.5），200 ms 后满。
    let mut eq = crate::eq::Equalizer::new();
    let mut starved = false;
    let mut buf = vec![0.0_f32; 4800 * 2];
    output_callback(&shared, &mut eq, &mut starved, &mut buf, 2);
    assert!(
        (buf[buf.len() - 1] - 0.5).abs() < 0.01,
        "{}",
        buf[buf.len() - 1]
    );
    assert!(buf.windows(2).all(|w| w[1] >= w[0] - 1e-6), "淡入单调");
    output_callback(&shared, &mut eq, &mut starved, &mut buf, 2);
    assert!((buf[buf.len() - 1] - 1.0).abs() < 1e-3);
    // 淡出：目标 0，200 ms 后静音。
    shared.fade_target.store(0f32.to_bits(), Ordering::Relaxed);
    let mut long = vec![0.0_f32; 9600 * 2];
    output_callback(&shared, &mut eq, &mut starved, &mut long, 2);
    assert!(
        long[long.len() - 1].abs() < 1e-6
            && f32::from_bits(shared.gain.load(Ordering::Relaxed)) == 0.0
    );
}

/// 播放统计阈值：任一条件满足即计；0 关闭该条件；两个都 0 立即计。
#[test]
fn play_stats_threshold_rules() {
    // 默认 60% / 30 秒，60 秒的歌。
    assert!(!counts_as_play(20.0, 60.0, 60, 30)); // 33% 且 <30s
    assert!(counts_as_play(30.0, 60.0, 60, 30)); // 秒数到了
    assert!(counts_as_play(36.0, 60.0, 60, 30)); // 60% 到了
    assert!(counts_as_play(60.0, 100.0, 60, 30)); // 恰好 60%
    // 0 = 不用该条件。
    assert!(counts_as_play(1.0, 600.0, 0, 1)); // 只按秒
    assert!(!counts_as_play(0.5, 600.0, 0, 1));
    assert!(counts_as_play(300.0, 600.0, 50, 0)); // 只按百分比
    assert!(!counts_as_play(299.0, 600.0, 50, 0));
    // 两个都 0：开始播放即计。
    assert!(counts_as_play(0.0, 600.0, 0, 0));
    // 时长未知 / 为 0：百分比条件不生效，秒数仍可用。
    assert!(!counts_as_play(20.0, 0.0, 60, 30));
    assert!(counts_as_play(30.0, 0.0, 60, 30));
}

/// 环形缓冲：按序取出、绕回、只写整帧、`clear` 作废未播样本。
#[test]
fn ring_keeps_order_frames_and_discards_on_clear() {
    let ring = Ring::new();
    assert_eq!(ring.push(&[1.0, 2.0], 2), 0, "未分配时什么也不写");
    ring.init(8);
    assert_eq!(ring.push(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 2), 6);
    assert_eq!(ring.push(&[7.0, 8.0, 9.0], 2), 2, "放不下的半帧留给调用方");
    let mut got = Vec::new();
    assert_eq!(ring.pop(4, |_, s| got.push(s)), 4);
    assert_eq!(got, [1.0, 2.0, 3.0, 4.0]);
    // 绕回到开头的槽。
    assert_eq!(ring.push(&[9.0, 10.0, 11.0, 12.0], 2), 4);
    got.clear();
    ring.pop(usize::MAX, |_, s| got.push(s));
    assert_eq!(got, [5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0]);
    // clear 之后只剩新写入的样本。
    ring.push(&[1.0, 1.0], 2);
    ring.clear();
    assert_eq!(ring.len(), 0);
    ring.push(&[2.0, 2.0], 2);
    got.clear();
    ring.pop(usize::MAX, |_, s| got.push(s));
    assert_eq!(got, [2.0, 2.0]);
}

/// 两个线程同时写读：消费者拿到的是连续递增的序列，不丢不重。
#[test]
fn ring_is_consistent_across_threads() {
    let ring = Arc::new(Ring::new());
    ring.init(1024);
    const TOTAL: usize = 200_000;
    let producer = {
        let ring = ring.clone();
        std::thread::spawn(move || {
            let mut next = 0usize;
            while next < TOTAL {
                let chunk: Vec<f32> = (next..(next + 100).min(TOTAL)).map(|v| v as f32).collect();
                next += ring.push(&chunk, 1);
                std::thread::yield_now();
            }
        })
    };
    let mut expected = 0usize;
    while expected < TOTAL {
        ring.pop(256, |_, s| {
            assert_eq!(s as usize, expected);
            expected += 1;
        });
        std::thread::yield_now();
    }
    producer.join().unwrap();
}

/// 44.1 kHz → 48 kHz（Windows 共享模式的常见组合），按 MP3 / FLAC 那样
/// 大小不一的包送入：输出连续、没有插入的静音或跳变，长度符合比例。
#[test]
fn resampling_odd_packets_is_continuous() {
    let (from, to, ch) = (44_100_u32, 48_000_u32, 2_usize);
    let freq = 1000.0_f32;
    let total = from as usize * 2; // 2 s
    let input: Vec<f32> = (0..total)
        .flat_map(|i| {
            let v = (i as f32 * freq * std::f32::consts::TAU / from as f32).sin() * 0.5;
            [v, v]
        })
        .collect();
    let mut r = ResamplerState::new(from, to, ch).unwrap();
    let mut out = Vec::new();
    let sizes = [1152_usize, 4096, 576, 1000, 4608, 17];
    let (mut at, mut k) = (0, 0);
    while at < total {
        let n = sizes[k % sizes.len()].min(total - at);
        r.push(&input[at * ch..(at + n) * ch], &mut out).unwrap();
        at += n;
        k += 1;
    }
    let frames = out.len() / ch;
    let expected = total as f64 * to as f64 / from as f64;
    assert!(
        (frames as f64) > expected - 3000.0 && (frames as f64) <= expected + 10.0,
        "{frames} vs {expected}"
    );
    // 跳过开头的滤波器延迟，此后相邻样本的差不超过正弦的最大斜率（加余量）。
    let max_step = 0.5 * freq * std::f32::consts::TAU / to as f32 * 1.3;
    let left: Vec<f32> = out.iter().step_by(ch).copied().collect();
    let worst = left[2048..]
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0_f32, f32::max);
    assert!(worst < max_step, "跳变 {worst} ≥ {max_step}");
}

/// 设备事件决策：Rerouted 不重建；Lost 播放中一定暂停；Invalidated
/// 播放中按设置决定是否暂停、但总是重建。
#[test]
fn device_plan_decides_rebuild_and_pause() {
    for ev in [
        DeviceEvent::Rerouted,
        DeviceEvent::Invalidated,
        DeviceEvent::Lost,
    ] {
        for playing in [false, true] {
            for pref in [false, true] {
                let (rebuild, pause) = device_plan(ev, playing, pref);
                assert_eq!(
                    rebuild,
                    ev != DeviceEvent::Rerouted,
                    "{ev:?} playing={playing} pref={pref}"
                );
                let want_pause = playing && (pref || ev == DeviceEvent::Lost);
                assert_eq!(pause, want_pause, "{ev:?} playing={playing} pref={pref}");
            }
        }
    }
    // 关键断言单独点出来（上表已覆盖，这里是语义文档）。
    assert_eq!(
        device_plan(DeviceEvent::Invalidated, true, false),
        (true, false)
    );
    assert_eq!(device_plan(DeviceEvent::Lost, true, false), (true, true));
}

/// 换到高规格设备后填充目标不超过容量的 2/3（容量只有一份）。
#[test]
fn device_target_never_exceeds_capacity() {
    let out = Output {
        sink: None,
        rate: 192_000,
        channels: 8,
        running: false,
    };
    let capacity = 48_000 * 3 * 2;
    let target = target_for(&out, capacity);
    assert!(target > 0);
    assert!(
        target <= capacity * 2 / 3,
        "{target} > {}",
        capacity * 2 / 3
    );
}

/// ReplayGain 增益：模式回退、预放大叠加、峰值限幅与上限钳制。
#[test]
fn replaygain_factor_follows_mode_and_fallbacks() {
    use crate::audio::ReplayGainMode::*;
    use zeo_metadata::ReplayGain;
    let cfg = |mode, preamp, fallback, clip| ReplayGainConfig {
        mode,
        preamp_db: preamp,
        fallback_db: fallback,
        clip_protect: clip,
    };
    let rg = |tg, tp, ag, ap| ReplayGain {
        track_gain: tg,
        track_peak: tp,
        album_gain: ag,
        album_peak: ap,
    };
    let near = |got: f32, want: f32, what: &str| {
        assert!((got - want).abs() < 1e-3, "{what}: {got} != {want}");
    };
    // 关闭 → 1.0；按曲目 −6.02 dB → ≈0.5。
    assert_eq!(
        replaygain_factor(&cfg(Off, 0.0, 0.0, true), &rg(Some(-6.0), None, None, None)),
        1.0
    );
    near(
        replaygain_factor(
            &cfg(Track, 0.0, 0.0, true),
            &rg(Some(-6.02), None, None, None),
        ),
        0.5,
        "track -6.02dB",
    );
    // 专辑模式没有专辑增益 → 回退曲目增益。
    near(
        replaygain_factor(
            &cfg(Album, 0.0, 0.0, true),
            &rg(Some(-6.02), None, None, None),
        ),
        0.5,
        "album 回退 track",
    );
    // 专辑模式有专辑增益 → 用专辑的。
    near(
        replaygain_factor(
            &cfg(Album, 0.0, 0.0, true),
            &rg(Some(-6.0), None, Some(-12.0), None),
        ),
        10f32.powf(-12.0 / 20.0),
        "album gain",
    );
    // 都没有 → 无标签增益。
    near(
        replaygain_factor(&cfg(Track, 0.0, -3.0, true), &rg(None, None, None, None)),
        10f32.powf(-3.0 / 20.0),
        "fallback",
    );
    // 预放大 +6 dB 与曲目 −6 dB 抵消 → ≈1。
    near(
        replaygain_factor(
            &cfg(Track, 6.0, 0.0, true),
            &rg(Some(-6.0), None, None, None),
        ),
        1.0,
        "preamp 叠加",
    );
    // 防削波：+6 dB 增益但峰值 0.8 → 钳到 1/0.8。
    near(
        replaygain_factor(
            &cfg(Track, 0.0, 0.0, true),
            &rg(Some(6.0), Some(0.8), None, None),
        ),
        1.0 / 0.8,
        "clip protect",
    );
    // 关掉防削波 → 全额 +6 dB。
    near(
        replaygain_factor(
            &cfg(Track, 0.0, 0.0, false),
            &rg(Some(6.0), Some(0.8), None, None),
        ),
        10f32.powf(6.0 / 20.0),
        "no clip protect",
    );
    // 上限钳到 4.0（+12 dB）。
    assert_eq!(
        replaygain_factor(
            &cfg(Track, 0.0, 0.0, false),
            &rg(Some(30.0), None, None, None)
        ),
        4.0
    );
}

/// 设备缓冲请求：目标 ~40ms；声明了范围的钳进范围，未知的直接要固定值。
#[test]
fn output_buffer_size_targets_forty_ms() {
    use cpal::{BufferSize, SupportedBufferSize};
    // 48 kHz → 1920 帧（40ms），落在范围内不动。
    assert_eq!(
        desired_buffer_size(&SupportedBufferSize::Range { min: 15, max: 4096 }, 48_000),
        BufferSize::Fixed(1920)
    );
    // 上限更小 / 下限更高时分别钳到端点。
    assert_eq!(
        desired_buffer_size(
            &SupportedBufferSize::Range {
                min: 128,
                max: 1024
            },
            48_000
        ),
        BufferSize::Fixed(1024)
    );
    assert_eq!(
        desired_buffer_size(
            &SupportedBufferSize::Range {
                min: 4096,
                max: 8192
            },
            44_100
        ),
        BufferSize::Fixed(4096)
    );
    // 范围未知（WASAPI 软栈常态）也要固定值——失败由调用方退默认。
    assert_eq!(
        desired_buffer_size(&SupportedBufferSize::Unknown, 48_000),
        BufferSize::Fixed(1920)
    );
}

/// 峰值限幅不会让输出超过 1.0。
#[test]
fn replaygain_factor_never_clips() {
    let cfg = ReplayGainConfig {
        mode: ReplayGainMode::Track,
        preamp_db: 0.0,
        fallback_db: 0.0,
        clip_protect: true,
    };
    let rg = zeo_metadata::ReplayGain {
        track_gain: Some(6.0),
        track_peak: Some(0.8),
        ..Default::default()
    };
    let factor = replaygain_factor(&cfg, &rg);
    assert!(
        (factor - 1.0 / 0.8).abs() < 1e-3,
        "{factor} != {}",
        1.0 / 0.8
    );
    assert!(factor <= 4.0);
}

/// 智能洗牌：同 seed 同输入必同输出；结果是全排列；同一歌手的曲目
/// 不再连成大段（对照均匀摊开的环上落点）。
#[test]
fn spread_order_is_deterministic_and_spreads_artists() {
    let artists: Vec<&str> = (0..30).map(|i| ["A", "B", "C"][i % 3]).collect();
    let order = spread_order(30, |i| artists[i], |_| 0.0, 42);
    assert_eq!(
        order,
        spread_order(30, |i| artists[i], |_| 0.0, 42),
        "同 seed 必须确定"
    );
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(sorted, (0..30).collect::<Vec<_>>(), "必须是全排列");
    // 相邻同歌手的最长连段：纯随机 30 首 3 歌手常见连段 ≥4；摊开应 ≤2。
    let mut run = 1;
    let mut worst = 1;
    for w in order.windows(2) {
        run = if artists[w[0]] == artists[w[1]] {
            run + 1
        } else {
            1
        };
        worst = worst.max(run);
    }
    assert!(worst <= 2, "同歌手最长连段 {worst} > 2");
    // 换 seed 应得不同顺序（极小概率相同就再换一个）。
    let other = spread_order(30, |i| artists[i], |_| 0.0, 7);
    assert_ne!(order, other, "不同 seed 顺序应不同");
}

/// 组内按 `rand × (score+1)` 加权：同一歌手的高分曲目更常占组内
/// 前位（多 seed 汇总，避免单次偶然）。
#[test]
fn spread_order_weights_high_score_first_within_group() {
    // 一个歌手 4 首：index 0 得分 100，其余 0。
    let artists = vec!["S"; 4];
    let mut first = 0usize;
    for seed in 0..64u64 {
        let order = spread_order(
            4,
            |i| artists[i],
            |i| if i == 0 { 100.0 } else { 0.0 },
            seed,
        );
        if order[0] == 0 {
            first += 1;
        }
    }
    assert!(first >= 40, "高分曲在队首 {first}/64 次，加权应明显偏前");
}
