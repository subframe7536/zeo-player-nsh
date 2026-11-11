use super::*;

#[test]
fn sidebar_navigates_between_library_views() {
    let mut app = start(1200.0, 800.0);
    for label in ["首页", "音乐库", "艺术家", "专辑"] {
        assert!(has(&app, label), "{label}: {:?}", texts(&app));
    }
    app.press("音乐库").unwrap();
    app.wait_for("text:音乐库").unwrap();
    app.press("艺术家").unwrap();
    app.wait_for("text:艺术家").unwrap();
    app.press("专辑").unwrap();
    // 播放条在（共享播放器可能已被别的用例装上曲目，不断言「未在播放」）。
    app.wait_for("label:播放队列").unwrap();
    // 设置弹窗：左栏分类 + 右栏选项，默认「外观」。
    app.press("设置").unwrap();
    app.wait_for("id:settings-content >> label:外观").unwrap();
    for label in [
        "曲库",
        "播放",
        "歌词",
        "封面",
        "窗口",
        "其他",
        "通用",
        "元数据",
    ] {
        assert!(has(&app, label), "{label}: {:?}", texts(&app));
    }
    for (pane, section) in [
        ("曲库", "音乐文件夹"),
        ("其他", "分号"),
        ("播放", "淡入淡出"),
        ("窗口", "关闭窗口时"),
        ("歌词", "歌词对齐"),
        ("封面", "本地图片优先"),
    ] {
        app.press(&format!("id:settings-nav >> role:button[{pane}]"))
            .unwrap();
        app.wait_for(section).unwrap();
    }
    app.press("id:settings-nav >> role:button[其他]").unwrap();
    app.wait_for("艺术家分隔符").unwrap();
    app.wait_for("标签分隔符").unwrap();
    assert!(
        !texts(&app).iter().any(|t| t.starts_with("预览 ·")),
        "其他页不应包含预览部分"
    );
    app.press("id:settings-nav >> role:button[曲库]").unwrap();
    app.wait_for("音乐文件夹").unwrap();
    for label in ["添加文件夹…", "重建曲库", "监视文件夹变化"] {
        assert!(has(&app, label), "{label}: {:?}", texts(&app));
    }
    app.press("label:关闭设置").unwrap();
    app.wait_for_gone("音乐文件夹", Duration::from_secs(2))
        .unwrap();
}

/// 设置弹窗的搜索框：命中节的标题 / 关键词时左栏给结果行，右栏只显示
/// 命中的节；分类名命中时整节都算命中。
#[test]
fn settings_search_filters_sections() {
    let mut app = start(1200.0, 800.0);
    app.press("设置").unwrap();
    app.wait_for("id:settings-content >> label:外观").unwrap();
    app.type_text("label:搜索设置", "字体").unwrap();
    // 只有「字体」一节命中：左栏一行结果，右栏是它的内容（「外观」节被滤掉）。
    app.wait_for("id:settings-nav >> role:button[字体]")
        .unwrap();
    // 字体 combobox 的输入框显示当前值（不是文本节点，看 input 的文本）。
    assert_eq!(
        app.screen()
            .input_text("id:settings-content >> label:字体")
            .unwrap(),
        Some("系统字体")
    );
    assert!(!has(&app, "或为界面固定一种"));
    // 空曲库时主窗口空状态里也有「添加文件夹…」（没语料的平台），只看
    // 设置内容区里它被滤掉。
    app.wait_for_gone(
        "id:settings-content >> text:添加文件夹…",
        Duration::from_secs(2),
    )
    .unwrap();
    // 没有命中：右栏空状态。
    app.type_text("label:搜索设置", "zzz").unwrap();
    app.wait_for("没有匹配的设置").unwrap();
    // 清空：分类列表与各节都回来。
    app.press("id:settings-nav >> label:清除搜索").unwrap();
    app.wait_for("id:settings-content >> label:外观").unwrap();
    assert!(has(&app, "播放"));
}

/// 数据目录里 library.json 的 fade_ms（保存在后台线程，调用方轮询）。
fn saved_fade_ms() -> Option<u64> {
    let file =
        std::path::Path::new(&std::env::var("ZEO_PLAYER_DATA_DIR").unwrap()).join("library.json");
    std::fs::read_to_string(&file)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["fade_ms"].as_u64())
}

