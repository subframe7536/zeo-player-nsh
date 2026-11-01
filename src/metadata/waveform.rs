//! 56 根波形条，按 ap-spec 的 `createWaveformGenerator` 计算：
//!
//! 1. 解码全文件，取第一声道，预计算约 [`PRECOMPUTE_BLOCKS`] 个块的 RMS；
//! 2. 以 RMS 的第 [`WaveformOptions::amplitude_percentile`] 分位为参考峰值
//!    （默认 95%：最响的 5% 尖峰被削到上限，波形主体更饱满）；
//! 3. 重采样到 [`BARS`] 根：每根取所覆盖块的 RMS 均值，除以参考峰值并截到
//!    0..1，再映射到 `min..max`（默认 0.1..0.9）。

use std::path::Path;
use std::sync::{Arc, OnceLock};

use crate::engine::codec::Source;

pub const BARS: usize = 56;
/// 预计算的 RMS 块数（目标值；实际为 `ceil(总样本 / 块大小)`）。
pub const PRECOMPUTE_BLOCKS: usize = 2000;
/// 解码时累加平方和的粒度（样本）；预计算块按它对齐，边界误差不超过一粒。
const GRAIN: usize = 64;

/// 归一化参数（全部用默认值）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveformOptions {
    /// 最小值。
    pub min: f64,
    /// 最大值。
    pub max: f64,
    /// 参考峰值取 RMS 的哪个分位（0..1）。越低波形越满、余量越少。
    pub amplitude_percentile: f64,
}

impl Default for WaveformOptions {
    fn default() -> Self {
        WaveformOptions {
            min: 0.1,
            max: 0.9,
            amplitude_percentile: 0.95,
        }
    }
}

type WaveCache = std::sync::Mutex<std::collections::HashMap<String, (u64, Arc<Vec<f64>>)>>;

fn cache() -> &'static WaveCache {
    static CACHE: OnceLock<WaveCache> = OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 一个文件的 56 根波形条（按文件指纹缓存）。
pub fn waveform(path: &Path) -> Result<Arc<Vec<f64>>, String> {
    let key = path.to_string_lossy().into_owned();
    let stamp = std::fs::metadata(path)
        .map(|m| crate::metadata::file_stamp(&m))
        .unwrap_or(0);
    if let Some((cached, bars)) = cache().lock().unwrap().get(&key)
        && *cached == stamp
    {
        return Ok(bars.clone());
    }
    let bars = Arc::new(compute(path, WaveformOptions::default())?);
    cache().lock().unwrap().insert(key, (stamp, bars.clone()));
    Ok(bars)
}

/// 解码，累加第一声道每 [`GRAIN`] 个样本的平方和，然后算 RMS 块与波形。
fn compute(path: &Path, options: WaveformOptions) -> Result<Vec<f64>, String> {
    let mut source = Source::open(path)?;
    let channels = source.channels.max(1);
    let mut grains: Vec<(f64, u32)> = Vec::new();
    let mut sum = 0.0_f64;
    let mut count = 0_u32;
    while let Some(block) = source.next_block()? {
        // 交错样本：只取第一声道（与 `getChannelData(0)` 一致）。
        for sample in block.iter().step_by(channels) {
            let s = *sample as f64;
            sum += s * s;
            count += 1;
            if count as usize == GRAIN {
                grains.push((sum, count));
                sum = 0.0;
                count = 0;
            }
        }
    }
    if count > 0 {
        grains.push((sum, count));
    }
    let rms = precompute_rms(&grains);
    Ok(resample(&rms, BARS, options))
}

/// 第 1 步：把平方和颗粒合成约 [`PRECOMPUTE_BLOCKS`] 个块，每块的 RMS。
fn precompute_rms(grains: &[(f64, u32)]) -> Vec<f32> {
    let total: usize = grains.iter().map(|g| g.1 as usize).sum();
    if total == 0 {
        return Vec::new();
    }
    let block_size = total.div_ceil(PRECOMPUTE_BLOCKS).max(1);
    let grains_per_block = block_size.div_ceil(GRAIN).max(1);
    grains
        .chunks(grains_per_block)
        .map(|chunk| {
            let (sum, n) = chunk
                .iter()
                .fold((0.0, 0_u64), |(s, n), g| (s + g.0, n + g.1 as u64));
            (sum / n.max(1) as f64).sqrt() as f32
        })
        .collect()
}

