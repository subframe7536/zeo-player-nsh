//! 10 段图形均衡器：RBJ Audio EQ Cookbook 的 peaking 双二阶，在 cpal
//! 输出回调里实时运行——不分配、不加锁。滤波器状态（每段每声道的
//! z1/z2）随输出流建在回调闭包里；参数经 `Shared` 的代次 + `Mutex`
//! 传入，流重建（设备切换）后按新采样率重算系数。

use serde::{Deserialize, Serialize};

/// 频点（Hz）。
pub const BANDS: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
/// 每段与前级的增益范围（dB）。
pub const MAX_DB: f32 = 12.0;
const Q: f32 = 1.41;
/// 最多声道数（超过的声道不处理 EQ，原样输出）。
pub const MAX_CHANNELS: usize = 8;

/// 均衡器设置（界面 → 引擎，持久化在 `library.json`）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqConfig {
    pub enabled: bool,
    /// 前级增益（dB，−12…+12）。
    pub preamp_db: f32,
    /// 各段增益（dB，−12…+12，按 [`BANDS`] 顺序）。
    pub bands: [f32; 10],
}

impl Default for EqConfig {
    fn default() -> EqConfig {
        EqConfig {
            enabled: false,
            preamp_db: 0.0,
            bands: [0.0; 10],
        }
    }
}

impl EqConfig {
    /// 是否等于直通（关闭，或全部为 0）。
    pub fn is_flat(&self) -> bool {
        !self.enabled || (self.preamp_db == 0.0 && self.bands.iter().all(|b| *b == 0.0))
    }
}

/// 内置预设（名字, 各段 dB）。
pub const PRESETS: &[(&str, [f32; 10])] = &[
    ("平直", [0.0; 10]),
    (
        "流行",
        [-1.0, 1.0, 3.0, 4.0, 3.0, 0.0, -1.0, -1.0, 1.0, 2.0],
    ),
    ("摇滚", [4.0, 3.0, 2.0, 0.0, -1.0, -1.0, 1.0, 2.0, 3.0, 4.0]),
    ("爵士", [3.0, 2.0, 1.0, 2.0, -1.0, -1.0, 0.0, 1.0, 2.0, 3.0]),
    ("古典", [4.0, 3.0, 2.0, 1.0, -1.0, -1.0, 0.0, 2.0, 3.0, 4.0]),
    (
        "人声",
        [-2.0, -2.0, -1.0, 1.0, 3.0, 4.0, 3.0, 1.0, 0.0, -1.0],
    ),
    (
        "低音增强",
        [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    ),
    (
        "高音增强",
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 5.0, 6.0],
    ),
];

/// 直通系数（b0=1，其余 0）：段的「关」不是不处理，而是渐变成恒等，
/// 渐变走完才置 `on=false`——开 / 关段、开关 EQ、拖推子都没有系数突变。
const IDENTITY: [f32; 5] = [1.0, 0.0, 0.0, 0.0, 0.0];

/// 系数渐变的时长占比（≈5 ms）：够盖掉爆音，又短到拖推子跟手。
const RAMP_DIVISOR: u32 = 200;

/// RBJ cookbook peaking EQ；返回已除 a0 的系数（b0,b1,b2,a1,a2）。
fn peaking(f0: f32, rate: f32, db: f32) -> [f32; 5] {
    let a = 10f32.powf(db / 40.0);
    let w0 = 2.0 * std::f32::consts::PI * f0 / rate;
    let alpha = w0.sin() / (2.0 * Q);
    let cos = w0.cos();
    let a0 = 1.0 + alpha / a;
    [
        (1.0 + alpha * a) / a0,
        -2.0 * cos / a0,
        (1.0 - alpha * a) / a0,
        -2.0 * cos / a0,
        (1.0 - alpha / a) / a0,
    ]
}

