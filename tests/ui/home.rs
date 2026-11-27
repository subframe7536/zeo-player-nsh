use super::*;

/// 曲目行：双击才播放（单击不动）；喜欢列；右键菜单的删除项与「删除
/// 文件…」确认弹窗。
#[test]
fn track_row_double_click_like_and_menu() {
    let _guard = play_lock();
    let mut app = start_inner(1200.0, 800.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    goto_folders(&mut app);
    app.press("zeo-player-test-music").unwrap();
    app.wait_for("text:测试歌曲").unwrap();
    let row = "role:button[测试歌曲]";
    // 单击选中行但不播放，双击才播放。
    let was_playing = zeo_player::engine::Player::global().state().is_playing;
    app.press(row).unwrap();
    assert!(app.screen().selected(row).unwrap_or(false), "单击应选中");
    assert_eq!(
        zeo_player::engine::Player::global().state().is_playing,
        was_playing,
        "单击不应播放"
    );
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

    // 艺术家链接：行内歌手名单独可点，进艺术家详情而不触发行选中/播放。
    app.press("role:button[测试歌曲] >> role:button[测试歌手 A]")
        .unwrap();
    app.wait_for("label:返回").unwrap();
    assert!(
        app.screen().selected("role:link[艺术家]").unwrap_or(false),
        "点歌手链接应落在艺术家页: {:?}",
        texts(&app)
    );
}

/// 首页是默认页：侧栏「首页」选中；有语料时随机推荐（「随机翻到了」
/// 小歌单卡 + 「换一批」）与曲库 / 播放统计标签都在。没语料时是曲库为
/// 空的空状态。
#[test]
fn home_is_default_and_shows_stats() {
    let mut app = start(1200.0, 800.0);
    assert!(
        app.screen().selected("role:link[首页]").unwrap_or(false),
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
    goto_folders(&mut app);
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
    app.press("role:link[首页]").unwrap();
    app.wait_for("最近播放").unwrap();
    assert!(has(&app, "热门艺术家"), "{:?}", texts(&app));
    assert!(has(&app, "测试歌曲"), "{:?}", texts(&app));

    // 「热门艺术家」深链：点歌手卡直接落进「歌手」详情（复用 Artists）。
    // 多歌手已按分隔符拆开（「测试歌手 A; 测试歌手 B」→ A、B 两张卡），
    // 歌手名同时是行内 / 播放条的链接钮，限定在卡片网格里点。
    // 首页长页面下滚一段，避开底部浮动播放条。
    let _ = app.scroll("text:热门艺术家", px(480.0));
    app.press("id:home-artists-grid >> role:button[测试歌手 A]")
        .unwrap();
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

/// 「最近播放」「热门艺术家」是响应式网格：没有翻页钮，网格不撑出窗口，
/// 卡宽（封面边长）随窗口宽度换列数而变。（语料只有几首歌、一两个歌手，
/// 折行本身不一定触发，所以断言列宽与边界，不数行。）
#[test]
fn home_recent_grid_is_responsive() {
    let mut app = start(760.0, 600.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.wait_for_within("label:立即播放", Duration::from_secs(20))
        .unwrap();
    app.wait_for("最近播放").unwrap();
    assert_eq!(
        app.screen().count("label:往后翻"),
        Ok(0),
        "网格不再有翻页钮: {:?}",
        texts(&app)
    );
    // (网格右缘, 第一张封面边长)。
    let measure = |app: &Headless| {
        let grid = app
            .screen()
            .layout("id:home-recent-grid")
            .unwrap()
            .expect("最近播放网格应在页面里")
            .bounds;
        let cover = app
            .screen()
            .layout("id:home-recent-grid >> id:cover >> nth=0")
            .unwrap()
            .expect("网格里应有封面")
            .bounds;
        (grid.origin.x.0 + grid.size.width.0, cover.size.width.0)
    };
    let (right, narrow) = measure(&app);
    assert!(right <= 760.0, "网格不应撑出窗口: {right}");
    app.set_viewport(size(px(1600.0), px(900.0))).unwrap();
    app.wait_until("拉宽后封面应换列宽", Duration::from_secs(5), |s| {
        s.layout("id:home-recent-grid >> id:cover >> nth=0")
            .ok()
            .flatten()
            .is_some_and(|l| (l.bounds.size.width.0 - narrow).abs() > 4.0)
    })
    .unwrap();
    let (right, _) = measure(&app);
    assert!(right <= 1600.0, "拉宽后网格不应撑出窗口: {right}");
}