/// 设置弹窗 →「播放」分类 →「淡入淡出」数字输入：正常值、超上限钳到
/// 5000、非法输入不改值（保存在后台线程，轮询 library.json 等落盘）。
#[test]
fn fade_duration_input_saves_clamped_value() {
    let mut app = start(1200.0, 800.0);
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[播放]").unwrap();
    app.wait_for("label:淡入淡出时长").unwrap();
    let field = "label:淡入淡出时长";
    let saved = |ms: u64| {
        for _ in 0..40 {
            if saved_fade_ms() == Some(ms) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    };
    app.clear(field).unwrap();
    app.type_text(field, "750\n").unwrap();
    assert!(saved(750), "fade_ms 应为 750: {:?}", saved_fade_ms());
    app.clear(field).unwrap();
    app.type_text(field, "99999\n").unwrap();
    assert!(saved(5000), "fade_ms 应钳到 5000: {:?}", saved_fade_ms());
    app.clear(field).unwrap();
    app.type_text(field, "abc\n").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(saved_fade_ms(), Some(5000), "非法输入不应改值");
    // 共享全局配置，用例结束前恢复默认 200。
    app.clear(field).unwrap();
    app.type_text(field, "200\n").unwrap();
    assert!(saved(200), "结束时恢复默认 200: {:?}", saved_fade_ms());
}

/// 数据目录里 library.json 的 eq 段（防抖 400ms 后在后台线程写盘，
/// 调用方轮询）。
fn saved_eq() -> Option<serde_json::Value> {
    let file =
        std::path::Path::new(&std::env::var("ZEO_PLAYER_DATA_DIR").unwrap()).join("library.json");
    std::fs::read_to_string(&file)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("eq").cloned())
}

/// 设置弹窗 →「播放」分类 →「均衡器」：开开关、选「低音增强」预设，
/// 频段值立即显示并落盘；结束时关掉开关恢复默认。
#[test]
fn equalizer_preset_applies_and_persists() {
    let mut app = start(1200.0, 800.0);
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[播放]").unwrap();
    app.wait_for("label:启用均衡器").unwrap();
    app.press("label:启用均衡器").unwrap();
    // 开关打开后 9 段竖推子出现（各段当前值显示 +0.0 dB）。
    app.wait_for("+0.0 dB").unwrap();
    // 触发器按压偶发打不开弹层（抬起触摸落在刚挂上的弹层上算作外部
    // 按压又把它关掉）：按到菜单出现为止。
    let mut opened = false;
    for _ in 0..10 {
        app.press("label:预设").unwrap();
        if app
            .wait_for_within("role:option[低音增强]", Duration::from_millis(500))
            .is_ok()
        {
            opened = true;
            break;
        }
    }
    assert!(opened, "预设菜单应打开");
    app.press("role:option[低音增强]").unwrap();
    // 低音增强的第一段（31 Hz）是 +6.0 dB，立即显示。
    app.wait_for("+6.0 dB").unwrap();
    let saved = |want: bool| {
        for _ in 0..40 {
            match saved_eq() {
                Some(eq)
                    if eq["enabled"].as_bool() == Some(want)
                        && eq["bands"][0].as_f64() == Some(if want { 6.0 } else { 0.0 }) =>
                {
                    return true;
                }
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        false
    };
    assert!(
        saved(true),
        "eq 应落盘为 enabled+低音增强: {:?}",
        saved_eq()
    );
    // 共享全局配置，用例结束前恢复：回平直、关开关。
    app.press("label:重置").unwrap();
    app.press("label:启用均衡器").unwrap();
    assert!(saved(false), "结束时恢复关闭: {:?}", saved_eq());
}

/// 数据目录里 library.json 的 global_lyrics_offset_ms（保存在后台线程，
/// 调用方轮询）。
fn saved_global_offset() -> Option<i64> {
    let file =
        std::path::Path::new(&std::env::var("ZEO_PLAYER_DATA_DIR").unwrap()).join("library.json");
    std::fs::read_to_string(&file)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["global_lyrics_offset_ms"].as_i64())
}

/// 全屏歌词区右缘的每曲偏移栏：±0.5s 步进、显示，重置在数值点开的弹层里。
#[test]
fn lyrics_offset_nudges_and_resets() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    goto_folders(&mut app);
    app.press("zeo-player-test-music").unwrap();
    app.wait_for("text:测试歌曲").unwrap();
    double_press(&mut app, "role:button[测试歌曲]");
    app.press("label:全屏播放").unwrap();
    app.wait_for("label:歌词提前").unwrap();
    // 暂停：测试曲只有 10 秒，播完自动切到无歌词的曲目会把偏移栏收掉。
    zeo_player::engine::Player::global().pause();
    app.press("label:歌词提前").unwrap();
    app.press("label:歌词提前").unwrap();
    app.wait_for("+1.0s").unwrap();
    // 「重置」在偏移数值点开的滑条弹层里；触发器按压偶发打不开弹层，
    // 按到出现为止（同「预设」菜单）。
    let mut opened = false;
    for _ in 0..10 {
        app.press("label:调整歌词时间偏移").unwrap();
        if app
            .wait_for_within("label:重置歌词偏移", Duration::from_millis(500))
            .is_ok()
        {
            opened = true;
            break;
        }
    }
    assert!(opened, "偏移弹层应打开");
    app.press("label:重置歌词偏移").unwrap();
    app.wait_for("0s").unwrap();
    // 共享播放器：别让它放着歌去影响别的用例。
    zeo_player::engine::Player::global().pause();
}

