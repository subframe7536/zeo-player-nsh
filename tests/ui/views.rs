use super::*;

/// 侧栏下半部可切换视图（歌单 / 标签 / 文件夹）与主视图联动：
/// 1. 默认展示歌单；
/// 2. 主视图切至「文件夹」，侧栏自动联动为文件树；
/// 3. 主视图切至「标签」，侧栏自动联动为标签列表，默认选中首个标签直达曲目；
/// 4. 侧栏下拉菜单可手动切换视图。
#[test]
fn sidebar_switchable_views_and_auto_linkage() {
    let mut app = start(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    if app
        .wait_for_within("label:我喜欢", Duration::from_millis(50))
        .is_err()
    {
        app.press("label:切换视图").unwrap();
        app.wait_for("role:option[歌单]").unwrap();
        app.press("role:option[歌单]").unwrap();
    }
    app.wait_for("label:我喜欢").unwrap();

    // 1. 手动切换：通过下拉切到「标签」
    app.press("label:切换视图").unwrap();
    app.wait_for("role:option[标签]").unwrap();
    app.press("role:option[标签]").unwrap();
    app.wait_for("text:Rock").unwrap();

    // 2. 切到「文件夹」：主视图联动为文件夹页，侧栏联动为文件树
    app.press("label:切换视图").unwrap();
    app.wait_for("role:option[文件夹]").unwrap();
    app.press("role:option[文件夹]").unwrap();
    let root = root_name();
    app.wait_for_within(&format!("label:展开 {root}"), Duration::from_secs(20))
        .unwrap();
    app.wait_for("label:切换视图 >> text:文件夹").unwrap();
    app.wait_for("text:测试歌曲").unwrap();

    // 3. 切到「标签」：主视图联动为标签页，侧栏联动为标签列表
    app.press("label:切换视图").unwrap();
    app.wait_for("role:option[标签]").unwrap();
    app.press("role:option[标签]").unwrap();
    app.wait_for("label:切换视图 >> text:标签").unwrap();
    app.wait_for("text:/^# /").unwrap();
    app.wait_for("text:测试歌曲").unwrap();
}

/// 右键菜单互斥：再开一个菜单时旧的关掉（上游 0.5.0 起次级点击纳入
/// PressOutside，菜单互斥由平台接管）；点到没有菜单的地方，开着的菜单
/// 也要关掉。
#[test]
fn context_menus_replace_each_other() {
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

    // 点在没有菜单的地方（页头）：PressOutside 把菜单关掉。真机上右键
    // 点空处同走 PressOutside（上游 #1544），但无头驱动的 right_click
    // 只发 ContextMenu + MouseUp、没有次级按压，这里用主按压覆盖同一条
    // 关闭路径。
    app.press("text:文件夹 >> nth=0").unwrap();
    app.wait_for_gone("role:menuitem[重命名文件…]", Duration::from_secs(2))
        .unwrap();
    assert_eq!(app.screen().count("role:menu").unwrap(), 0);
}

/// 「编辑元数据…」：曲目行右键菜单打开弹窗，「基础元数据」栏各输入框
/// 预填当前标签值，流派渲染成标签 chip；切「封面」「歌词」栏各见其控件。
/// 只开不保存（不动共享语料）。
#[test]
fn edit_info_dialog_opens_with_current_tags() {
    let mut app = start(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    goto_folders(&mut app);
    app.wait_for_within("text:测试歌曲", Duration::from_secs(20))
        .unwrap();
    app.right_click("label:测试歌曲").unwrap();
    app.wait_for("role:menuitem[编辑元数据…]").unwrap();
    app.press("role:menuitem[编辑元数据…]").unwrap();
    // 弹窗打开：标题行与预填的标签值（语料 sine.flac 的已知标签）。
    app.wait_for("text:编辑元数据").unwrap();
    for (label, expect) in [
        ("标题", "测试歌曲"),
        ("歌手", "测试歌手 A; 测试歌手 B"),
        ("专辑", "测试专辑"),
        ("音轨号", "1"),
    ] {
        assert_eq!(
            app.screen().input_text(&format!("label:{label}")).unwrap(),
            Some(expect),
            "{label} 应预填当前标签值"
        );
    }
    // 流派是标签输入器：语料的三个流派各成一个 chip。
    app.wait_for("text:标签").unwrap();
    app.wait_for("text:流派：Rock; Indie; Dream Pop").unwrap();
    for chip in ["Rock", "Indie", "Dream Pop"] {
        assert!(
            app.screen().count(&format!("text:{chip}")).unwrap() >= 1,
            "流派 chip {chip} 应预填"
        );
    }
    // 下拉：输入即过滤，首项「添加“…”」自由新增，回车收下成一个 chip。
    app.type_text("label:标签", "r").unwrap();
    app.wait_for("role:option").unwrap();
    app.wait_for("text:添加“r”").unwrap();
    assert!(app.screen().count("role:option").unwrap() >= 2);
    // 回车走 keymap（listbox 的 enter 绑定收高亮项）；`submit` 只发输入框
    // 级 KeyPress 事件、不过 keymap，模拟不了它。
    app.press_key("enter").unwrap();
    app.wait_for("text:r").unwrap();
    app.wait_for("text:流派：Rock; Indie; Dream Pop; r")
        .unwrap();
    assert_eq!(
        app.screen().input_text("label:标签").unwrap(),
        Some(""),
        "新增后查询应清空"
    );
    // 「封面」栏有更换按钮；「歌词」栏默认写同名 .lrc（语料有
    // sine.lrc），多行输入预填文件内容。
    app.press("label:封面").unwrap();
    app.wait_for("label:更换图片…").unwrap();
    app.press("label:歌词").unwrap();
    app.wait_for("text:保存到").unwrap();
    let lyrics = app.screen().input_text("label:歌词").unwrap();
    assert!(
        lyrics.is_some_and(|t| t.contains("第一句歌词")),
        "歌词应预填 .lrc 内容，得到 {lyrics:?}"
    );
    app.press("label:取消").unwrap();
    app.wait_for_gone("text:编辑元数据", Duration::from_secs(2))
        .unwrap();
}

#[test]
fn narrow_windows_drop_album_and_size_columns() {
    let mut app = start(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    goto_folders(&mut app);
    app.wait_for_within("大小", Duration::from_secs(20))
        .unwrap();
    // 展开时「专辑」至少有两处：侧栏导航行与曲目表的列头。
    let expanded = app.screen().count("text:专辑").unwrap_or(0);
    assert!(expanded >= 2, "{expanded}");

    // 窗口收窄到 640 pt（低于运行时的最小宽，仅无头测试可达），内容区
    // 408 pt：大小与专辑列收起——「专辑」只剩侧栏导航一处。
    app.set_viewport(size(px(640.0), px(800.0))).unwrap();
    assert!(!has(&app, "大小"));
    assert!(
        app.screen().count("text:专辑").unwrap_or(0) < expanded,
        "专辑列应收起（只剩侧栏导航）"
    );
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
    goto_folders(&mut app);
    // 等扫描完（根目录出现即曲库就绪）。
    let root = root_name();
    app.wait_for_within(&format!("label:展开 {root}"), Duration::from_secs(20))
        .unwrap();
    // 文件夹页：没有 `#` 列头。
    assert!(!has(&app, "#"), "{:?}", texts(&app));
    // 艺术家详情：也没有。
    app.press("label:艺术家").unwrap();
    app.wait_for("role:button[测试歌手 A]").unwrap();
    app.press("role:button[测试歌手 A]").unwrap();
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

/// 侧栏搜索按钮：点击直接打开 command palette，支持搜索曲目与页面导航。
#[test]
fn sidebar_search_opens_command_palette() {
    let mut app = start(1200.0, 800.0);
    app.press("label:搜索").unwrap();
    app.wait_for("label:命令面板").unwrap();
    let t = texts(&app);
    assert!(t.iter().any(|s| s == "页面" || s == "首页"), "{t:?}");
    // 搜索曲目
    app.type_text("label:命令面板", "short").unwrap();
    app.wait(Duration::from_millis(50)).unwrap();
    let t_after = texts(&app);
    assert!(t_after.iter().any(|s| s == "short"), "{t_after:?}");

    // 选择曲目播放并关闭面板
    app.press("short.flac").unwrap();
    app.wait_for_gone("label:命令面板", Duration::from_secs(2))
        .unwrap();

    // 再次点击打开，搜索并导航至元数据页面
    app.press("label:搜索").unwrap();
    app.wait_for("label:命令面板").unwrap();
    app.type_text("label:命令面板", "元数据").unwrap();
    app.wait(Duration::from_millis(50)).unwrap();
    app.press("role:option[元数据]").unwrap();
    app.wait_for_gone("label:命令面板", Duration::from_secs(2))
        .unwrap();
    app.wait_for("label:列显示设置").unwrap();
}

#[test]
fn sidebar_no_track_count() {
    let mut app = start(1200.0, 800.0);
    // 侧栏底部不再有 "首曲目" 文本
    let t = texts(&app);
    assert!(
        !t.iter().any(|s| s.contains("首曲目")),
        "侧栏底部不应再出现曲目总数统计: {t:?}"
    );

    // 能看到「曲库」小标题
    assert!(has(&app, "曲库"), "应有「曲库」小标题");

    // 搜索按钮可以点击
    app.press("label:搜索").unwrap();
    app.wait_for("label:命令面板").unwrap();
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
/// 实时匹配数；删除前确认。（歌单文件在共享的数据目录里，folders 用例
/// 也写它——持锁与它会合，避免整表保存互相覆盖。）
#[test]
fn playlists_manual_and_smart() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    let corpus = isolate();
    assert!(has(&app, "歌单"), "{:?}", texts(&app));

    if app
        .wait_for_within("label:添加歌单", Duration::from_millis(50))
        .is_err()
    {
        app.press("label:切换视图").unwrap();
        app.wait_for("role:option[歌单]").unwrap();
        app.press("role:option[歌单]").unwrap();
    }
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
        goto_folders(&mut app);
        app.wait_for_within("大小", Duration::from_secs(20))
            .unwrap();
        app.press(&format!("text:{}", root_name())).unwrap();
        app.wait_for_within("label:测试歌曲", Duration::from_secs(5))
            .unwrap();
        app.right_click("label:测试歌曲").unwrap();
        app.press("label:添加到歌单").unwrap();
        app.press("role:menuitem[测试歌单]").unwrap();
        app.press("label:切换视图").unwrap();
        app.press("role:option[歌单]").unwrap();
        app.press("role:link[测试歌单]").unwrap();
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
        app.press("role:option[标签]").unwrap();
        app.press("label:值").unwrap();
        app.press("role:option[Indie]").unwrap();
        app.wait_for("匹配 1 首").unwrap();
        app.press("label:字段").unwrap();
        app.press("role:option[歌手]").unwrap();
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
