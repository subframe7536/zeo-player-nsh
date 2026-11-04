//! 界面测试：无头引擎运行根组件 `App`（`cargo test` / `nsh test` 都会跑）。
//!
//! 曲库用独立的数据目录；有 `.nsh/tmp/zeo-player-test-music` 语料（`scripts/make-test-music.sh`）
//! 时把它设为根文件夹，没有时只跑不依赖曲目的部分。

use std::sync::Once;
use std::time::Duration;

use nsh_core::geometry::{point, px, size};
use nsh_core::platform::Os;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use zeo_player::App;

const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/.nsh/tmp/zeo-player-test-music"
);

/// 隔离的数据 / 缓存目录（整个测试进程一次；曲库存储是全局的）。
fn isolate() -> bool {
    static ONCE: Once = Once::new();
    let corpus = std::path::Path::new(CORPUS).is_dir();
    ONCE.call_once(|| {
        let data = std::env::temp_dir().join(format!("zeo-player-ui-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        if corpus {
            // 语料只有 1–3 秒、几十 KB：library.json 里把过滤阈值写成 0。
            let config = serde_json::json!({
                "roots": [dunce::canonicalize(CORPUS).unwrap()],
                "min_duration_secs": 0,
                "min_size_kb": 0,
            });
            std::fs::write(data.join("library.json"), config.to_string()).unwrap();
        }
        // SAFETY: 在任何线程读这些变量之前设置（Once 内，测试开始前）。
        unsafe {
            std::env::set_var("ZEO_PLAYER_DATA_DIR", &data);
        }
    });
    corpus
}

/// 播放器是进程内全局单例：`init` 的 `restore_last` 会换掉共享的当前
/// 曲目。播放敏感的用例持锁跑全程；其它用例起 app 时短暂取锁，撞上了
/// 就等它放完。
static PLAY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 持锁的用例 panic 会毒化 Mutex——锁里只有顺序语义没有数据，照拿。
fn play_lock() -> std::sync::MutexGuard<'static, ()> {
    PLAY_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// 桌面尺寸（宽 × 高，点）启动应用，主题与 nsh.json 相同。
fn start(width: f32, height: f32) -> Headless {
    let _guard = play_lock();
    start_inner(width, height)
}

fn start_inner(width: f32, height: f32) -> Headless {
    isolate();
    let theme = nsh_base::Theme::from_json(include_str!("../theme.json")).expect("theme.json");
    let mut config = EngineConfig::new(Root::of::<App>().tokens(theme.into_tokens()));
    config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
    config.viewport = size(px(width), px(height));
    config.environment.os = Os::MacOs;
    // 静音（在挂载前）：按压与恢复的播放会真的走 cpal 输出，测试不要出声。
    zeo_player::engine::Player::global().set_volume(0.0);
    let mut app = Headless::start(config).expect("start the app");
    app.wait_for("文件夹").unwrap();
    app
}

fn texts(app: &Headless) -> Vec<String> {
    app.screen()
        .texts()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn has(app: &Headless, s: &str) -> bool {
    app.screen().texts().contains(&s)
}

/// 双击播放 `sel`：行的双击窗口是 450ms，并行负载下两次 `press` 的往返
/// 间隔可能超过它、退化成两次单击——连按到有相邻一对落进窗口为止（有上
/// 界，超时说明播放真的没发生）。退出要看「选中且在播」，不能只看选中：
/// `restore_last` 会把上次暂停的曲目恢复成暂停的当前行（selected 为真），
/// 那时一次单击什么都不会触发。
fn double_press(app: &mut Headless, sel: &str) {
    for _ in 0..12 {
        if app.screen().selected(sel).unwrap_or(false)
            && zeo_player::engine::Player::global().state().is_playing
        {
            return;
        }
        app.press(sel).unwrap();
        std::thread::sleep(Duration::from_millis(40));
    }
    panic!("双击 {sel} 未播放: {:?}", texts(app));
}

#[test]
fn sidebar_navigates_between_library_views() {
    let mut app = start(1200.0, 800.0);
    for label in ["歌手", "专辑", "标签"] {
        assert!(has(&app, label), "{label}: {:?}", texts(&app));
    }
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
        "标签",
        "通用",
        "元数据",
    ] {
        assert!(has(&app, label), "{label}: {:?}", texts(&app));
    }
    for (pane, section) in [
        ("曲库", "音乐文件夹"),
        ("标签", "分号"),
        ("播放", "淡入淡出"),
        ("窗口", "关闭窗口时"),
        ("歌词", "歌词对齐"),
        ("封面", "本地图片优先"),
    ] {
        app.press(&format!("id:settings-nav >> role:button[{pane}]"))
            .unwrap();
        app.wait_for(section).unwrap();
    }
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
    assert!(has(&app, "系统字体"));
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
            .wait_for_within("role:menuitem[低音增强]", Duration::from_millis(500))
            .is_ok()
        {
            opened = true;
            break;
        }
    }
    assert!(opened, "预设菜单应打开");
    app.press("role:menuitem[低音增强]").unwrap();
    // 低音增强的第一段（31 Hz）是 +6.0 dB，立即显示。
    app.wait_for("+6.0 dB").unwrap();
    let saved = |want: bool| {
        for _ in 0..40 {
            match saved_eq() {
                Some(eq)
                    if eq["enabled"].as_bool() == Some(want)
                        && eq["bands"][0].as_f64() == Some(if want { 6.0 } else { 0.0 }) =>
                {
                    return true
                }
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        false
    };
    assert!(saved(true), "eq 应落盘为 enabled+低音增强: {:?}", saved_eq());
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
    app.press("文件夹").unwrap();
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
    app.press("文件夹").unwrap();
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
    // 共享全局配置，用例结束前恢复 0。
    app.press("label:收起").unwrap();
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[歌词]").unwrap();
    app.wait_for("label:全局歌词偏移").unwrap();
    app.clear(field).unwrap();
    app.type_text(field, "0\n").unwrap();
    assert!(saved(0), "结束时恢复 0: {:?}", saved_global_offset());
    zeo_player::engine::Player::global().pause();
}

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

/// 设置弹窗 →「外观」分类 →「主题色」色块：选中态即时生效并落盘。
#[test]
fn accent_swatches_switch_and_persist_the_theme_color() {
    let mut app = start(1200.0, 800.0);
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[外观]").unwrap();
    app.wait_for("id:settings-content >> text:主题色").unwrap();
    let swatch = |label: &str| format!("id:settings-content >> role:button[主题色：{label}]");
    app.wait_until(
        "「封面」选中（出厂默认）",
        Duration::from_secs(2),
        |s| s.selected(&swatch("封面")).unwrap_or(false),
    )
    .unwrap();
    app.press(&swatch("红")).unwrap();
    app.wait_until("「红」选中", Duration::from_secs(2), |s| {
        s.selected(&swatch("红")).unwrap_or(false)
    })
    .unwrap();
    assert!(!app.screen().selected(&swatch("封面")).unwrap());
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
    app.press(&swatch("封面")).unwrap();
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
    app.press("文件夹").unwrap();
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

/// 「在列表中显示封面」关掉：曲目表的封面列整列移除——行内封面框与
/// 首字占位都不画，开关切回整列恢复；播放条的当前曲目封面不受影响
/// 一直显示。设置是全局持久化的，测完开回来。
#[test]
fn list_rows_drop_the_cover_column_when_covers_are_off() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("文件夹").unwrap();
    app.press("zeo-player-test-music").unwrap();
    app.wait_for("text:测试歌曲").unwrap();
    let frame = "role:button[测试歌曲] >> id:cover";
    let img = "role:button[测试歌曲] >> id:cover-img";
    let letter = "role:button[测试歌曲] >> text:测";
    let bar_img = "role:button[展开播放页] >> id:cover-img";
    // 默认开：语料曲目带内嵌封面，图片元素在；首字占位垫在底下。
    app.wait_until("封面图已就位", Duration::from_secs(10), |s| {
        s.count(img).unwrap_or(0) == 1
    })
    .unwrap();
    assert_eq!(app.screen().count(letter).unwrap(), 1);
    // 播起来：播放条的封面不受列表开关影响，断言它一直有图。
    double_press(&mut app, "role:button[测试歌曲]");
    app.wait_until("播放条封面已就位", Duration::from_secs(10), |s| {
        s.count(bar_img).unwrap_or(0) == 1
    })
    .unwrap();
    // 设置 → 封面 → 关掉「在列表中显示封面」：整列没了——图与首字占位
    // 都不画；播放条封面还在。
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[封面]").unwrap();
    app.wait_for("role:switch[在列表中显示封面]").unwrap();
    app.press("role:switch[在列表中显示封面]").unwrap();
    app.press("label:关闭设置").unwrap();
    app.wait_until("封面列应整列消失", Duration::from_secs(5), |s| {
        s.count(frame).unwrap_or(1) == 0
    })
    .unwrap();
    assert_eq!(app.screen().count(img).unwrap(), 0);
    assert_eq!(app.screen().count(letter).unwrap(), 0);
    assert_eq!(app.screen().count(bar_img).unwrap(), 1, "播放条封面应一直显示");
    // 队列弹层的行也没有封面位了（行在，封面槽被移除）。
    app.press("label:播放队列").unwrap();
    app.wait_for("label:关闭播放队列").unwrap();
    assert_eq!(
        app.screen().count("role:button[测试歌曲]").unwrap(),
        2,
        "列表行 + 队列行"
    );
    assert_eq!(app.screen().count(frame).unwrap(), 0);
    app.press("label:关闭播放队列").unwrap();
    // 开回来（别把关闭状态留给同进程的其他用例）。
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[封面]").unwrap();
    app.press("role:switch[在列表中显示封面]").unwrap();
    app.press("label:关闭设置").unwrap();
    app.wait_until("封面图应回来", Duration::from_secs(10), |s| {
        s.count(img).unwrap_or(0) == 1
    })
    .unwrap();
    zeo_player::engine::Player::global().pause();
}

/// 设置弹窗 →「外观」分类 →「字体」下拉：触发器即当前字体；弹层里有
/// 搜索框与预览行，选「系统字体」立即生效并关弹层。
#[test]
fn font_select_opens_a_searchable_popover() {
    let mut app = start(1200.0, 800.0);
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[外观]").unwrap();
    app.wait_for("id:settings-content >> label:字体").unwrap();
    // 弹层未开时预览行不在屏上。
    assert!(!has(&app, "系统字体 · 歌词 Aa あ"));
    app.press("id:settings-content >> label:字体").unwrap();
    assert!(has(&app, "系统字体 · 歌词 Aa あ"), "{:?}", texts(&app));
    app.press("label:系统字体").unwrap();
    assert!(!has(&app, "系统字体 · 歌词 Aa あ"), "选完弹层应关闭");
}

/// 文件夹页：左侧目录树只列文件夹（根与子文件夹），右侧默认显示第一个
/// 根直接包含的曲目；点子文件夹后右侧换成它的文件。
#[test]
fn folders_page_shows_tree_and_files() {
    let mut app = start(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("文件夹").unwrap();
    // 子文件夹要等扫描完曲目才出现在树里，顺带等到曲库就绪。
    app.wait_for_within("text:sub", Duration::from_secs(20))
        .unwrap();
    let t = texts(&app);
    assert!(t.iter().any(|s| s == &root_name()), "{t:?}");
    for label in ["大小", "时长", "格式"] {
        assert!(t.iter().any(|s| s == label), "{label}: {t:?}");
    }
    // 默认选中根：右侧是它直接包含的曲目（子文件夹里的不在列）。
    app.wait_for("text:测试歌曲").unwrap();
    app.press("sub").unwrap();
    app.wait_for("text:mono").unwrap();
    app.wait_for("text:short").unwrap();
}

/// 目录树增删改：右键新建 → 磁盘上真建出来并进树（空文件夹也显示）；
/// 重命名 → 路径换名；删除走「移到回收站」确认（先取消后确认）。曲目
/// 行右键里有「重命名文件…」。语料是共享的，只动自己建的文件夹。
#[test]
fn folders_page_tree_ops() {
    let mut app = start(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("文件夹").unwrap();
    app.wait_for_within("text:sub", Duration::from_secs(20))
        .unwrap();
    let sub = dunce::canonicalize(CORPUS).unwrap().join("sub");
    let made = sub.join("临时文件夹");
    let renamed = sub.join("改名后");
    for p in [&made, &renamed] {
        if p.exists() {
            // 上次跑挂了的残局先清掉。
            let _ = std::fs::remove_dir_all(p);
        }
    }

    // 右键树里的 sub：增删改三项都在；「新建子文件夹」开输入弹窗。
    app.right_click("label:sub").unwrap();
    for item in ["新建子文件夹…", "重命名…", "删除文件夹…"] {
        app.wait_for(&format!("role:menuitem[{item}]")).unwrap();
    }
    app.press("role:menuitem[新建子文件夹…]").unwrap();
    app.wait_for("label:名称").unwrap();
    app.type_text("label:名称", "临时文件夹").unwrap();
    app.press("label:创建").unwrap();
    app.wait_for("text:临时文件夹").unwrap();
    assert!(made.is_dir(), "新建应在磁盘上建出目录");

    // 重命名：树行右键 → 重命名… → 输入新名 → 确定；后台改完树刷新。
    app.right_click("label:临时文件夹").unwrap();
    app.press("role:menuitem[重命名…]").unwrap();
    app.wait_for("label:名称").unwrap();
    app.clear("label:名称").unwrap();
    app.type_text("label:名称", "改名后").unwrap();
    app.press("role:button[重命名]").unwrap();
    app.wait_for("text:改名后").unwrap();
    assert!(
        renamed.is_dir() && !made.exists(),
        "重命名应改磁盘上的目录名"
    );

    // 删除：确认弹窗先取消（目录还在），再确认（没了）。
    app.right_click("label:改名后").unwrap();
    app.press("role:menuitem[删除文件夹…]").unwrap();
    app.wait_for("role:button[移到回收站]").unwrap();
    app.press("label:取消").unwrap();
    app.wait_for_gone("role:button[移到回收站]", Duration::from_secs(2))
        .unwrap();
    assert!(renamed.is_dir());
    app.right_click("label:改名后").unwrap();
    app.press("role:menuitem[删除文件夹…]").unwrap();
    app.press("role:button[移到回收站]").unwrap();
    app.wait_for_gone("text:改名后", Duration::from_secs(5))
        .unwrap();
    assert!(!renamed.exists(), "确认删除后目录应没了");

    // 曲目行右键菜单里有「重命名文件…」，弹窗能开能取消（不动共享文件）。
    app.wait_for("text:mono").unwrap();
    app.right_click("label:mono").unwrap();
    app.press("role:menuitem[重命名文件…]").unwrap();
    app.wait_for("label:名称").unwrap();
    app.press("label:取消").unwrap();
}

/// 右键菜单互斥：再开一个菜单时旧的关掉（右键按下不产生 PressOutside，
/// 平台不会收旧菜单，靠 `store::menu_control` 的令牌互斥）；右键点到
/// 没有菜单的地方，开着的菜单也要关掉。
#[test]
fn context_menus_replace_each_other() {
    let mut app = start(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("文件夹").unwrap();
    app.wait_for_within("text:sub", Duration::from_secs(20))
        .unwrap();
    app.press("sub").unwrap();
    app.wait_for("text:mono").unwrap();

    // 开树行的菜单。
    app.right_click("label:sub").unwrap();
    app.wait_for("role:menuitem[新建子文件夹…]").unwrap();
    assert_eq!(app.screen().count("role:menu").unwrap(), 1);

    // 再开曲目行的：树菜单关掉，曲目菜单开——仍只有一个。
    app.right_click("label:mono").unwrap();
    app.wait_for("role:menuitem[重命名文件…]").unwrap();
    app.wait_for_gone("role:menuitem[新建子文件夹…]", Duration::from_secs(2))
        .unwrap();
    assert_eq!(app.screen().count("role:menu").unwrap(), 1);

    // 右键点在没有菜单的地方（页头）：菜单关掉。
    app.right_click("text:文件夹 >> nth=0").unwrap();
    app.wait_for_gone("role:menuitem[重命名文件…]", Duration::from_secs(2))
        .unwrap();
    assert_eq!(app.screen().count("role:menu").unwrap(), 0);
}

#[test]
fn narrow_windows_drop_album_and_size_columns() {
    let mut app = start(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("文件夹").unwrap();
    app.wait_for_within("大小", Duration::from_secs(20))
        .unwrap();
    // 侧栏收成图标栏（< 900 pt，导航文字被裁掉不绘制），内容区 572 pt：
    // 大小与专辑列收起——「专辑」文本一条都不剩（侧栏导航也没有）。
    app.set_viewport(size(px(640.0), px(800.0))).unwrap();
    assert!(!has(&app, "大小"));
    assert!(!has(&app, "专辑"));
    assert!(has(&app, "时长"));
}

/// 序号列（`#`）只在专辑详情显示（值是标签曲目号）；文件夹页与歌手详
/// 情的曲目表都没有这一列。
#[test]
fn number_column_only_in_album_detail() {
    let mut app = start(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("文件夹").unwrap();
    // 等扫描完（树里的 sub 出现即曲库就绪）。
    app.wait_for_within("text:sub", Duration::from_secs(20))
        .unwrap();
    // 文件夹页：没有 `#` 列头。
    assert!(!has(&app, "#"), "{:?}", texts(&app));
    // 歌手详情：也没有。
    app.press("label:歌手").unwrap();
    app.wait_for("role:button[测试歌手 A; 测试歌手 B]").unwrap();
    app.press("role:button[测试歌手 A; 测试歌手 B]").unwrap();
    app.wait_for("label:返回").unwrap();
    assert!(!has(&app, "#"), "歌手详情: {:?}", texts(&app));
    app.press("label:返回").unwrap();
    // 专辑详情：有 `#` 列头（行内是标签曲目号；sine.flac 若正是共享播放
    // 器的当前曲目，格子里是播放图标而不是「1」，不断言数字本身）。
    app.press("label:专辑").unwrap();
    app.wait_for("role:button[测试专辑]").unwrap();
    app.press("role:button[测试专辑]").unwrap();
    app.wait_for("#").unwrap();
    app.press("label:返回").unwrap();
}

#[test]
fn full_player_opens_and_closes() {
    let mut app = start(1200.0, 800.0);
    app.press("label:全屏播放").unwrap();
    app.wait_for("label:收起").unwrap();
    let t = texts(&app);
    assert!(t.iter().any(|s| s == "Zeo Player"), "{t:?}");
    // 没在播放时才没有歌词区；播放器是共享的，别的用例可能已装上曲目。
    if has(&app, "未在播放") {
        assert!(
            !t.iter().any(|s| s == "暂无歌词" || s == "正在载入歌词…"),
            "{t:?}"
        );
    }
    app.press("label:收起").unwrap();
    app.wait_for_gone("label:收起", Duration::from_secs(2))
        .unwrap();
}

/// 搜索框：点框里的清除按钮不失焦，点框外（侧栏）失焦。
#[test]
fn search_box_blurs_when_pressing_outside() {
    use nsh_core::Command;
    let blurs = |app: &Headless| {
        app.screen()
            .commands()
            .iter()
            .filter(|(_, c)| *c == Command::Blur)
            .count()
    };
    let mut app = start(1200.0, 800.0);
    app.type_text("label:搜索", "zz").unwrap();
    app.wait_for("label:清除搜索").unwrap();
    let before = blurs(&app);
    // 有搜索词时搜索页页顶也有一个同样的清除按钮；侧栏的在树里更靠前。
    app.press("label:清除搜索 >> nth=0").unwrap();
    app.wait(Duration::from_millis(50)).unwrap();
    assert_eq!(blurs(&app), before, "点在搜索框里不失焦");
    app.type_text("label:搜索", "zz").unwrap();
    app.press("label:歌手").unwrap();
    app.wait(Duration::from_millis(50)).unwrap();
    assert_eq!(blurs(&app), before + 1, "点在框外失焦");
    app.press("label:专辑").unwrap();
    app.wait(Duration::from_millis(50)).unwrap();
    assert_eq!(blurs(&app), before + 1, "已失焦后不再发");
}

#[test]
fn queue_panel_opens_from_the_player_bar() {
    let mut app = start(1200.0, 800.0);
    app.press("label:播放队列").unwrap();
    app.wait_for("label:关闭播放队列").unwrap();
    let t = texts(&app);
    assert!(
        t.iter().any(|s| s == "没有曲目" || s.starts_with("第 ")),
        "{t:?}"
    );
    app.press("label:关闭播放队列").unwrap();
}

/// 歌单：新建手动歌单、右键把曲目加进去、从歌单移除；新建智能歌单并看
/// 实时匹配数；删除前确认。（歌单文件在共享的数据目录里，只在这一个用例里改。）
#[test]
fn playlists_manual_and_smart() {
    let mut app = start(1200.0, 800.0);
    let corpus = isolate();
    assert!(has(&app, "歌单"), "{:?}", texts(&app));

    // 新建手动歌单：打开页面，空状态。
    app.press("label:添加歌单").unwrap();
    app.press("role:menuitem[新建歌单]").unwrap();
    app.wait_for("label:歌单名称").unwrap();
    app.clear("label:歌单名称").unwrap();
    app.type_text("label:歌单名称", "测试歌单").unwrap();
    app.press("label:创建").unwrap();
    app.wait_for_gone("label:歌单名称", Duration::from_secs(2))
        .unwrap();
    app.wait_for("歌单是空的").unwrap();
    assert!(has(&app, "测试歌单"), "{:?}", texts(&app));

    if corpus {
        // 文件夹页右键第一首 → 添加到歌单 → 测试歌单。
        app.press("文件夹").unwrap();
        app.wait_for_within("大小", Duration::from_secs(20))
            .unwrap();
        app.press(&format!("text:{}", root_name())).unwrap();
        app.wait_for_within("label:测试歌曲", Duration::from_secs(5))
            .unwrap();
        app.right_click("label:测试歌曲").unwrap();
        app.press("label:添加到歌单").unwrap();
        app.press("role:menuitem[测试歌单]").unwrap();
        app.press("role:button[测试歌单]").unwrap();
        app.wait_for("label:测试歌曲").unwrap();
        assert!(!has(&app, "歌单是空的"));
        // 从歌单移除。
        app.right_click("label:测试歌曲").unwrap();
        app.press("role:menuitem[从歌单中移除]").unwrap();
        app.wait_for("歌单是空的").unwrap();
    }

    // 新建智能歌单：条件为空时匹配整个曲库，填了不可能的值后为 0。
    app.press("label:添加歌单").unwrap();
    app.press("role:menuitem[新建智能歌单]").unwrap();
    app.wait_for("编辑智能歌单")
        .or_else(|_| app.wait_for("新建智能歌单"))
        .unwrap();
    app.wait_for("text:/^匹配 \\d+ 首$/").unwrap();
    if corpus {
        // 字段换「标签」后值变成选择器：列出曲库的标签，选 Indie 命中 1 首。
        app.press("label:字段").unwrap();
        app.press("role:menuitem[标签]").unwrap();
        app.press("label:值").unwrap();
        app.press("role:menuitem[Indie]").unwrap();
        app.wait_for("匹配 1 首").unwrap();
        app.press("label:字段").unwrap();
        app.press("role:menuitem[歌手]").unwrap();
        app.clear("label:值").unwrap();
    }
    app.type_text("label:值", "不存在的歌手-zz").unwrap();
    app.wait_for("匹配 0 首").unwrap();
    app.press("label:创建").unwrap();
    app.wait_for("没有符合条件的曲目").unwrap();
    assert!(has(&app, "智能歌单"), "{:?}", texts(&app));

    // 删除：确认后回到文件夹页，侧栏里没有了。
    app.on_dialog(|_| nsh_testing::DialogAnswer::Press("删除".into()));
    app.press("label:删除歌单").unwrap();
    app.wait_for_gone("没有符合条件的曲目", Duration::from_secs(2))
        .unwrap();
    assert!(!has(&app, "智能歌单"), "{:?}", texts(&app));
    assert!(has(&app, "测试歌单"));
}

/// 曲目行：双击才播放（单击不动）；喜欢列；右键菜单的删除项与「删除
/// 文件…」确认弹窗。
#[test]
fn track_row_double_click_like_and_menu() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("文件夹").unwrap();
    app.press("zeo-player-test-music").unwrap();
    app.wait_for("text:测试歌曲").unwrap();
    let row = "role:button[测试歌曲]";
    // 共享播放器：别的用例可能已把它播成当前曲目——那下单击断言无从
    // 区分「没播」与「本来就选着」，只跑双击那半。
    let was_current = app.screen().selected(row).unwrap_or(false);
    app.press(row).unwrap();
    if !was_current {
        assert!(!app.screen().selected(row).unwrap_or(false), "单击不应播放");
    }
    double_press(&mut app, row);
    zeo_player::engine::Player::global().pause();

    // 喜欢列：行内爱心按钮，点了变「取消喜欢」。
    app.press("role:button[测试歌曲] >> role:button[喜欢]")
        .unwrap();
    app.wait_for("role:button[测试歌曲] >> role:button[取消喜欢]")
        .unwrap();

    // 右键菜单：删除两项都在（context_menu 的项是普通 menuitem 元素）；
    // 「删除文件…」只开确认弹窗（取消，不真删）。
    app.right_click(row).unwrap();
    app.wait_for("role:menuitem[从曲库中删除]").unwrap();
    for item in ["播放", "添加到歌单", "删除文件…"] {
        app.wait_for(&format!("role:menuitem[{item}]")).unwrap();
    }
    app.press("role:menuitem[删除文件…]").unwrap();
    app.wait_for("移到回收站").unwrap();
    app.press("取消").unwrap();
    app.wait_for_gone("移到回收站", Duration::from_secs(2))
        .unwrap();

    // 还原：取消喜欢，别让喜欢状态漏给别的用例。
    app.press("role:button[测试歌曲] >> role:button[取消喜欢]")
        .unwrap();
}

/// 首页是默认页：侧栏「首页」选中；有语料时随机推荐（「随机翻到了」
/// 小歌单卡 + 「换一批」）与曲库 / 播放统计标签都在。没语料时是曲库为
/// 空的空状态。
#[test]
fn home_is_default_and_shows_stats() {
    let mut app = start(1200.0, 800.0);
    assert!(
        app.screen().selected("role:button[首页]").unwrap_or(false),
        "默认应停在首页: {:?}",
        texts(&app)
    );
    if !isolate() {
        assert!(has(&app, "还没有音乐文件夹"), "{:?}", texts(&app));
        return;
    }
    // 曲库扫描完才出内容（扫描期间首页是空状态）。
    app.wait_for_within("label:立即播放", Duration::from_secs(20))
        .unwrap();
    // 「随机翻到了「X」」的曲名是动态的，按前缀查。
    assert!(
        texts(&app).iter().any(|t| t.starts_with("随机翻到了「")),
        "{:?}",
        texts(&app)
    );
    // 头部：五档时段问候之一 + 三个本周指标。
    let greetings = ["早上好", "中午好", "下午好", "晚上好", "夜深了"];
    assert!(
        greetings.iter().any(|g| has(&app, g)),
        "时段问候: {:?}",
        texts(&app)
    );
    for label in ["本周时长", "新增收藏", "连续收听"] {
        assert!(has(&app, label), "{label}: {:?}", texts(&app));
    }
    // 「新添加」等 added 表对账完（db::later 异步 + 2 秒轮询重读）才出现。
    app.wait_for("新添加").unwrap();
    // 换一批重抽：卡还在、按钮还在（语料里必有一首被抽中）。
    app.press("label:换一批").unwrap();
    assert!(
        texts(&app).iter().any(|t| t.starts_with("随机翻到了「")),
        "换一批后: {:?}",
        texts(&app)
    );
}

/// 播放统计：听过阈值那一刻就计一次（不等播完）。把阈值改成「实际播
/// 1 秒即计」后播 sine 3 秒，回首页应出现「最常播放」一节；测完把阈值
/// 恢复成默认 60% / 30 秒并暂停共享播放器。
#[test]
fn home_counts_a_play() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    // 设置 → 播放 → 播放统计：百分比改 0、秒数改 1（任一满足即计）。
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[播放]").unwrap();
    app.wait_for("label:最少播放时长").unwrap();
    app.clear("label:最少播放时长").unwrap();
    app.type_text("label:最少播放时长", "1\n").unwrap();
    app.clear("label:最少播放百分比").unwrap();
    app.type_text("label:最少播放百分比", "0\n").unwrap();
    app.press("label:关闭设置").unwrap();

    // 播三秒：阈值一过引擎即记播放（播放记录代次 +1）。
    let before = zeo_player::engine::Player::global().plays_generation();
    app.press("文件夹").unwrap();
    app.press("zeo-player-test-music").unwrap();
    app.wait_for("text:测试歌曲").unwrap();
    double_press(&mut app, "role:button[测试歌曲]");
    app.wait_until("应已记一次播放", Duration::from_secs(10), |_| {
        zeo_player::engine::Player::global().plays_generation() > before
    })
    .unwrap();
    // 共享播放器别放着歌（也防 10 秒的曲子播完自动切下一首）。
    zeo_player::engine::Player::global().pause();

    // 回首页：统计重读后「最近播放」卡在，「热门艺术家」同时在。
    app.press("role:button[首页]").unwrap();
    app.wait_for("最近播放").unwrap();
    assert!(has(&app, "热门艺术家"), "{:?}", texts(&app));
    assert!(has(&app, "测试歌曲"), "{:?}", texts(&app));

    // 「热门艺术家」深链：点歌手圆钮直接落进「歌手」详情（复用 Artists）。
    app.press("role:button[测试歌手 A; 测试歌手 B]").unwrap();
    app.wait_for("label:返回").unwrap();
    // 回网格并把选中复位，别留给后面的侧栏用例。
    app.press("label:返回").unwrap();

    // 还原阈值（60% / 30 秒），别漏给别的用例。
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[播放]").unwrap();
    app.wait_for("label:最少播放百分比").unwrap();
    app.clear("label:最少播放百分比").unwrap();
    app.type_text("label:最少播放百分比", "60\n").unwrap();
    app.clear("label:最少播放时长").unwrap();
    app.type_text("label:最少播放时长", "30\n").unwrap();
    app.press("label:关闭设置").unwrap();
}

/// 测试语料根文件夹在文件夹页里的名字。
fn root_name() -> String {
    dunce::canonicalize(CORPUS)
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}
