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

/// 「编辑元数据…」：曲目行右键菜单打开弹窗，单个滚动表单的各输入框
/// 预填当前标签值，流派渲染成标签 chip；文件卡片有封面操作与「保存
/// 到」目标，歌词区预填 .lrc 内容。只开不保存（不动共享语料）。
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
        ("艺术家", "测试歌手 A; 测试歌手 B"),
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
    // 新增后查询清空：「添加“r”」只在查询为 r 时存在，清掉就从下拉
    // 里消失（nsh-testing 的 input_text 现在优先读无障碍值——combobox
    // 报的是已选标题串，读不到输入框原文，见 upstream.md「测试驱动」）。
    app.wait_for_gone("text:添加“r”", Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        app.screen().input_text("label:标签").unwrap(),
        Some("Rock, Indie, Dream Pop"),
        "combobox 的无障碍值应报已选标题（新增的 r 不在 items 里被略过）"
    );
    // 文件卡片：更换 / 删除封面按钮与「保存到」目标下拉都在（语料有
    // sine.png 同名图片，默认写它）；歌词默认写同名 .lrc（语料有
    // sine.lrc），多行输入预填文件内容——都在同一个滚动表单里，不用切栏。
    app.wait_for("label:更换封面…").unwrap();
    app.wait_for("label:删除封面").unwrap();
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

/// 序号列（`#`）：音乐库与艺术家详情显示列表位置，专辑详情显示标签
/// 曲目号；文件夹页的曲目表没有这一列。
#[test]
fn number_column_shows_position_or_track_number() {
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
    // 艺术家详情：有 `#` 列头（行内是列表位置，同音乐库）。
    app.press("label:艺术家").unwrap();
    // 歌手名同时是行内 / 播放条的链接钮，限定在艺术家网格里点卡片。
    app.wait_for("id:artist-grid >> role:button[测试歌手 A]")
        .unwrap();
    app.press("id:artist-grid >> role:button[测试歌手 A]")
        .unwrap();
    app.wait_for("label:返回").unwrap();
    assert!(has(&app, "#"), "歌手详情: {:?}", texts(&app));
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

/// 歌词手动滚动后：鼠标还在歌词区内不恢复跟随；移出 3 秒后滚回高亮行。
#[test]
fn lyrics_resume_follow_after_mouse_leaves() {
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
    app.wait_for("text:第一句歌词").unwrap();
    // 停在「第一句歌词」（1s；0.5s 的提前显示也不会到 5s 的下一行），
    // 让高亮行在测试期间保持不变。
    zeo_player::engine::Player::global().pause();
    zeo_player::engine::Player::global().seek(1.2);
    app.wait(Duration::from_millis(800)).unwrap();

    let line_y = |app: &Headless| {
        app.screen()
            .layout("text:第一句歌词")
            .unwrap()
            .map(|l| l.bounds.origin.y.0)
            .expect("第一句歌词应在歌词区")
    };
    let anchored_y = line_y(&app);
    app.hover("text:第一句歌词").unwrap();
    app.scroll("text:第一句歌词", px(220.0)).unwrap();
    let scrolled_y = line_y(&app);
    assert!(
        scrolled_y < anchored_y - 100.0,
        "手动滚动应离开高亮行: {anchored_y} -> {scrolled_y}"
    );

    // 鼠标还在歌词区：超过 3 秒也不自动回去。
    app.wait(Duration::from_millis(3200)).unwrap();
    let hovered_y = line_y(&app);
    assert!(
        hovered_y < anchored_y - 100.0,
        "鼠标未移出时不应恢复跟随: {anchored_y} -> {hovered_y}"
    );

    // 移出歌词区后 3 秒计时，滚动动画约 0.3 秒。
    app.hover("label:收起").unwrap();
    app.wait(Duration::from_millis(3100)).unwrap();
    app.wait_until("歌词滚回高亮行", Duration::from_secs(2), |screen| {
        screen
            .layout("text:第一句歌词")
            .ok()
            .flatten()
            .is_some_and(|l| (l.bounds.origin.y.0 - anchored_y).abs() < 30.0)
    })
    .unwrap();
    zeo_player::engine::Player::global().pause();
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

/// 音乐库：列表位置 `#` 列 + 大小列；序号格常是可按的播放钮（悬停显
/// 播放 / 暂停图标，单击直接播、不占行的双击窗口）；专辑名链接进专
/// 辑详情；正在播放的曲目滚出视野时右下角浮出「定位到当前播放」，
/// 点它滚回当前行（语料里「测试歌曲」按歌手排序排最后，窗口收矮后
/// 顶部视野里没有它，走的是真路径）。
#[test]
fn library_row_play_album_link_and_locate() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.press("音乐库").unwrap();
    app.wait_for_within("label:测试歌曲", Duration::from_secs(20))
        .unwrap();
    // 列头：列表位置的 `#` 与「大小」。
    assert!(has(&app, "#"), "{:?}", texts(&app));
    assert!(has(&app, "大小"), "{:?}", texts(&app));

    // 序号格：悬停行后按「播放…」直接开播。
    app.hover("label:测试歌曲").unwrap();
    app.press("label:播放「测试歌曲」").unwrap();
    app.wait_until("测试歌曲开播", Duration::from_secs(10), |_| {
        let s = zeo_player::engine::Player::global().state();
        s.is_playing && s.track.title == "测试歌曲"
    })
    .unwrap();
    // 开播后同一格标签换成「暂停…」（当前曲目行）。随后暂停住共享播放
    // 器：语料单曲只有几秒，一路播下去当前曲目会顺到别首，定位钮断
    // 言就花了。
    app.wait_for("label:暂停「测试歌曲」").unwrap();
    zeo_player::engine::Player::global().pause();

    // 专辑名链接进专辑详情（「播放全部」是音乐库页头特有）。
    app.press("role:button[测试专辑]").unwrap();
    app.wait_for("label:返回").unwrap();
    assert!(has(&app, "测试专辑"), "{:?}", texts(&app));
    assert!(!has(&app, "播放全部"), "{:?}", texts(&app));
    app.press("label:返回").unwrap();
    app.press("音乐库").unwrap();
    app.wait_for("label:测试歌曲").unwrap();

    // 当前行在视野内：定位钮不显示。
    app.wait_for_gone("label:定位到当前播放", Duration::from_secs(2))
        .unwrap();
    // 「测试歌曲」排序在最后：窗口收矮后它在视野区间之外（虚拟列表的
    // 预载行可能还挂着，断言走 viewable 区间判定的按钮本身，不造假）。
    app.set_viewport(size(px(1200.0), px(300.0))).unwrap();
    if app
        .wait_for_within("label:定位到当前播放", Duration::from_secs(2))
        .is_ok()
    {
        // 行滚出视野 → 浮钮出现；点它把当前行滚回视口中央（暂停后该
        // 格标签是「播放…」）。
        app.press("label:定位到当前播放").unwrap();
        app.wait_for_gone("label:定位到当前播放", Duration::from_secs(2))
            .unwrap();
        app.wait_for("label:播放「测试歌曲」").unwrap();
    } else {
        // 语料太短、行仍在视野内：按钮应该一直不显示（不造假离屏）。
        app.wait_for_gone("label:定位到当前播放", Duration::from_secs(2))
            .unwrap();
    }
    zeo_player::engine::Player::global().pause();
}
