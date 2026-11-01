//! 界面测试：无头引擎运行根组件 `App`（`cargo test` / `nsh test` 都会跑）。
//!
//! 曲库用独立的数据目录；有 `.nsh/tmp/zeo-player-test-music` 语料（`scripts/make-test-music.sh`）
//! 时把它设为根文件夹，没有时只跑不依赖曲目的部分。

use std::sync::Once;
use std::time::Duration;

use nsh_core::geometry::{px, size};
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

/// 双击播放 `sel`（行选中了即已播）：行的双击窗口是 450ms，并行负载下
/// 两次 `press` 的往返间隔可能超过它、退化成两次单击——连按到有相邻
/// 一对落进窗口为止（有上界，超时说明播放真的没发生）。
fn double_press(app: &mut Headless, sel: &str) {
    for _ in 0..12 {
        app.press(sel).unwrap();
        if app.screen().selected(sel).unwrap_or(false) {
            return;
        }
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
    assert!(!has(&app, "添加文件夹…"));
    // 没有命中：右栏空状态。
    app.type_text("label:搜索设置", "zzz").unwrap();
    app.wait_for("没有匹配的设置").unwrap();
    // 清空：分类列表与各节都回来。
    app.press("id:settings-nav >> label:清除搜索").unwrap();
    app.wait_for("id:settings-content >> label:外观").unwrap();
    assert!(has(&app, "播放"));
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

#[test]
fn folders_page_lists_roots_with_columns() {
    let mut app = start(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.wait_for_within("大小", Duration::from_secs(20))
        .unwrap();
    let t = texts(&app);
    assert!(t.iter().any(|s| s.contains("个根文件夹")), "{t:?}");
    for label in ["时长", "格式"] {
        assert!(t.iter().any(|s| s == label), "{label}: {t:?}");
    }
}

#[test]
fn narrow_windows_drop_album_and_size_columns() {
    let mut app = start(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.wait_for_within("大小", Duration::from_secs(20))
        .unwrap();
    app.set_viewport(size(px(700.0), px(800.0))).unwrap(); // 内容区 468 pt
    assert!(!has(&app, "大小"));
    // 侧栏也有「专辑」：列头那个应已收起。
    assert_eq!(
        app.screen()
            .texts()
            .iter()
            .filter(|s| **s == "专辑")
            .count(),
        1
    );
    assert!(has(&app, "时长"));
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
    app.press("label:清除搜索").unwrap();
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

/// 测试语料根文件夹在文件夹页里的名字。
fn root_name() -> String {
    dunce::canonicalize(CORPUS)
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}
