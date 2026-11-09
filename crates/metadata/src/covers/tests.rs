use super::*;

/// 调色板取色：纯色图出主色，灰图不给强调色。
#[test]
fn palette_hue_picks_the_saturated_color() {
    let red = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        64,
        64,
        image::Rgb([220, 30, 40]),
    ));
    let (h, s) = palette_hue(&red).expect("红图应有主色调");
    assert!((h - 0.0).abs() < 0.02 || h > 0.98, "红色相约 0: {h}");
    assert!(s > 0.5, "饱和度应高: {s}");
    let gray = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        64,
        64,
        image::Rgb([128, 128, 128]),
    ));
    assert_eq!(palette_hue(&gray), None, "灰图不给强调色");
}

/// sRGB → HSL 的基准色与灰度。
#[test]
fn hsl_of_primaries_and_grays() {
    let (h, s, l) = hsl_of(255, 0, 0);
    assert!((h - 0.0).abs() < 1e-6 && (s - 1.0).abs() < 1e-6 && (l - 0.5).abs() < 1e-6);
    let (h, _, l) = hsl_of(0, 255, 0);
    assert!((h - 1.0 / 3.0).abs() < 1e-6 && (l - 0.5).abs() < 1e-6);
    let (h, _, l) = hsl_of(0, 0, 255);
    assert!((h - 2.0 / 3.0).abs() < 1e-6 && (l - 0.5).abs() < 1e-6);
    let (_, s, l) = hsl_of(255, 255, 255);
    assert_eq!((s, l), (0.0, 1.0));
    let (_, s, l) = hsl_of(128, 128, 128);
    assert_eq!(s, 0.0);
    assert!((l - 128.0 / 255.0).abs() < 1e-6);
}

/// 语料 sine.flac（内嵌红色封面）：读标签 → has_art → 取封面 → 主色调。
#[test]
fn corpus_cover_pipeline() {
    let path = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../..",
        "/.nsh/tmp/zeo-player-test-music/sine.flac"
    ));
    if !path.is_file() {
        eprintln!("跳过：语料不存在（sh scripts/make-test-music.sh）");
        return;
    }
    let info = crate::read_meta(path).expect("read_meta");
    assert!(info.has_art, "sine.flac 应有内嵌封面");
    let (bytes, mime) = crate::read_artwork(path).expect("read_artwork");
    let img = image::load_from_memory(&bytes).expect("decode cover");
    let hue = palette_hue(&img);
    assert!(hue.is_some(), "红色封面应有主色调（mime={mime:?}）");
}

/// 语料 sine.flac 内嵌红色封面、同目录有蓝色 sine.png：本地图片优先
/// 取蓝（sidecar），内嵌优先取红（read_artwork）。
#[test]
fn cover_source_respects_local_first() {
    let path = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../..",
        "/.nsh/tmp/zeo-player-test-music/sine.flac"
    ));
    if !path.is_file() || find_sidecar(path).is_none() {
        eprintln!("跳过：语料不存在（sh scripts/make-test-music.sh）");
        return;
    }
    let hue_of = |src: Option<CoverSrc>| match src? {
        CoverSrc::Img(img) => palette_hue(&img).map(|(h, _)| h),
        CoverSrc::Raw(_) => None,
    };
    let local = hue_of(cover_source_for(path, true)).expect("sidecar 应有主色调");
    assert!(
        (0.55..0.75).contains(&local),
        "本地优先应取蓝 sidecar：{local:.3}"
    );
    let embedded = hue_of(cover_source_for(path, false)).expect("内嵌封面应有主色调");
    assert!(
        !(0.55..0.75).contains(&embedded),
        "内嵌优先应取红封面而非蓝 sidecar：{embedded:.3}"
    );
}

/// 真实曲库里的一首（没有则跳过）：生成缩略图并命中缓存。
#[test]
fn thumbnails_are_small_jpegs() {
    let Some(home) = dirs::home_dir() else { return };
    let Some(file) = walkdir::WalkDir::new(home.join("Music"))
        .max_depth(3)
        .into_iter()
        .flatten()
        .map(|e| e.into_path())
        .find(|p| p.extension().is_some_and(|e| e == "flac" || e == "mp3"))
    else {
        eprintln!("跳过：~/Music 里没有曲目");
        return;
    };
    // SAFETY(测试): 只有本测试读写这个变量。
    unsafe { std::env::set_var("ZEO_PLAYER_CACHE_DIR", "/tmp/zeo-player-test-cache") };
    let _ = std::fs::remove_dir_all("/tmp/zeo-player-test-cache");
    let started = std::time::Instant::now();
    let Some(thumb) = cover_file(&file, THUMB) else {
        eprintln!("跳过：{} 没有封面", file.display());
        return;
    };
    let first = started.elapsed();
    let img = image::open(&thumb).expect("缩略图可解码");
    assert!(img.width() <= THUMB && img.height() <= THUMB);
    let size = std::fs::metadata(&thumb).unwrap().len();
    let again = std::time::Instant::now();
    assert_eq!(cover_file(&file, THUMB), Some(thumb.clone()));
    eprintln!(
        "{} → {}×{} {size} B，首次 {first:?}，缓存 {:?}",
        file.display(),
        img.width(),
        img.height(),
        again.elapsed()
    );
}
