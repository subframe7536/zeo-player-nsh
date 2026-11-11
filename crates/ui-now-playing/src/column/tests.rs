use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn meta() -> MetaInfo {
        MetaInfo {
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            album_artist: String::new(),
            duration: 0.0,
            has_art: false,
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
            art_hue: None,
            replaygain: Default::default(),
            ..Default::default()
        }
    }

    #[test]
    pub(crate) fn audio_line_matches_the_design() {
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
    pub(crate) fn quality_tags() {
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
