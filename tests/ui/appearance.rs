use super::*;

/// 设置弹窗 →「外观」分类 →「外观」下拉：选深色 / 浅色立即换色板
/// （文字颜色即变），回到「系统」。
#[test]
fn appearance_select_switches_the_color_scheme() {
    let mut app = start(1200.0, 800.0);
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[外观]").unwrap();
    app.wait_for("id:settings-content >> label:外观").unwrap();
    // 说明文字是 muted_fg，浅色 / 深色下不同；「外观」本身在多处出现，用整句取唯一。
    let fg = |app: &Headless| {
        app.screen()
            .span_style("text:或为界面固定一种")
            .unwrap()
            .color
    };
    let light = fg(&app);
    app.press("id:settings-content >> label:外观").unwrap();
    app.press("label:深色").unwrap();
    assert_ne!(fg(&app), light, "切到深色后界面应换深色色板");
    app.press("id:settings-content >> label:外观").unwrap();
    app.press("label:浅色").unwrap();
    assert_eq!(fg(&app), light, "切到浅色后界面回到浅色色板");
    app.press("id:settings-content >> label:外观").unwrap();
    app.press("label:系统").unwrap();
}

/// 设置弹窗 →「外观」分类 →「主题色」：支持配置黑白、封面、自定义。
#[test]
fn accent_swatches_switch_and_persist_the_theme_color() {
    let mut app = start(1200.0, 800.0);
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[外观]").unwrap();
    app.wait_for("id:settings-content >> text:主题色").unwrap();
    let radio = |label: &str| format!("id:settings-content >> role:radio[{label}]");
    let swatch = |label: &str| format!("id:settings-content >> role:button[主题色：{label}]");
    app.wait_until(
        "「封面」选中（出厂默认）",
        Duration::from_secs(2),
        |s| s.selected(&radio("封面")).unwrap_or(false),
    )
    .unwrap();
    app.press(&radio("自定义")).unwrap();
    app.wait_for(&swatch("红")).unwrap();
    app.press(&swatch("红")).unwrap();
    app.wait_until("「红」选中", Duration::from_secs(2), |s| {
        s.selected(&swatch("红")).unwrap_or(false)
    })
    .unwrap();
    assert!(!app.screen().selected(&radio("封面")).unwrap());
    // 配置在线程里落盘：等它写完再断言。
    let file =
        std::path::Path::new(&std::env::var("ZEO_PLAYER_DATA_DIR").unwrap()).join("library.json");
    let saved = |color: &str| {
        std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some_and(|v| v["theme_color"] == color)
    };
    for _ in 0..40 {
        if saved("red") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(saved("red"));
    app.press(&radio("黑白")).unwrap();
    for _ in 0..40 {
        if saved("neutral") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(saved("neutral"));
    app.press(&radio("封面")).unwrap();
    for _ in 0..40 {
        if saved("cover") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(saved("cover"));
}

/// 「跟随封面」（出厂默认）：播放带封面的曲目后强调色换成封面主色调——
/// 曲目标题从黑白染上彩色（饱和度 > 0）。
#[test]
fn cover_mode_tints_the_playing_track_title() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    // 先等封面线程把主色调算出来（写进全局元数据缓存）：之后播放时
    // request_accent 走同步快路径，不依赖可能被别的用例抢先取走的
    // drain 通知。
    let sine = dunce::canonicalize(CORPUS)
        .unwrap()
        .join("sine.flac")
        .to_string_lossy()
        .into_owned();
    app.wait_until("封面色调已算好", Duration::from_secs(10), |_| {
        zeo_player::engine::Meta::art_hue(&sine).is_some()
    })
    .unwrap();
    // 文件夹视图 → 点进根文件夹，曲目行才出现。双击才播放。
    goto_folders(&mut app);
    app.press("zeo-player-test-music").unwrap();
    app.wait_for("text:测试歌曲").unwrap();
    double_press(&mut app, "role:button[测试歌曲]");
    app.wait_until("曲目标题染成封面色", Duration::from_secs(5), |s| {
        // 播放条也有同名标题（中性色）：只看曲目行里的那一个。
        s.span_style("role:button[测试歌曲] >> text:测试歌曲")
            .map(|st| {
                let c = st.color.0;
                let (r, g, b) = (c >> 24 & 0xff, c >> 16 & 0xff, c >> 8 & 0xff);
                // 黑白文字三通道相等；封面强调色有明显色差。
                r.max(g).max(b) - r.min(g).min(b) > 40
            })
            .unwrap_or(false)
    })
    .unwrap();
    // 共享播放器：别让它放着歌去影响别的用例。
    zeo_player::engine::Player::global().pause();
}