/// 一组双二阶在 `rate` 下的合成幅频响应峰值（dB）：采样各段中心、
/// 相邻几何中点与频带两端。前级按它下压，让 EQ 的提升不会把输出
/// 推过满幅——不然推子只要往上，响处就被 ±1 硬削出失真。
fn combined_headroom_db(tgts: &[[f32; 5]; 10], rate: f32) -> f32 {
    let mut freqs: Vec<f32> = Vec::with_capacity(22);
    freqs.push(20.0f32.min(0.45 * rate));
    for (i, &f0) in BANDS.iter().enumerate() {
        freqs.push(f0.min(0.45 * rate));
        if let Some(&next) = BANDS.get(i + 1) {
            freqs.push((f0 * next).sqrt().min(0.45 * rate));
        }
    }
    freqs.push(20_000.0f32.min(0.45 * rate));
    let mut max_db = 0.0_f64;
    for f in freqs {
        let w = f64::from(2.0 * std::f32::consts::PI * f / rate);
        let (c1, c2) = (w.cos(), (2.0 * w).cos());
        let mut mag2 = 1.0_f64;
        for c in tgts {
            if *c == IDENTITY {
                continue;
            }
            let (b0, b1, b2, a1, a2) = (
                f64::from(c[0]),
                f64::from(c[1]),
                f64::from(c[2]),
                f64::from(c[3]),
                f64::from(c[4]),
            );
            let num =
                b0 * b0 + b1 * b1 + b2 * b2 + 2.0 * (b0 * b1 + b1 * b2) * c1 + 2.0 * b0 * b2 * c2;
            let den = 1.0 + a1 * a1 + a2 * a2 + 2.0 * (a1 + a1 * a2) * c1 + 2.0 * a2 * c2;
            mag2 *= num / den.max(1e-12);
        }
        max_db = max_db.max(10.0 * mag2.log10());
    }
    (max_db.max(0.0)) as f32
}

/// 一段的系数与逐声道状态（Transposed Direct Form II）。系数朝 `tgt`
/// 逐帧渐变（`Equalizer::ramp_left` 计帧），滤波器状态 z1/z2 改参数
/// 时不清零——渐变期间的瞬态远小于状态清零 + 系数突变。
#[derive(Clone, Copy)]
struct Band {
    /// 是否参与处理（渐变成恒等的过程中保持 true）。
    on: bool,
    /// 当前系数（b0,b1,b2,a1,a2，已除 a0）；`on=false` 时恒为 `IDENTITY`。
    coef: [f32; 5],
    tgt: [f32; 5],
    step: [f32; 5],
    z1: [f32; MAX_CHANNELS],
    z2: [f32; MAX_CHANNELS],
}

impl Band {
    fn off() -> Band {
        Band {
            on: false,
            coef: IDENTITY,
            tgt: IDENTITY,
            step: [0.0; 5],
            z1: [0.0; MAX_CHANNELS],
            z2: [0.0; MAX_CHANNELS],
        }
    }
}

/// 回调侧的 EQ 实例：随输出流创建，按该流的采样率配置。
pub struct Equalizer {
    /// 前级线性增益（渐变中逐帧逼近 `preamp_tgt`）。
    preamp: f32,
    preamp_tgt: f32,
    preamp_step: f32,
    bands: [Band; 10],
    /// 剩余渐变帧数（0 = 系数已在目标上）。
    ramp_left: u32,
    /// 渐变收尾后回到旁路（关闭 / 平直配置的过渡）。
    deactivating: bool,
    /// `false` = 完全旁路（配置为平直或关闭，且过渡已走完）。
    active: bool,
}

impl Equalizer {
    /// 未配置的旁路状态（首块回调读配置前用）。
    pub fn new() -> Equalizer {
        Equalizer {
            preamp: 1.0,
            preamp_tgt: 1.0,
            preamp_step: 0.0,
            bands: [Band::off(); 10],
            ramp_left: 0,
            deactivating: false,
            active: false,
        }
    }

