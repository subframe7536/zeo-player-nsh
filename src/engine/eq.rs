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

/// 一段的系数与逐声道状态（Transposed Direct Form II）。
#[derive(Clone, Copy)]
struct Band {
    on: bool,
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: [f32; MAX_CHANNELS],
    z2: [f32; MAX_CHANNELS],
}

impl Band {
    fn off() -> Band {
        Band {
            on: false,
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: [0.0; MAX_CHANNELS],
            z2: [0.0; MAX_CHANNELS],
        }
    }

    /// RBJ cookbook peaking EQ；返回已除 a0 的系数。
    fn peaking(f0: f32, rate: f32, db: f32) -> Band {
        let a = 10f32.powf(db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * f0 / rate;
        let alpha = w0.sin() / (2.0 * Q);
        let cos = w0.cos();
        let a0 = 1.0 + alpha / a;
        Band {
            on: true,
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * cos / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha / a) / a0,
            z1: [0.0; MAX_CHANNELS],
            z2: [0.0; MAX_CHANNELS],
        }
    }
}

/// 回调侧的 EQ 实例：随输出流创建，按该流的采样率配置。
pub struct Equalizer {
    /// 前级线性增益。
    preamp: f32,
    bands: [Band; 10],
    /// `false` = 完全旁路（配置为平直或关闭）。
    active: bool,
}

impl Equalizer {
    /// 未配置的旁路状态（首块回调读配置前用）。
    pub fn new() -> Equalizer {
        Equalizer {
            preamp: 1.0,
            bands: [Band::off(); 10],
            active: false,
        }
    }

    /// 按 `rate` 重算系数并清零状态；增益为 0 或频点 ≥ 0.45·rate 的段跳过。
    pub fn configure(&mut self, cfg: &EqConfig, rate: u32) {
        self.bands = [Band::off(); 10];
        self.preamp = 1.0;
        self.active = !cfg.is_flat();
        if !self.active {
            return;
        }
        self.preamp = 10f32.powf(cfg.preamp_db.clamp(-MAX_DB, MAX_DB) / 20.0);
        let rate = rate as f32;
        for (band, (&f0, &db)) in self
            .bands
            .iter_mut()
            .zip(BANDS.iter().zip(cfg.bands.iter()))
        {
            if db != 0.0 && f0 < 0.45 * rate {
                *band = Band::peaking(f0, rate, db.clamp(-MAX_DB, MAX_DB));
            }
        }
    }

    /// 原地处理交错样本（旁路时立即返回；超过 `MAX_CHANNELS` 的声道不动）。
    pub fn process(&mut self, data: &mut [f32], channels: usize) {
        if !self.active || channels == 0 {
            return;
        }
        let nch = channels.min(MAX_CHANNELS);
        let frames = data.len() / channels;
        let preamp = self.preamp;
        for f in 0..frames {
            let base = f * channels;
            for ch in 0..nch {
                let mut x = data[base + ch] * preamp;
                for band in self.bands.iter_mut() {
                    if band.on {
                        let y = band.b0 * x + band.z1[ch];
                        band.z1[ch] = band.b1 * x - band.a1 * y + band.z2[ch];
                        band.z2[ch] = band.b2 * x - band.a2 * y;
                        x = y;
                    }
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

    /// 仅 1 kHz 段 +6 dB：1 kHz 正弦 RMS ≈ ×1.995，100 Hz 几乎不变。
    #[test]
    fn eq_boost_at_center_frequency() {
        let mut bands = [0.0; 10];
        bands[5] = 6.0; // 1 kHz
        let rate = 48_000;

        let input = sine(1000.0, rate, 2.0, 2);
        let mut data = input.clone();
        let mut eq = Equalizer::new();
        eq.configure(&cfg(true, 0.0, bands), rate);
        eq.process(&mut data, 2);
        let skip = (rate as f32 * 0.2) as usize;
        let ratio = rms(&data, 2, skip) / rms(&input, 2, skip);
        assert!(
            (ratio - 1.995).abs() / 1.995 < 0.03,
            "1kHz RMS 比 {ratio}（期望 ≈1.995）"
        );

        let input = sine(100.0, rate, 2.0, 2);
        let mut data = input.clone();
        let mut eq = Equalizer::new();
        eq.configure(&cfg(true, 0.0, bands), rate);
        eq.process(&mut data, 2);
        let ratio = rms(&data, 2, skip) / rms(&input, 2, skip);
        assert!(
            (0.95..=1.08).contains(&ratio),
            "100Hz RMS 比 {ratio}（期望 0.95–1.08）"
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
        for (i, (got, src)) in data.iter().zip(input.iter()).enumerate().skip(96) {
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
            data.iter().step_by(2).skip(96).any(|x| x.abs() > 0.3),
            "左声道应有 +6dB 提升"
        );
    }
}