/// 第 2、3 步：按分位求参考峰值，再重采样到 `bars` 根并映射到 `min..max`。
pub fn resample(rms: &[f32], bars: usize, options: WaveformOptions) -> Vec<f64> {
    let WaveformOptions {
        min,
        max,
        amplitude_percentile,
    } = options;
    let bars = bars.max(1);
    let blocks = rms.len();
    if blocks == 0 {
        return vec![min; bars];
    }
    let mut sorted = rms.to_vec();
    sorted.sort_by(f32::total_cmp);
    let index = ((blocks as f64 * amplitude_percentile).floor() as usize).min(blocks - 1);
    // 全静音时参考值为 0：退回 1，避免除以零。
    let reference = match sorted[index] as f64 {
        r if r > 0.0 => r,
        _ => 1.0,
    };
    let scale = max - min;
    let ratio = blocks as f64 / bars as f64;
    (0..bars)
        .map(|i| {
            let start = (i as f64 * ratio).floor() as usize;
            let end = (((i + 1) as f64 * ratio).ceil() as usize).min(blocks);
            let avg = if end > start {
                rms[start..end].iter().map(|r| *r as f64).sum::<f64>() / (end - start) as f64
            } else {
                0.0
            };
            min + (avg / reference).clamp(0.0, 1.0) * scale
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn bars_is_56() {
        assert_eq!(BARS, 56);
    }

    #[test]
    fn constant_signal_reaches_max() {
        let bars = resample(&vec![0.5; 2000], BARS, WaveformOptions::default());
        assert_eq!(bars.len(), BARS);
        assert!(bars.iter().all(|b| close(*b, 0.9)), "{bars:?}");
    }

    #[test]
    fn silence_sits_at_min() {
        let bars = resample(&vec![0.0; 2000], BARS, WaveformOptions::default());
        assert!(bars.iter().all(|b| close(*b, 0.1)));
        assert_eq!(
            resample(&[], BARS, WaveformOptions::default()),
            vec![0.1; BARS]
        );
    }

    #[test]
    fn top_five_percent_spikes_are_clamped() {
        // 96% 的块为 0.2，最后 4% 是 1.0 的尖峰：参考值取 95 分位 = 0.2，
        // 主体顶满（0.9），尖峰被截到 0.9，而不是把主体压到 0.26。
        let mut rms = vec![0.2_f32; 1920];
        rms.extend(vec![1.0_f32; 80]);
        let bars = resample(&rms, BARS, WaveformOptions::default());
        assert!(bars.iter().all(|b| close(*b, 0.9)), "{bars:?}");
        // 一半音量的段落落在中间。
        let mut half = vec![0.1_f32; 1000];
        half.extend(vec![0.2_f32; 1000]);
        let bars = resample(&half, 2, WaveformOptions::default());
        assert!(close(bars[0], 0.5) && close(bars[1], 0.9), "{bars:?}");
    }

    #[test]
    fn precompute_targets_two_thousand_blocks() {
        // 10 秒 44.1 kHz：441000 样本 → 块大小 221 → 每块 4 粒（256 样本）。
        let grains = vec![(GRAIN as f64 * 0.25, GRAIN as u32); 441_000 / GRAIN];
        let rms = precompute_rms(&grains);
        assert!((1700..=2000).contains(&rms.len()), "{} 块", rms.len());
        assert!(
            rms.iter().all(|r| (*r - 0.5).abs() < 1e-6),
            "平方均值 0.25 → RMS 0.5"
        );
    }
}

#[cfg(test)]
mod real_file {
    /// `cargo test --release -- --ignored real_track_waveform --nocapture`
    #[test]
    #[ignore = "decodes a track from ~/Music"]
    fn real_track_waveform() {
        let music = dirs::audio_dir().expect("~/Music");
        let file = walkdir::WalkDir::new(music)
            .into_iter()
            .flatten()
            .map(|e| e.into_path())
            .find(|p| p.extension().is_some_and(|e| e == "flac"))
            .expect("a flac in ~/Music");
        let started = std::time::Instant::now();
        let bars = super::waveform(&file).unwrap();
        let line: String = bars
            .iter()
            .map(|b| " ▁▂▃▄▅▆▇█".chars().nth((b * 8.0).round() as usize).unwrap())
            .collect();
        eprintln!(
            "{}\n{line}\nmin {:.2} max {:.2} · {:?}",
            file.display(),
            bars.iter().cloned().fold(1.0, f64::min),
            bars.iter().cloned().fold(0.0, f64::max),
            started.elapsed()
        );
        assert!(bars.iter().all(|b| (0.1..=0.9 + 1e-9).contains(b)));
    }
}