/// 均衡器竖推子在设置滚动区里拖得动（responder 的 claim 要挡下滚动视图
/// 抢拖）：上拖 1/4 行程（148pt）从 0 到 +6.0 dB。
#[test]
fn eq_fader_drags_in_settings_scroll() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[播放]").unwrap();
    app.wait_for("label:启用均衡器").unwrap();
    app.press("label:启用均衡器").unwrap();
    app.wait_for("+0.0 dB").unwrap();
    app.drag("label:均衡器 1k", point(px(0.0), px(-37.0)))
        .unwrap();
    app.wait_for("+6.0 dB").unwrap();
    // 共享全局配置，用例结束前恢复：回平直、关开关。
    app.press("label:重置").unwrap();
    app.press("label:启用均衡器").unwrap();
}

/// 设置 →「歌词」分类的「全局歌词偏移」数字输入：正常值、钳到 ±30000、
/// 非法回退；设好后全屏歌词区偏移弹层里的「全局」小字出现（每曲那份不受影响）。
#[test]
fn lyrics_offset_global_saves() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[歌词]").unwrap();
    app.wait_for("label:全局歌词偏移").unwrap();
    let field = "label:全局歌词偏移";
    let saved = |ms: i64| {
        for _ in 0..40 {
            if saved_global_offset() == Some(ms) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    };
    app.clear(field).unwrap();
    app.type_text(field, "300\n").unwrap();
    assert!(saved(300), "应为 300: {:?}", saved_global_offset());
    app.clear(field).unwrap();
    app.type_text(field, "99999\n").unwrap();
    assert!(saved(30000), "应钳到 30000: {:?}", saved_global_offset());
    app.clear(field).unwrap();
    app.type_text(field, "abc\n").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(saved_global_offset(), Some(30000), "非法输入不应改值");
    // 设回 300 后去全屏看小字提示。
    app.clear(field).unwrap();
    app.type_text(field, "300\n").unwrap();
    assert!(saved(300), "改回 300: {:?}", saved_global_offset());
    app.press("label:关闭设置").unwrap();
    goto_folders(&mut app);
    app.press("zeo-player-test-music").unwrap();
    app.wait_for("text:测试歌曲").unwrap();
    double_press(&mut app, "role:button[测试歌曲]");
    app.press("label:全屏播放").unwrap();
    app.wait_for("label:歌词提前").unwrap();
    // 暂停：测试曲只有 10 秒，播完自动切到无歌词的曲目会把偏移栏收掉。
    zeo_player::engine::Player::global().pause();
    // 「全局 +0.3s」小字在偏移数值点开的弹层里；触发器按压偶发落空，重试。
    let mut opened = false;
    for _ in 0..10 {
        app.press("label:调整歌词时间偏移").unwrap();
        if app
            .wait_for_within("全局 +0.3s", Duration::from_millis(500))
            .is_ok()
        {
            opened = true;
            break;
        }
    }
    assert!(opened, "偏移弹层应打开");
    assert!(has(&app, "0s"), "每曲偏移不应受影响");
    // 共享全局配置，用例结束前恢复 0。等全屏淡出完再点侧栏（淡出中的
    // 覆盖层仍画在「设置」上，新版 nsh-testing 会判遮挡）。
    app.press("label:收起").unwrap();
    app.wait_for_gone("label:收起", Duration::from_secs(2))
        .unwrap();
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[歌词]").unwrap();
    app.wait_for("label:全局歌词偏移").unwrap();
    app.clear(field).unwrap();
    app.type_text(field, "0\n").unwrap();
    assert!(saved(0), "结束时恢复 0: {:?}", saved_global_offset());
    zeo_player::engine::Player::global().pause();
}
