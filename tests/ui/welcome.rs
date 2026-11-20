use super::*;

use zeo_player::config::library_store;

/// 测试配置里 `welcome_done` 写成真（见 `isolate`）；用例临时改成假来
/// 验证落盘，收尾（含 panic）一律还原——配置是进程全局的，别的用例在没
/// 语料时（空曲库）一起来就会被欢迎页盖住。
struct RestoreWelcome;

impl Drop for RestoreWelcome {
    fn drop(&mut self) {
        library_store().set_welcome_done(true);
    }
}

/// 主题色是进程全局配置，别的用例会改它：欢迎页断言出厂选中项前钉成
/// 「封面」，结束时还原。
struct RestoreThemeColor(zeo_types::config::ThemeColor);

impl RestoreThemeColor {
    fn cover() -> Self {
        let prev = library_store().config().theme_color;
        library_store().set_theme_color(zeo_types::config::ThemeColor::Cover);
        Self(prev)
    }
}

impl Drop for RestoreThemeColor {
    fn drop(&mut self) {
        library_store().set_theme_color(self.0);
    }
}

/// 数据目录里 library.json 的 welcome_done（保存在后台线程，调用方轮询）。
fn saved_welcome_done() -> Option<bool> {
    let file =
        std::path::Path::new(&std::env::var("ZEO_PLAYER_DATA_DIR").unwrap()).join("library.json");
    std::fs::read_to_string(file)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["welcome_done"].as_bool())
}

/// 等 library.json 里的 `welcome_done` 落成真（后台线程写盘）。
fn wait_saved_welcome_done() {
    for _ in 0..40 {
        if saved_welcome_done() == Some(true) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(saved_welcome_done(), Some(true));
}

/// 设置 →「关于」→「欢迎指南」→「重新查看」：设置关掉，欢迎页盖上来。
fn reopen_welcome(app: &mut Headless) {
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[关于]").unwrap();
    app.wait_for("id:settings-content >> role:button[重新查看]")
        .unwrap();
    app.press("id:settings-content >> role:button[重新查看]")
        .unwrap();
    app.wait_for("id:welcome >> text:欢迎使用 Zeo Player")
        .unwrap();
    app.wait_for_gone("id:settings-content", Duration::from_secs(2))
        .unwrap();
}

/// 逐步走完：欢迎 → 添加音乐文件夹 → 主题色（默认「封面」）→ 功能一览，
/// 「开始使用」关掉并把 `welcome_done` 落盘。
#[test]
fn welcome_walks_through_every_step_and_is_remembered() {
    // 持锁跑全程：期间 `welcome_done` 是假，别的用例的 `start` 要等到还原。
    let _guard = play_lock();
    let corpus = isolate();
    let mut app = start_inner(1200.0, 800.0);
    let _restore = RestoreWelcome;
    let _restore_color = RestoreThemeColor::cover();
    library_store().set_welcome_done(false);
    // 启动时配置里 `welcome_done` 为真：不自动弹。
    assert!(!has(&app, "欢迎使用 Zeo Player"), "{:?}", texts(&app));
    reopen_welcome(&mut app);

    // 1 欢迎
    app.press("id:welcome >> role:button[开始]").unwrap();
    // 2 添加音乐文件夹：没有文件夹时下一步叫「稍后添加」，有语料就是「下一步」。
    app.wait_for("id:welcome >> text:添加音乐文件夹").unwrap();
    app.wait_for("id:welcome >> role:button[添加文件夹…]")
        .unwrap();
    app.wait_for(
        "id:welcome >> text:也可以稍后在 设置 › 曲库 中添加，或把文件夹拖到「文件夹」页。",
    )
    .unwrap();
    let next = format!(
        "id:welcome >> role:button[{}]",
        if corpus { "下一步" } else { "稍后添加" }
    );
    app.wait_for(&next).unwrap();
    app.press(&next).unwrap();
    // 3 主题色：默认跟随封面。别的用例会短暂改全局主题色再还原，多等一会儿。
    app.wait_for("id:welcome >> role:radio[封面]").unwrap();
    app.wait_until(
        "「封面」选中（出厂默认）",
        Duration::from_secs(5),
        |s| {
            s.selected("id:welcome >> role:radio[封面]")
                .unwrap_or(false)
        },
    )
    .unwrap();
    // 上一步回得去，再前进。
    app.press("id:welcome >> role:button[上一步]").unwrap();
    app.wait_for("id:welcome >> text:添加音乐文件夹").unwrap();
    app.press(&next).unwrap();
    app.wait_for("id:welcome >> role:radio[封面]").unwrap();
    app.press("id:welcome >> role:button[下一步]").unwrap();
    // 4 功能一览。
    for title in ["命令面板", "智能歌单", "托盘与全局快捷键"] {
        app.wait_for(&format!("id:welcome >> text:{title}"))
            .unwrap();
    }
    app.press("id:welcome >> role:button[开始使用]").unwrap();
    app.wait_for_gone("id:welcome", Duration::from_secs(2))
        .unwrap();
    wait_saved_welcome_done();
}

/// 右上角「跳过」：同样关掉并记下。
#[test]
fn welcome_skip_button_skips_the_guide() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    let _restore = RestoreWelcome;
    library_store().set_welcome_done(false);
    reopen_welcome(&mut app);
    app.press("id:welcome >> role:button[跳过]").unwrap();
    app.wait_for_gone("id:welcome", Duration::from_secs(2))
        .unwrap();
    wait_saved_welcome_done();
}

/// Esc 同样算跳过。
#[test]
fn welcome_escape_skips_the_guide() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    let _restore = RestoreWelcome;
    library_store().set_welcome_done(false);
    reopen_welcome(&mut app);
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:welcome", Duration::from_secs(2))
        .unwrap();
    wait_saved_welcome_done();
}

/// 欢迎页盖着整个窗口：期间窗口内快捷键（如 ⌘K 命令面板）停用，跳过后
/// 恢复。命令面板的页脚有「导航」提示，以它判断面板开没开。
#[test]
fn welcome_suspends_window_shortcuts_until_skipped() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    let _restore = RestoreWelcome;
    library_store().set_welcome_done(false);
    reopen_welcome(&mut app);
    app.press_key("mod+k").unwrap();
    app.wait(Duration::from_millis(400)).unwrap();
    assert!(!has(&app, "导航"), "欢迎页打开时不该弹命令面板");
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:welcome", Duration::from_secs(2))
        .unwrap();
    // 快捷键在欢迎页关掉后重绑（dynamic 换 owner），给它一点时间。
    app.wait(Duration::from_millis(300)).unwrap();
    app.press_key("mod+k").unwrap();
    app.wait_for("导航").unwrap();
    app.press_key("escape").unwrap();
    wait_saved_welcome_done();
}

/// 已经添加过文件夹的用户（即使 `welcome_done` 缺省为假）启动时不弹。
#[test]
fn welcome_is_skipped_when_library_already_has_folders() {
    let _guard = play_lock();
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    let _restore = RestoreWelcome;
    library_store().set_welcome_done(false);
    let app = start_inner(1200.0, 800.0);
    assert!(!has(&app, "欢迎使用 Zeo Player"), "{:?}", texts(&app));
}
