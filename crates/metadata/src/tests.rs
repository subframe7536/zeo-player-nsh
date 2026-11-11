use std::path::Path;

use crate::*;

/// ReplayGain 标签的常见写法：带 / 不带 dB 后缀、前导加号、空白。
#[test]
fn replaygain_parses_common_forms() {
    assert_eq!(parse_gain("-6.54 dB"), Some(-6.54));
    assert_eq!(parse_gain("+1.2dB"), Some(1.2));
    assert_eq!(parse_gain(" -3 "), Some(-3.0));
    assert_eq!(parse_gain("abc"), None);
    assert_eq!(parse_gain("nan"), None);
    assert_eq!(parse_gain("-100 dB"), None, "超界");
    assert_eq!(parse_peak("0.988525"), Some(0.988525));
    assert_eq!(parse_peak("0"), None, "峰值必须为正");
    assert_eq!(parse_peak("abc"), None);
}

/// 带 RG 标签的语料样本（scripts/make-test-music.sh 生成）：
/// read_meta 与 read_replaygain 都读出相同的值。
#[test]
fn replaygain_reads_flac_tags() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".nsh/tmp/zeo-player-test-formats/rg.flac");
    if !path.is_file() {
        eprintln!("跳过：没有 zeo-player-test-formats 测试语料");
        return;
    }
    for rg in [
        read_meta(&path).expect("read_meta").replaygain,
        read_replaygain(&path),
    ] {
        assert_eq!(rg.track_gain, Some(-6.0));
        assert_eq!(rg.track_peak, Some(0.5));
        assert_eq!(rg.album_gain, Some(-3.0));
        assert_eq!(rg.album_peak, None);
    }
}
