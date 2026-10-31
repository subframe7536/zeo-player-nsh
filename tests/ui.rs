//! 界面测试：无头引擎运行根组件 `App`（`cargo test` / `nsh test` 都会跑）。
//!
//! 曲库用独立的数据目录；有 `/tmp/zeo-player-test-music` 语料（`scripts/make-test-music.sh`）
//! 时把它设为根文件夹，没有时只跑不依赖曲目的部分。

use std::sync::Once;
use std::time::Duration;

use zeo_player::App;
use nsh_core::geometry::{px, size};
use nsh_core::platform::Os;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

const CORPUS: &str = "/tmp/zeo-player-test-music";

/// 隔离的数据 / 缓存目录（整个测试进程一次；曲库存储是全局的）。
fn isolate() -> bool {
    static ONCE: Once = Once::new();
    let corpus = std::path::Path::new(CORPUS).is_dir();
    ONCE.call_once(|| {
        let data = std::env::temp_dir().join(format!("zeo-player-ui-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        if corpus {
            let config = serde_json::json!({ "roots": [dunce::canonicalize(CORPUS).unwrap()] });
            std::fs::write(data.join("library.json"), config.to_string()).unwrap();
        }
        // SAFETY: 在任何线程读这些变量之前设置（Once 内，测试开始前）。
        unsafe {
            std::env::set_var("ZEO_PLAYER_DATA_DIR", &data);
            std::env::set_var("ZEO_PLAYER_CACHE_DIR", data.join("cache"));
        }
    });
    corpus
}

/// 桌面尺寸（宽 × 高，点）启动应用，主题与 nsh.json 相同。
fn start(width: f32, height: f32) -> Headless {
    isolate();
    let theme = nsh_base::Theme::from_json(include_str!("../theme.json")).expect("theme.json");
    let mut config = EngineConfig::new(Root::of::<App>().tokens(theme.into_tokens()));
    config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
    config.viewport = size(px(width), px(height));
    config.environment.os = Os::MacOs;
    let mut app = Headless::start(config).expect("start the app");
    app.wait_for("文件夹").unwrap();
    app
}

fn texts(app: &Headless) -> Vec<String> {
    app.screen().texts().into_iter().map(str::to_owned).collect()
}

fn has(app: &Headless, s: &str) -> bool {
    app.screen().texts().contains(&s)
}

#[test]
fn sidebar_navigates_between_library_views() {
    let mut app = start(1200.0, 800.0);
    for label in ["歌手", "专辑", "标签"] {
        assert!(has(&app, label), "{label}: {:?}", texts(&app));
    }
    app.press("专辑").unwrap();
    assert!(has(&app, "未在播放"));
    app.press("设置").unwrap();
    app.wait_for("音乐文件夹").unwrap();
    for label in ["曲库", "添加文件夹…", "重建曲库", "流派标签", "分号", "监视文件夹变化", "系统字体", "关闭窗口时", "淡入淡出"] {
        assert!(has(&app, label), "{label}: {:?}", texts(&app));
    }
}

#[test]
fn folders_page_lists_roots_with_columns() {
    let mut app = start(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.wait_for_within("大小", Duration::from_secs(20)).unwrap();
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
    app.wait_for_within("大小", Duration::from_secs(20)).unwrap();
    app.set_viewport(size(px(700.0), px(800.0))).unwrap(); // 内容区 468 pt
    assert!(!has(&app, "大小"));
    // 侧栏也有「专辑」：列头那个应已收起。
    assert_eq!(app.screen().texts().iter().filter(|s| **s == "专辑").count(), 1);
    assert!(has(&app, "时长"));
}

#[test]
fn full_player_opens_and_closes() {
    let mut app = start(1200.0, 800.0);
    app.press("label:全屏播放").unwrap();
    app.wait_for("label:收起").unwrap();
    let t = texts(&app);
    assert!(t.iter().any(|s| s == "暂无歌词" || s == "正在载入歌词…" || s == "Zeo Player"), "{t:?}");
    app.press("label:收起").unwrap();
    app.wait_for_gone("label:收起", Duration::from_secs(2)).unwrap();
}

/// 搜索框：点框里的清除按钮不失焦，点框外（侧栏）失焦。
#[test]
fn search_box_blurs_when_pressing_outside() {
    use nsh_core::Command;
    let blurs = |app: &Headless| app.screen().commands().iter().filter(|(_, c)| *c == Command::Blur).count();
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
    assert!(t.iter().any(|s| s == "没有曲目" || s.starts_with("第 ")), "{t:?}");
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
    app.wait_for_gone("label:歌单名称", Duration::from_secs(2)).unwrap();
    app.wait_for("歌单是空的").unwrap();
    assert!(has(&app, "测试歌单"), "{:?}", texts(&app));

    if corpus {
        // 文件夹页右键第一首 → 添加到歌单 → 测试歌单。
        app.press("文件夹").unwrap();
        app.wait_for_within("大小", Duration::from_secs(20)).unwrap();
        app.press(&format!("text:{}", root_name())).unwrap();
        app.wait_for_within("label:测试歌曲", Duration::from_secs(5)).unwrap();
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
    app.wait_for("编辑智能歌单").or_else(|_| app.wait_for("新建智能歌单")).unwrap();
    app.wait_for("text:/^匹配 \\d+ 首$/").unwrap();
    app.type_text("label:值", "不存在的歌手-zz").unwrap();
    app.wait_for("匹配 0 首").unwrap();
    app.press("label:创建").unwrap();
    app.wait_for("没有符合条件的曲目").unwrap();
    assert!(has(&app, "智能歌单"), "{:?}", texts(&app));

    // 删除：确认后回到文件夹页，侧栏里没有了。
    app.on_dialog(|_| nsh_testing::DialogAnswer::Press("删除".into()));
    app.press("label:删除歌单").unwrap();
    app.wait_for_gone("没有符合条件的曲目", Duration::from_secs(2)).unwrap();
    assert!(!has(&app, "智能歌单"), "{:?}", texts(&app));
    assert!(has(&app, "测试歌单"));
}

/// 测试语料根文件夹在文件夹页里的名字。
fn root_name() -> String {
    dunce::canonicalize(CORPUS).unwrap().file_name().unwrap().to_string_lossy().into_owned()
}
