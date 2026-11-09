//! 播放设置的持久化形态（`library.json`）：循环模式、ReplayGain、10 段 EQ。
//! 运行逻辑（增益计算、双二阶 DSP）在 `zeo-engine`。

use serde::{Deserialize, Serialize};

/// 默认淡入淡出时长。
pub const DEFAULT_FADE_MS: u32 = 200;
/// 默认播放统计阈值：听过整首的百分比。
pub const DEFAULT_STATS_MIN_PERCENT: u32 = 60;
/// 默认播放统计阈值：实际播出秒数。
pub const DEFAULT_STATS_MIN_SECS: u32 = 30;

/// 队列的循环模式（Zplayer 的 LOOP_MODE：list / single / random）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoopMode {
    /// 顺序播放整个队列，到末尾回到第一首。
    #[default]
    List,
    /// 单曲循环：自然播完或按「下一首」都重播同一首。
    Single,
    /// 随机：队列洗牌后的顺序循环（界面显示的队列即洗牌后顺序）。
    Random,
}

impl LoopMode {
    /// 下一个模式（点循环按钮的顺序：顺序 → 单曲 → 随机）。
    pub fn cycle(self) -> LoopMode {
        match self {
            LoopMode::List => LoopMode::Single,
            LoopMode::Single => LoopMode::Random,
            LoopMode::Random => LoopMode::List,
        }
    }

    pub fn from_bits(value: u8) -> LoopMode {
        match value {
            1 => LoopMode::Single,
            2 => LoopMode::Random,
            _ => LoopMode::List,
        }
    }
}

/// ReplayGain 模式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplayGainMode {
    /// 不做音量标准化。
    #[default]
    Off,
    /// 按曲目增益。
    Track,
    /// 按专辑增益。
    Album,
}

/// ReplayGain 设置（界面 → 引擎，持久化在 `library.json`）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplayGainConfig {
    pub mode: ReplayGainMode,
    /// 预放大（dB，−15…+15）。
    pub preamp_db: f32,
    /// 没有 RG 标签的曲目用的增益（dB，−15…+15）。
    pub fallback_db: f32,
    /// 按标签峰值限制增益，防止削波。
    pub clip_protect: bool,
}

impl Default for ReplayGainConfig {
    fn default() -> ReplayGainConfig {
        ReplayGainConfig {
            mode: ReplayGainMode::Off,
            preamp_db: 0.0,
            fallback_db: 0.0,
            clip_protect: true,
        }
    }
}

/// 频点（Hz）。
pub const BANDS: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
/// 每段与前级的增益范围（dB）。
pub const MAX_DB: f32 = 12.0;
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
