use super::*;

/// 即使子文件夹当前正处于选中状态，点击父文件夹的收起按钮也必须能够成功收起（子项在树中隐藏）。
#[test]
fn folder_parent_collapses_even_when_child_is_selected() {
    let mut app = start(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    goto_folders(&mut app);
    let root = root_name();
    app.wait_for_within(&format!("label:展开 {root}"), Duration::from_secs(20))
        .unwrap();
    app.press(&format!("label:展开 {root}")).unwrap();
    app.wait_for("text:sub").unwrap();
    // 选中子文件夹 sub，主视图展示 sub 里的曲目
    app.press("sub").unwrap();
    app.wait_for("text:mono").unwrap();

    let root = root_name();
    let collapse_btn = format!("label:收起 {root}");
    let expand_btn = format!("label:展开 {root}");

    // 点击父目录的收起按钮
    app.press(&collapse_btn).unwrap();
    // 收起后：树中的 sub 行消失，按钮变成「展开 ...」
    app.wait_for(&expand_btn).unwrap();
    app.wait_for_gone("role:link[sub]", Duration::from_secs(2))
        .unwrap();

    // 再次点击展开按钮：sub 重新在树中出现
    app.press(&expand_btn).unwrap();
    app.wait_for(&collapse_btn).unwrap();
    app.wait_for("role:link[sub]").unwrap();
}

/// 「元数据」页：侧栏进入见到曲目表；表头「全选」复选框勾出批量
/// 修改面板，再按一次取消选择、面板消失。
#[test]
fn metadata_select_all_toggles_batch_panel() {
    let mut app = start(1400.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("元数据").unwrap();
    app.wait_for("label:全选").unwrap();
    app.wait_for("text:测试歌曲").unwrap();
    app.press("label:全选").unwrap();
    app.wait_for("text:批量修改").unwrap();
    app.press("label:全选").unwrap();
    app.wait_for_gone("text:批量修改", Duration::from_secs(5))
        .unwrap();
}

/// 行体按一次：选中且只选中一次——选择列出主色「✓」字形，右侧出单选
/// 编辑面板，「标题」输入框预填曲目现有标题。
#[test]
fn metadata_row_press_opens_single_panel() {
    let mut app = start(1400.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("元数据").unwrap();
    app.wait_for("text:sine.flac").unwrap();
    app.press("text:sine.flac").unwrap();
    app.wait_for("label:标题").unwrap();
    assert_eq!(
        app.screen().input_text("label:标题").unwrap(),
        Some("测试歌曲"),
        "{:?}",
        texts(&app)
    );
}

/// 重命名弹窗：模板里含目录分隔符时显示分隔符报错且「重命名」禁用。
#[test]
fn metadata_rename_dialog_rejects_separators() {
    let mut app = start(1400.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("元数据").unwrap();
    app.wait_for("text:sine.flac").unwrap();
    app.press("text:sine.flac").unwrap();
    app.press("重命名文件…").unwrap();
    app.wait_for("label:模板").unwrap();
    app.type_text("label:模板", "a/b").unwrap();
    app.wait_for("text:仅支持重命名文件名，不能包含目录分隔符（/ 或 \\）")
        .unwrap();
    assert_eq!(app.screen().enabled("label:确认重命名"), Ok(false));
    app.press("取消").unwrap();
}

/// 曲库索引刷新（`s.tracks` 变更）不重建整页：筛选框的输入与单选面板
/// 都留住。用「重新扫描」触发——只重读标签，不写语料。
#[test]
fn metadata_page_survives_track_refresh() {
    let mut app = start(1400.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("元数据").unwrap();
    app.wait_for("label:筛选").unwrap();
    app.type_text("label:筛选", "sine").unwrap();
    app.wait_for("text:sine.flac").unwrap();
    app.press("text:sine.flac").unwrap();
    app.wait_for("label:标题").unwrap();
    // 触发一次只读的曲库刷新（s.tracks 换 Arc）：设置 → 曲库 →「扫描更新」。
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[曲库]").unwrap();
    app.wait_for("扫描更新").unwrap();
    app.press("label:扫描更新").unwrap();
    app.press("label:关闭设置").unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(
        app.screen().input_text("label:筛选").unwrap(),
        Some("sine"),
        "筛选框被重建丢了输入: {:?}",
        texts(&app)
    );
    assert_eq!(
        app.screen().input_text("label:标题").unwrap(),
        Some("测试歌曲"),
        "单选面板丢了: {:?}",
        texts(&app)
    );
}

/// 筛选后行按压仍选对路径：筛出 `sine.w`（只中 sine.wav），按它，
/// 单选面板顶栏标题应是 sine.wav、「文件夹」行指向语料目录；顶栏铅
/// 笔进改名模式，主干输入预填 stem。
#[test]
fn metadata_row_press_binds_path_after_filter() {
    let mut app = start(1400.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("元数据").unwrap();
    app.wait_for("label:筛选").unwrap();
    app.type_text("label:筛选", "sine.w").unwrap();
    app.wait_for("text:sine.wav").unwrap();
    app.press("text:sine.wav").unwrap();
    app.wait_for("label:标题").unwrap();
    // 顶栏标题 = 文件名（sine.wav），「文件夹」行 = 语料根目录。
    let dir = dunce::canonicalize(CORPUS)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    app.wait_until(
        "顶栏标题 / 文件夹应指向 sine.wav",
        Duration::from_secs(5),
        |s| {
            s.texts().iter().any(|t| t.ends_with("sine.wav"))
                && s.texts().iter().any(|t| t.ends_with(dir.as_str()))
        },
    )
    .unwrap();
    assert!(
        !texts(&app).iter().any(|t| t.ends_with("sine.flac")),
        "选中了别的文件: {:?}",
        texts(&app)
    );
    // 铅笔进改名模式：主干输入预填 stem（扩展名独立显示），✕ 取消。
    app.press("label:重命名").unwrap();
    assert_eq!(
        app.screen().input_text("label:文件名").unwrap(),
        Some("sine"),
        "{:?}",
        texts(&app)
    );
    app.press("label:取消重命名").unwrap();
    app.wait_for_gone("label:文件名", Duration::from_secs(5))
        .unwrap();
}

/// 顶栏改名模式：分隔符触发 `plan_name` 硬规则报错（状态行即时显
/// 示），✓ 随之禁用；✕ 退出改名态。
#[test]
fn metadata_header_rename_mode_validates() {
    let mut app = start(1400.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("元数据").unwrap();
    app.wait_for("text:sine.flac").unwrap();
    app.press("text:sine.flac").unwrap();
    app.wait_for("label:标题").unwrap();
    app.press("label:重命名").unwrap();
    app.wait_for("label:文件名").unwrap();
    app.type_text("label:文件名", "a/b").unwrap();
    app.wait_for("text:仅支持重命名文件名，不能包含目录分隔符（/ 或 \\）")
        .unwrap();
    assert_eq!(app.screen().enabled("label:完成重命名"), Ok(false));
    app.press("label:取消重命名").unwrap();
    app.wait_for_gone("label:文件名", Duration::from_secs(5))
        .unwrap();
}

/// 单选面板顶栏的 ✕：清空选中集（与范围锚点），面板连同「标题」输入
/// 一起消失。
#[test]
fn metadata_panel_close_clears_selection() {
    let mut app = start(1400.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("元数据").unwrap();
    app.wait_for("text:sine.flac").unwrap();
    app.press("text:sine.flac").unwrap();
    app.wait_for("label:标题").unwrap();
    app.press("label:关闭").unwrap();
    app.wait_for_gone("label:标题", Duration::from_secs(5))
        .unwrap();
}

#[test]
fn metadata_row_press_toggles_selection_off() {
    let mut app = start(1400.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("元数据").unwrap();
    app.wait_for("text:sine.flac").unwrap();
    // 行体按压（标题列）：选中 sine.flac，面板打开。
    app.press("text:测试歌曲").unwrap();
    app.wait_for("label:标题").unwrap();
    // 同一行再按一次：取消选中，面板收起。
    app.press("text:测试歌曲").unwrap();
    app.wait_for_gone("label:标题", Duration::from_secs(5))
        .unwrap();
}

#[test]
fn metadata_column_selector_opens() {
    let mut app = start(1400.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("元数据").unwrap();
    app.wait_for("label:列显示设置").unwrap();
    assert!(texts(&app).iter().any(|t| t == "显示列 (15/24)"));

    // 打开列设置下拉菜单
    app.press("label:列显示设置").unwrap();
    app.wait_for("text:选择显示列").unwrap();
    app.wait_for("text:基本信息").unwrap();
    app.wait_for("text:人员与版权").unwrap();
    app.wait_for("text:封面与歌词").unwrap();
    app.wait_for("text:ReplayGain").unwrap();

    // 勾选未选中的「作曲」
    app.press("text:作曲").unwrap();
    app.wait_for("text:显示列 (16/24)").unwrap();

    // 点击「默认」还原为 15 列
    app.press("text:默认").unwrap();
    app.wait_for("text:显示列 (15/24)").unwrap();

    // 验证遮罩层存在并点击遮罩层可关闭弹窗
    app.wait_for("id:column_selector_overlay").unwrap();
    app.press("id:column_selector_overlay").unwrap();
    app.wait_for_gone("text:选择显示列", std::time::Duration::from_millis(1000))
        .unwrap();
}

#[test]
fn metadata_column_resize_drag() {
    let mut app = start(1400.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("元数据").unwrap();
    app.wait_for("id:resize:title").unwrap();
    app.drag(
        "id:resize:title",
        nsh_core::geometry::point(px(50.0), px(0.0)),
    )
    .unwrap();
}

/// 窄窗口元数据视图：在 720pt 最小窗口下，工具栏按钮与输入框自适应不截断，
/// 选中曲目后右侧面板自适应收窄（280pt），左侧表格仍然维持可用宽度。
#[test]
fn metadata_narrow_window_layout() {
    let mut app = start(720.0, 720.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("元数据").unwrap();
    app.wait_for("label:筛选").unwrap();
    app.wait_for("label:全选").unwrap();
    app.wait_for("text:sine.flac").unwrap();
    // 选中一行
    app.press("text:sine.flac").unwrap();
    app.wait_for("label:标题").unwrap();
    // 验证工具栏上的操作按钮依然存在且可点击（无截断丢失）
    app.wait_for("label:重命名文件…").unwrap();
    app.wait_for("label:取消选择").unwrap();
    // 点击取消选择
    app.press("label:取消选择").unwrap();
    app.wait_for_gone("label:标题", Duration::from_secs(2))
        .unwrap();
}

/// 点窗口的关闭按钮：平台把按压报给 `App()` 的 `on_close_press`（
/// `close_on_press(false)` 下不直接关），走 `close_requested`——默认
/// 「每次询问」弹出选择而窗口不动；选「最小化到托盘」后窗口隐藏
/// （`s.hidden` 卸载 shell）而窗口对象与引擎都在。
#[test]
fn window_close_press_asks_then_hides_to_tray() {
    let mut app = start(1440.0, 900.0);
    let store = zeo_player::config::library_store();
    store.set_close_action(zeo_player::config::CloseAction::Ask);
    app.press_window_close("main").unwrap();
    app.wait_for("关闭窗口").unwrap();
    // 询问时窗口内容原样留着。
    assert!(app.shows("首页"), "ask: window keeps its contents");
    app.press("最小化到托盘").unwrap();
    app.wait_for_gone("首页", Duration::from_secs(5)).unwrap();
    // 已记住托盘：再点直接隐藏；窗口对象仍在——是拦截不是退出。
    app.press_window_close("main").unwrap();
    assert!(
        app.window_state("main").unwrap().is_some(),
        "tray close must keep the window alive"
    );
    store.set_close_action(zeo_player::config::CloseAction::Ask);
}
