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

    // 「热门艺术家」深链：点歌手圆钮直接落进「歌手」详情（复用 Artists）。
    // 首页长页面下滚一段，避开底部浮动播放条。
    let _ = app.scroll("text:热门艺术家", px(480.0));
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

/// 「最近播放」单行横排：窄窗放不下时标题行出翻页钮、条子沿横轴滚
/// 得动、到边的钮禁用；窗口拉宽全放下后钮消失。（语料只有几首
/// 歌、一两个歌手：「最近播放」一定溢出，「热门艺术家」没有播放
/// 记录时整节不渲染，也不会溢出，所以翻页钮恰有一对。）
#[test]
fn home_recent_shelf_scrolls_horizontally() {
    let mut app = start(760.0, 600.0);
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    app.wait_for_within("label:立即播放", Duration::from_secs(20))
        .unwrap();
    app.wait_for("最近播放").unwrap();
    // 卡按 200pt 排不下 → 翻页钮挂出来；起点处「往前翻」禁用。
    assert_eq!(
        app.screen().count("label:往后翻"),
        Ok(1),
        "{:?}",
        texts(&app)
    );
    assert_eq!(app.screen().enabled("label:往前翻"), Ok(false));
    // 条子本体沿横轴滚（视口若被内容撑到 max-content 宽，这里报错）。
    app.scroll("id:home-recent-shelf", px(300.0))
        .expect("卡条应能横向滚动");
    // 翻页钮一次滚近一屏：右翻后左翻解禁，回到起点再禁用。
    // 并行用例会经共享曲库 / 播放记录触发本节重建——按压若落在重建间隙
    // 的已卸载节点上就丢了（或按下时按钮暂时不存在）：按到解禁为止。
    let mut paged = false;
    for _ in 0..10 {
        let _ = app.press("label:往后翻");
        if app
            .wait_until(
                "右翻后「往前翻」应解禁",
                Duration::from_secs(2),
                |s| s.enabled("label:往前翻") == Ok(true),
            )
            .is_ok()
        {
            paged = true;
            break;
        }
    }
    assert!(paged, "右翻后「往前翻」应解禁: {:?}", texts(&app));
    // 回翻同样可能被重建间隙吃掉按压，照上法重试。
    let mut back = false;
    for _ in 0..10 {
        let _ = app.press("label:往前翻");
        if app
            .wait_until(
                "回翻后「往前翻」应禁用",
                Duration::from_secs(2),
                |s| s.enabled("label:往前翻") == Ok(false),
            )
            .is_ok()
        {
            back = true;
            break;
        }
    }
    assert!(back, "回翻后「往前翻」应禁用: {:?}", texts(&app));
    // 拉宽到全放下 → 翻页钮消失。
    app.set_viewport(size(px(2200.0), px(900.0))).unwrap();
    app.wait_until("放宽后翻页钮应收起", Duration::from_secs(5), |s| {
        s.count("label:往后翻") == Ok(0)
    })
    .unwrap();
}
