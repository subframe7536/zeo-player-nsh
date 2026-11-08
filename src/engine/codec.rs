//! symphonia 解码封装：打开文件、逐块解码为交错 f32、精确 seek。
//! 支持 MP3 / FLAC / WAV / AIFF / AAC / ALAC（M4A）/ OGG Vorbis。

use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::TrackType;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

/// 一个打开的解码源。
pub struct Source {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    pub sample_rate: u32,
    pub channels: usize,
    scratch: Vec<f32>,
}

impl Source {
    /// 打开并探测一个音频文件。
    pub fn open(path: &Path) -> Result<Source, String> {
        let file = File::open(path).map_err(|e| format!("打开失败: {e}"))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension() {
            hint.with_extension(&ext.to_string_lossy());
        }
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| format!("不支持的格式: {e}"))?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| "没有音频轨道".to_owned())?
            .clone();
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio().cloned())
            .ok_or_else(|| "缺少音频参数".to_owned())?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|e| format!("解码器创建失败: {e}"))?;
        let sample_rate = params.sample_rate.unwrap_or(44_100).max(1);
        let channels = params.channels.map(|c| c.count()).unwrap_or(2).max(1);
        Ok(Source {
            format,
            decoder,
            track_id: track.id,
            sample_rate,
            channels,
            scratch: Vec::new(),
        })
    }

    /// 解码下一块：返回交错 f32 样本（帧数 = len / channels）。`None` 表示结束。
    pub fn next_block(&mut self) -> Result<Option<&[f32]>, String> {
        loop {
            match self.format.next_packet() {
                Ok(Some(packet)) => {
                    if packet.track_id != self.track_id {
                        continue;
                    }
                    match self.decoder.decode(&packet) {
                        Ok(buf) => {
                            let count = buf.samples_interleaved();
                            self.scratch.clear();
                            self.scratch.resize(count, 0.0);
                            buf.copy_to_slice_interleaved(&mut self.scratch);
                            return Ok(Some(&self.scratch));
                        }
                        Err(Error::DecodeError(_)) | Err(Error::IoError(_)) => continue,
                        Err(e) => return Err(format!("解码错误: {e}")),
                    }
                }
                Ok(None) => return Ok(None),
                Err(Error::ResetRequired) => {
                    // 轨道列表变化（链式流）：重新取默认轨道并重建解码器。
                    let track = self
                        .format
                        .default_track(TrackType::Audio)
                        .ok_or_else(|| "轨道重置后没有音频轨道".to_owned())?
                        .clone();
                    let params = track
                        .codec_params
                        .as_ref()
                        .and_then(|p| p.audio().cloned())
                        .ok_or_else(|| "轨道重置后缺少音频参数".to_owned())?;
                    self.decoder = symphonia::default::get_codecs()
                        .make_audio_decoder(&params, &AudioDecoderOptions::default())
                        .map_err(|e| format!("解码器重建失败: {e}"))?;
                    self.track_id = track.id;
                }
                Err(Error::IoError(_)) => return Ok(None), // 截断文件按结束处理
                Err(e) => return Err(format!("读取错误: {e}")),
            }
        }
    }

    /// 精确 seek 到 `seconds`；返回实际落点（秒，可能略早于请求值）。
    pub fn seek(&mut self, seconds: f64) -> Result<f64, String> {
        let clamped = seconds.clamp(0.0, 24.0 * 3600.0);
        let nanos = (clamped.fract() * 1e9) as u32;
        let time = Time::try_new(clamped.floor() as i64, nanos.min(999_999_999))
            .ok_or_else(|| "seek 时间非法".to_owned())?;
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| format!("seek 失败: {e}"))?;
        self.decoder.reset();
        // actual_ts 是轨道时间基单位；用 fmt 的时间基换回秒。
        if let Some(track) = self.format.tracks().iter().find(|t| t.id == self.track_id)
            && let Some(tb) = track.time_base
        {
            return Ok(tb.calc_time_saturating(seeked.actual_ts).as_secs_f64());
        }
        Ok(clamped)
    }
}

#[cfg(test)]
mod tests {
    use super::Source;
    use std::path::Path;

    /// 每种新格式一个 2 秒样本（scripts/make-test-music.sh 的
    /// zeo-player-test-formats）：能打开、总帧数够、seek(1.0) 落点准确、
    /// 标题与 quality 对、波形生成成功。
    #[test]
    fn decodes_every_supported_format() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".nsh/tmp/zeo-player-test-formats");
        if !dir.is_dir() {
            eprintln!("跳过：没有 {} 测试语料", dir.display());
            return;
        }
        // （文件名, 期望标题, 期望 quality）
        let cases = [
            ("aac.m4a", "格式 aac", "Lossy"),
            ("alac.m4a", "格式 alac", "Lossless"),
            ("raw.aac", "", "Lossy"),
            ("vorbis.ogg", "格式 vorbis", "Lossy"),
            ("pcm.aiff", "格式 pcm", "Lossless"),
            ("rg.flac", "格式 rg", "Lossless"),
        ];
        for (name, title, quality) in cases {
            let path = dir.join(name);
            let mut src =
                Source::open(&path).unwrap_or_else(|e| panic!("{name}: Source::open: {e}"));
            let channels = src.channels;
            let mut frames = 0usize;
            while let Some(block) = src
                .next_block()
                .unwrap_or_else(|e| panic!("{name}: next_block: {e}"))
            {
                frames += block.len() / channels;
            }
            assert!(
                frames as f64 >= src.sample_rate as f64 * 1.5,
                "{name}: 解码帧数 {frames} 不足（sample_rate {}）",
                src.sample_rate
            );
            match src.seek(1.0) {
                Ok(at) => assert!((0.9..=1.05).contains(&at), "{name}: seek(1.0) 落点 {at}"),
                // ADTS 裸流没有索引，symphonia 对它不做精确 seek——只有它允许跳过断言。
                Err(_) if name == "raw.aac" => {}
                Err(e) => panic!("{name}: seek(1.0): {e}"),
            }
            let meta = crate::metadata::read_meta(&path)
                .unwrap_or_else(|e| panic!("{name}: read_meta: {e}"));
            assert_eq!(meta.title, title, "{name}: 标题不符");
            assert_eq!(
                meta.quality.as_deref(),
                Some(quality),
                "{name}: quality 不符"
            );
            crate::metadata::waveform::waveform(&path)
                .unwrap_or_else(|e| panic!("{name}: waveform: {e}"));
        }
    }
}
