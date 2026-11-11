use super::*;

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
    goto_folders(&mut app);
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
    assert_eq!(
        app.screen().count(bar_img).unwrap(),
        1,
        "播放条封面应一直显示"
    );
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
    // combobox：在输入框里输入即开并过滤（选中名先被整词替换）。
    app.type_text("id:settings-content >> label:字体", "系统")
        .unwrap();
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
    goto_folders(&mut app);
    let root = root_name();
    // 默认不打开：根文件夹处于折叠状态，子文件夹 sub 初始不显示
    app.wait_for_within(&format!("label:展开 {root}"), Duration::from_secs(20))
        .unwrap();
    assert!(!texts(&app).iter().any(|t| t == "sub"));

    // 点击展开根文件夹，子文件夹 sub 出现
    app.press(&format!("label:展开 {root}")).unwrap();
    app.wait_for("text:sub").unwrap();

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

    // 头部字段筛选功能（点击「筛选」后展开，布局与智能歌单一致；按钮变为「清除筛选」）
    app.wait_for("text:筛选").unwrap();
    assert!(!texts(&app).contains(&"符合以下".to_string()));
    app.press("筛选").unwrap();
    app.wait_for("text:符合以下").unwrap();
    app.wait_for("text:匹配 2 首").unwrap();
    app.wait_for("text:清除筛选").unwrap();

    app.press("label:字段").unwrap();
    app.press("role:option[文件名]").unwrap();
    app.type_text("label:值", "mono").unwrap();
    app.wait_for("text:mono").unwrap();
    app.wait_for("text:匹配 1 首").unwrap();
    assert!(!texts(&app).contains(&"short".to_string()));

    // 单规则删除按钮：不删除行，重置到默认状态（字段变回歌手，值清空，匹配数恢复为 2）
    app.press("label:删除条件").unwrap();
    app.wait_for("text:short").unwrap();
    app.wait_for("text:匹配 2 首").unwrap();
    app.wait_for("text:歌手").unwrap();

    app.type_text("label:值", "nonexistent").unwrap();
    app.wait_for("text:没有符合条件的曲目").unwrap();
    app.wait_for("text:匹配 0 首").unwrap();

    // 点击「清除筛选」：重置过滤条件并收起筛选面板，按钮变回「筛选」
    app.press("清除筛选").unwrap();
    app.wait_for("text:short").unwrap();
    app.wait_for("text:筛选").unwrap();
    assert!(!texts(&app).contains(&"符合以下".to_string()));

    // 重新开启筛选，测试添加条件与模式切换
    app.press("筛选").unwrap();
    app.wait_for("text:符合以下").unwrap();
    app.press("添加条件").unwrap();
    app.press("任一").unwrap();
    app.press("全部").unwrap();
    // 多条规则时点击删除按钮：删除对应行
    app.press("label:删除条件 >> nth=0").unwrap();
    app.press("清除筛选").unwrap();
    app.wait_for("text:筛选").unwrap();
}