    /// 按 `rate` 算目标系数：增益为 0 或频点 ≥ 0.45·rate 的段目标是恒等。
    /// 已激活时朝新目标渐变（保留滤波器状态），未激活且配置平直时保持旁路。
    pub fn configure(&mut self, cfg: &EqConfig, rate: u32) {
        let rate_f = rate as f32;
        let enabled = cfg.enabled;
        let mut tgts = [IDENTITY; 10];
        if enabled {
            for (tgt, (&f0, &db)) in tgts.iter_mut().zip(BANDS.iter().zip(cfg.bands.iter())) {
                if db != 0.0 && f0 < 0.45 * rate_f {
                    *tgt = peaking(f0, rate_f, db.clamp(-MAX_DB, MAX_DB));
                }
            }
        }
        // 前级先扣除合成响应的峰值：推子往上推多少，整体就压多少，
        // EQ 的净提升不会超过用户的前级设置——提升不再削波。
        let headroom = if enabled {
            combined_headroom_db(&tgts, rate_f)
        } else {
            0.0
        };
        let preamp_tgt = if enabled {
            10f32.powf((cfg.preamp_db.clamp(-MAX_DB, MAX_DB) - headroom) / 20.0)
        } else {
            1.0
        };
        if !self.active && cfg.is_flat() {
            return;
        }
        // 旁路 → 激活也走同一渐变：系数从恒等起步推到目标约 5 ms——
        // 直接落目标的话，非零系数配零状态首样本就有 (b0−1)·x 的台阶。
        self.active = true;
        // 已激活：全部段朝目标渐变（含正在关掉的段——目标恒等）。
        let ramp = (rate / RAMP_DIVISOR).max(64);
        self.ramp_left = ramp;
        self.deactivating = cfg.is_flat();
        self.preamp_tgt = preamp_tgt;
        self.preamp_step = (preamp_tgt - self.preamp) / ramp as f32;
        for (band, &t) in self.bands.iter_mut().zip(tgts.iter()) {
            if !band.on && t == IDENTITY {
                continue;
            }
            band.on = true;
            band.tgt = t;
            for k in 0..5 {
                band.step[k] = (t[k] - band.coef[k]) / ramp as f32;
            }
        }
    }

    /// 渐变推进一帧；收尾时系数落目标、直通段撤 `on`、按 `deactivating`
    /// 关旁路（关闭 EQ 是淡出到直通，不是瞬时断）。
    fn advance(&mut self) {
        self.ramp_left -= 1;
        if self.ramp_left > 0 {
            self.preamp += self.preamp_step;
            for band in self.bands.iter_mut() {
                if band.on {
                    for k in 0..5 {
                        band.coef[k] += band.step[k];
                    }
                }
            }
            return;
        }
        self.preamp = self.preamp_tgt;
        for band in self.bands.iter_mut() {
            band.coef = band.tgt;
            if band.on && band.coef == IDENTITY {
                band.on = false;
            }
        }
        if self.deactivating {
            self.deactivating = false;
            self.active = false;
        }
    }

