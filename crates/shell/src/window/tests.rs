#[test]
fn tray_glyph_svg_rasterizes() {
    use zeo_icons::play_pause_svg;
    let px = |pause: bool, tile: bool, scale: f32, u: f32, v: f32| -> [u8; 4] {
        let r = nsh_core::svg::rasterize(play_pause_svg(pause, tile, 0.0, scale).as_bytes(), 44)
            .unwrap();
        let (x, y) = ((u * 44.0).min(43.0) as usize, (v * 44.0).min(43.0) as usize);
        let i = (y * 44 + x) * 4;
        [
            r.pixels[i],
            r.pixels[i + 1],
            r.pixels[i + 2],
            r.pixels[i + 3],
        ]
    };
    // 暂停双杠：左杠、右杠在里面，中间缝不在。
    assert!(px(true, false, 1.0, 0.30, 0.5)[3] > 200);
    assert!(px(true, false, 1.0, 0.64, 0.5)[3] > 200);
    assert!(px(true, false, 1.0, 0.5, 0.5)[3] < 40);
    // 播放三角：内部实、右下外。
    assert!(px(false, false, 1.0, 0.45, 0.5)[3] > 200);
    assert!(px(false, false, 1.0, 0.6, 0.72)[3] < 40);
    // 0.8 缩（macOS）：满格时在左杠内的点被缩出去。
    assert!(px(true, false, 0.8, 0.24, 0.5)[3] < 40);
    // Windows 砖：角外透明，砖内字形外的点是浅绿渐变（绿通道最高）。
    assert!(px(false, true, 1.0, 0.02, 0.02)[3] < 40);
    let c = px(false, true, 1.0, 0.5, 0.9);
    assert!(c[3] > 200 && c[1] > c[0] && c[1] > 200);
}