/// 文件夹页：列表支持单选与多选（Cmd/Ctrl+单击切换、Shift+单击范围选择）。
#[test]
fn folders_page_selection_and_multi_selection() {
    // 也改共享的 playlists.json：与 playlists_manual_and_smart 互相覆盖
    // （各自存整表），并行跑会丢另一方的写入——全程持锁串行。
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    goto_folders(&mut app);
    let root = root_name();
    app.wait_for_within(&format!("label:展开 {root}"), Duration::from_secs(20))
        .unwrap();
    app.press(&format!("label:展开 {root}")).unwrap();
    app.wait_for("text:sub").unwrap();

    // 切换到 sub 目录（内含 mono 与 short 两个曲目）
    app.press("sub").unwrap();
    app.wait_for("text:mono").unwrap();
    app.wait_for("text:short").unwrap();

    let mono = "role:button[mono]";
    let short = "role:button[short]";

    // 初始状态：未选中任何行
    assert_eq!(app.screen().selected(mono).unwrap(), false);
    assert_eq!(app.screen().selected(short).unwrap(), false);

    // 单击 mono：单选 mono
    app.press(mono).unwrap();
    assert_eq!(app.screen().selected(mono).unwrap(), true);
    assert_eq!(app.screen().selected(short).unwrap(), false);
    app.wait_for("text:已选 1 首（共 2 首）").unwrap();

    // 单击 short：单选 short（mono 取消选中）
    app.press(short).unwrap();
    assert_eq!(app.screen().selected(mono).unwrap(), false);
    assert_eq!(app.screen().selected(short).unwrap(), true);
    app.wait_for("text:已选 1 首（共 2 首）").unwrap();

    // Cmd / Ctrl + 单击 mono：多选（同时选中 mono 和 short）
    app.hold_keys("mod", true).unwrap();
    app.press(mono).unwrap();
    app.hold_keys("mod", false).unwrap();
    assert_eq!(app.screen().selected(mono).unwrap(), true);
    assert_eq!(app.screen().selected(short).unwrap(), true);
    app.wait_for("text:已选 2 首（共 2 首）").unwrap();

    // Cmd / Ctrl + 单击 mono：取消选中 mono，保留 short
    app.hold_keys("mod", true).unwrap();
    app.press(mono).unwrap();
    app.hold_keys("mod", false).unwrap();
    assert_eq!(app.screen().selected(mono).unwrap(), false);
    assert_eq!(app.screen().selected(short).unwrap(), true);
    app.wait_for("text:已选 1 首（共 2 首）").unwrap();

    // 单击 mono（设为锚点），然后 Shift + 单击 short：范围选择
    app.press(mono).unwrap();
    assert_eq!(app.screen().selected(mono).unwrap(), true);
    assert_eq!(app.screen().selected(short).unwrap(), false);
    app.hold_keys("shift", true).unwrap();
    app.press(short).unwrap();
    app.hold_keys("shift", false).unwrap();
    assert_eq!(app.screen().selected(mono).unwrap(), true);
    assert_eq!(app.screen().selected(short).unwrap(), true);
    app.wait_for("text:已选 2 首（共 2 首）").unwrap();

    // 多选状态下右键点击其中一项：保持多选，菜单精简（隐藏单文件专有项）
    app.right_click("label:mono").unwrap();
    app.wait_for("role:menuitem[播放]").unwrap();
    app.wait_for("label:添加到歌单").unwrap();
    app.wait_for("role:menuitem[从曲库中删除]").unwrap();
    app.wait_for("role:menuitem[删除文件…]").unwrap();
    assert!(
        !texts(&app).iter().any(|t| t == "编辑元数据…"),
        "多选时右键菜单不应出现「编辑元数据…」"
    );
    assert!(
        !texts(&app).iter().any(|t| t == "重命名文件…"),
        "多选时右键菜单不应出现「重命名文件…」"
    );
    assert!(
        !texts(&app)
            .iter()
            .any(|t| t.starts_with("在访达中显示") || t.starts_with("在资源管理器中显示")),
        "多选时右键菜单不应出现显示文件项"
    );
    app.press("label:添加到歌单").unwrap();
    app.press("role:menuitem[我喜欢]").unwrap();
    let pl_file =
        std::path::Path::new(&std::env::var("ZEO_PLAYER_DATA_DIR").unwrap()).join("playlists.json");
    let mut has_both = false;
    for _ in 0..40 {
        has_both = std::fs::read_to_string(&pl_file)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["playlists"].as_array().cloned())
            .and_then(|arr| arr.into_iter().find(|p| p["favorite"] == true))
            .and_then(|fav| fav["tracks"].as_array().cloned())
            .is_some_and(|tracks| {
                tracks
                    .iter()
                    .any(|t| t.as_str().is_some_and(|p| p.contains("mono")))
                    && tracks
                        .iter()
                        .any(|t| t.as_str().is_some_and(|p| p.contains("short")))
            });
        if has_both {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(has_both, "多选添加到歌单应将选中的全部曲目加入歌单");

    // 切换文件夹时清空选择状态
    app.press(&root_name()).unwrap();
    app.wait_for("text:测试歌曲").unwrap();
    assert_eq!(
        app.screen().selected("role:button[测试歌曲]").unwrap(),
        false
    );

    // 单文件右键菜单包含单文件项
    app.right_click("label:测试歌曲").unwrap();
    app.wait_for("role:menuitem[编辑元数据…]").unwrap();
    app.wait_for("role:menuitem[重命名文件…]").unwrap();
    assert!(
        texts(&app)
            .iter()
            .any(|t| t.starts_with("在访达中显示") || t.starts_with("在资源管理器中显示")),
        "单选时右键菜单应出现显示文件项"
    );
    app.press("role:menuitem[播放]").unwrap();

    zeo_player::engine::Player::global().pause();
}

/// 侧栏下半部：切换视图（歌单/标签/文件夹）选择项保存在全局状态。
#[test]
fn sidebar_section_select_switches_and_persists() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    let file =
        std::path::Path::new(&std::env::var("ZEO_PLAYER_DATA_DIR").unwrap()).join("library.json");
    let saved = |section: &str| {
        std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some_and(|v| v["sidebar_section"] == section)
    };
    // 打开切换视图下拉
    app.press("label:切换视图").unwrap();
    app.wait_for("role:option[标签]").unwrap();
    app.wait_for("role:option[文件夹]").unwrap();
    // 切换到文件夹
    app.press("role:option[文件夹]").unwrap();
    app.wait_for("label:切换视图 >> text:文件夹").unwrap();
    for _ in 0..40 {
        if saved("folders") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(saved("folders"));

    // 换回歌单（避免影响后续测试环境）
    app.press("label:切换视图").unwrap();
    app.wait_for("role:option[歌单]").unwrap();
    app.press("role:option[歌单]").unwrap();
    app.wait_for("label:切换视图 >> text:歌单").unwrap();
    for _ in 0..40 {
        if saved("playlists") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(saved("playlists"));
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
    goto_folders(&mut app);
    let root = root_name();
    app.wait_for_within(&format!("label:展开 {root}"), Duration::from_secs(20))
        .unwrap();
    app.press(&format!("label:展开 {root}")).unwrap();
    app.wait_for("text:sub").unwrap();
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