    /// 原地处理交错样本（旁路时立即返回；超过 `MAX_CHANNELS` 的声道不动）。
    pub fn process(&mut self, data: &mut [f32], channels: usize) {
        if !self.active || channels == 0 {
            return;
        }
        let nch = channels.min(MAX_CHANNELS);
        let frames = data.len() / channels;
        for f in 0..frames {
            if self.ramp_left > 0 {
                self.advance();
            }
            let base = f * channels;
            let preamp = self.preamp;
            for ch in 0..nch {
                let mut x = data[base + ch] * preamp;
                for band in self.bands.iter_mut() {
                    if !band.on {
                        continue;
                    }
                    let c = &band.coef;
                    let y = c[0] * x + band.z1[ch];
                    band.z1[ch] = c[1] * x - c[3] * y + band.z2[ch];
                    band.z2[ch] = c[2] * x - c[4] * y;
                    // 静默期的滤波器尾端会衰减进非规格化数（x86 上非规格
                    // 数运算慢几十倍，占实时回调的时间）：过小直接冲零，
                    // 归零后整链对零输入恒出零。
                    x = if y.abs() < 1e-15 { 0.0 } else { y };
                }
                data[base + ch] = x;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(enabled: bool, preamp: f32, bands: [f32; 10]) -> EqConfig {
        EqConfig {
            enabled,
            preamp_db: preamp,
            bands,
        }
    }

    /// 交错正弦波（每声道同相）。
    fn sine(freq: f32, rate: u32, secs: f32, channels: usize) -> Vec<f32> {
        let frames = (rate as f32 * secs) as usize;
        let mut data = Vec::with_capacity(frames * channels);
        for f in 0..frames {
            let v = (f as f32 * freq * std::f32::consts::TAU / rate as f32).sin() * 0.25;
            for _ in 0..channels {
                data.push(v);
            }
        }
        data
    }

    /// 丢弃前 `skip_frames` 帧（滤波器瞬态）后的 RMS。
    fn rms(data: &[f32], channels: usize, skip_frames: usize) -> f32 {
        let tail = &data[skip_frames * channels..];
        (tail.iter().map(|x| x * x).sum::<f32>() / tail.len().max(1) as f32).sqrt()
    }

    /// 全零配置 = 直通：输出逐样本等于输入。
    #[test]
    fn eq_flat_is_identity() {
        let input = sine(1000.0, 48_000, 0.1, 2);
        let mut data = input.clone();
        let mut eq = Equalizer::new();
        eq.configure(&cfg(true, 0.0, [0.0; 10]), 48_000);
        eq.process(&mut data, 2);
        assert_eq!(data, input);
        // 未启用的配置也直通。
        let mut eq = Equalizer::new();
        eq.configure(&cfg(false, 6.0, [3.0; 10]), 48_000);
        eq.process(&mut data, 2);
        assert_eq!(data, input);
    }

    /// 仅 1 kHz 段 +6 dB：1 kHz 相对 100 Hz 的增益差 ≈ ×1.995。合成
    /// 响应的削波 headroom 会把绝对电平压回前级以内，所以按两频点的
    /// 相对比验证；顺带断言绝对电平不超输入（不削波）。
    #[test]
    fn eq_boost_at_center_frequency() {
        let mut bands = [0.0; 10];
        bands[5] = 6.0; // 1 kHz
        let rate = 48_000;
        let skip = (rate as f32 * 0.2) as usize;
        let run = |freq: f32| {
            let input = sine(freq, rate, 2.0, 2);
            let mut data = input.clone();
            let mut eq = Equalizer::new();
            eq.configure(&cfg(true, 0.0, bands), rate);
            eq.process(&mut data, 2);
            rms(&data, 2, skip) / rms(&input, 2, skip)
        };
        let at_1k = run(1000.0);
        let at_100 = run(100.0);
        let ratio = at_1k / at_100;
        assert!(
            (ratio - 1.995).abs() / 1.995 < 0.05,
            "1k/100Hz 相对增益 {ratio}（期望 ≈1.995）"
        );
        assert!(
            at_1k <= 1.05 && at_100 <= 1.05,
            "绝对电平应被 headroom 压回：1k {at_1k} / 100 {at_100}"
        );
    }

    /// 1 kHz 段 −6 dB：RMS ≈ ×0.501。
    #[test]
    fn eq_cut_reduces_level() {
        let mut bands = [0.0; 10];
        bands[5] = -6.0;
        let rate = 48_000;
        let input = sine(1000.0, rate, 2.0, 2);
        let mut data = input.clone();
        let mut eq = Equalizer::new();
        eq.configure(&cfg(true, 0.0, bands), rate);
        eq.process(&mut data, 2);
        let skip = (rate as f32 * 0.2) as usize;
        let ratio = rms(&data, 2, skip) / rms(&input, 2, skip);
        assert!(
            (ratio - 0.501).abs() / 0.501 < 0.03,
            "RMS 比 {ratio}（期望 ≈0.501）"
        );
    }

    /// 32 kHz 采样率：16 kHz 段（≥0.45·rate）直接跳过，处理白噪声
    /// 不产生 NaN / Inf / 削波级输出。
    #[test]
    fn eq_skips_bands_near_nyquist() {
        let mut bands = [0.0; 10];
        bands[9] = 12.0; // 16 kHz
        let rate = 32_000;
        // 简单确定性噪声（LCG）。
        let mut seed = 0x1234_5678u32;
        let mut data: Vec<f32> = (0..rate as usize * 2)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
            })
            .collect();
        let mut eq = Equalizer::new();
        eq.configure(&cfg(true, 0.0, bands), rate);
        eq.process(&mut data, 2);
        let peak = data.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak.is_finite() && peak < 4.0, "峰值 {peak}");
        assert!(data.iter().all(|x| x.is_finite()));
    }

