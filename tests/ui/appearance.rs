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
    // 写全局主题色：持锁跑全程（欢迎页用例会断言出厂选中项）。
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[外观]").unwrap();
    app.wait_for("id:settings-content >> text:主题色").unwrap();
    let radio = |label: &str| format!("id:settings-content >> role:radio[{label}]");
    // 配置在线程里落盘：等它写完再断言。
    let file =
        std::path::Path::new(&std::env::var("ZEO_PLAYER_DATA_DIR").unwrap()).join("library.json");
    let saved = |color: &str| {
        for _ in 0..40 {
            if std::fs::read_to_string(&file)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .is_some_and(|v| v["theme_color"] == color)
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    };
    // 自定义卡：首次的自定义色是蓝（0x2563eb，落盘 "blue"）。
    app.press(&radio("自定义")).unwrap();
    assert!(app.screen().selected(&radio("自定义")).unwrap());
    assert!(saved("blue"), "theme_color 应为 blue");
    // 选中「自定义」后节头出现色块触发器：点开是取色弹层（HSB 区域 +
    // 色相条 + 颜色代码）。
    app.wait_for("id:settings-content >> label:自定义颜色")
        .unwrap();
    app.press("id:settings-content >> label:自定义颜色")
        .unwrap();
    app.wait_for("label:色相").unwrap();
    // 点弹层外（下一张单选卡）收掉弹层，也顺手切到「黑白」。
    app.press(&radio("黑白")).unwrap();
    app.wait_for_gone("label:色相", Duration::from_secs(2))
        .unwrap();
    assert!(saved("neutral"), "theme_color 应为 neutral");
    app.press(&radio("封面")).unwrap();
    assert!(saved("cover"), "theme_color 应为 cover");
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