    /// 各段为 0、preamp −6 dB：输出 ≈ 输入 × 0.501。
    #[test]
    fn eq_preamp_scales() {
        let input = sine(1000.0, 48_000, 0.1, 2);
        let mut data = input.clone();
        let mut eq = Equalizer::new();
        eq.configure(&cfg(true, -6.0, [0.0; 10]), 48_000);
        eq.process(&mut data, 2);
        let want = 10f32.powf(-6.0 / 20.0);
        // 跳过前 ~10ms（前级也在渐变区里，走完了才到目标值）。
        for (i, (got, src)) in data.iter().zip(input.iter()).enumerate().skip(960) {
            assert!(
                (got - src * want).abs() < 1e-6,
                "样本 {i}: {got} != {} * {want}",
                src
            );
        }
    }

    /// 声道独立：左声道正弦、右声道全 0 → 右声道输出仍全 0。
    #[test]
    fn eq_channels_are_independent() {
        let rate = 48_000;
        let frames = rate as usize / 10;
        let mut data = vec![0.0f32; frames * 2];
        for f in 0..frames {
            data[f * 2] = (f as f32 * 1000.0 * std::f32::consts::TAU / rate as f32).sin() * 0.25;
        }
        let mut bands = [0.0; 10];
        bands[0] = 6.0;
        bands[5] = 6.0;
        let mut eq = Equalizer::new();
        eq.configure(&cfg(true, 3.0, bands), rate);
        eq.process(&mut data, 2);
        assert!(
            data.iter().skip(1).step_by(2).all(|x| *x == 0.0),
            "右声道应仍为 0"
        );
        assert!(
            data.iter().step_by(2).skip(960).any(|x| x.abs() > 0.3),
            "左声道应有 +6dB 提升"
        );
    }

    /// 改参数不清零滤波器状态、系数渐变：1 kHz 正弦中途把 31 Hz 段
    /// 推满 +12 dB，相邻样本不出现爆音级跳变（系数突变会让输出瞬时
    /// 放大数倍）。
    #[test]
    fn eq_reconfigure_does_not_click() {
        let rate = 48_000;
        let input = sine(1000.0, rate, 0.2, 2);
        let mut data = input.clone();
        let mut eq = Equalizer::new();
        let mut bands = [0.0; 10];
        bands[5] = 6.0;
        eq.configure(&cfg(true, 0.0, bands), rate);
        let half = data.len() / 4 * 2;
        eq.process(&mut data[..half], 2);
        bands[0] = 12.0;
        eq.configure(&cfg(true, 0.0, bands), rate);
        eq.process(&mut data[half..], 2);
        // 0.25 幅 1 kHz 的正常相邻样本差 ≈0.033；留六倍余量。
        let max_step = data
            .windows(2)
            .fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        assert!(max_step < 0.2, "改配置后样本跳变 {max_step}");
    }

    /// 关闭 EQ：渐变到直通后旁路——输出回到与输入逐样本一致
    /// （恒等系数下残存状态两帧内归零，不留滤波痕迹）。
    #[test]
    fn eq_disable_returns_to_bypass() {
        let rate = 48_000;
        let input = sine(500.0, rate, 0.2, 2);
        let mut data = input.clone();
        let mut eq = Equalizer::new();
        let mut bands = [0.0; 10];
        bands[2] = 8.0;
        eq.configure(&cfg(true, 0.0, bands), rate);
        eq.process(&mut data[..960], 2); // 10ms：激活渐变走完
        eq.configure(&cfg(false, 0.0, bands), rate);
        eq.process(&mut data[960..], 2);
        assert_eq!(&data[4800..], &input[4800..], "关 EQ 后应逐样本直通");
    }

    /// 推满提升也不削波：+12 dB 段推近满幅的 31 Hz 正弦，峰值仍
    /// ≤ 输入峰值（headroom 前级把净提升压回 0 dB 内）。
    #[test]
    fn eq_headroom_prevents_clip() {
        let rate = 48_000;
        let input = sine(31.0, rate, 1.0, 2);
        let in_peak = input.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let mut data = input.clone();
        let mut eq = Equalizer::new();
        let mut bands = [0.0; 10];
        bands[0] = 12.0;
        eq.configure(&cfg(true, 0.0, bands), rate);
        eq.process(&mut data, 2);
        let out_peak = data[4800 * 2..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(
            out_peak <= in_peak * 1.05,
            "输出峰值 {out_peak} 应 ≤ 输入 {in_peak}"
        );
    }
}
