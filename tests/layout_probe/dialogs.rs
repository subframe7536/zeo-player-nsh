use super::*;

#[test]
fn edit_dialog_layout_probe() {
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    let mut app = start(1200.0, 800.0);
    let probed = probe(&mut app);

    app.press("label:切换视图").unwrap();
    app.wait_for("role:option[文件夹]").unwrap();
    app.press("role:option[文件夹]").unwrap();
    app.wait_for_within("text:测试歌曲", Duration::from_secs(20))
        .unwrap();
    app.right_click("label:测试歌曲").unwrap();
    app.wait_for("role:menuitem[编辑元数据…]").unwrap();
    app.press("role:menuitem[编辑元数据…]").unwrap();
    app.wait_for("text:编辑元数据").unwrap();
    app.wait(Duration::from_millis(300)).unwrap();

    let p = probed.lock().unwrap();

    // 流派输入框（combobox 内部 input，placeholder「回车添加」）。
    let input_id = p
        .placeholders
        .iter()
        .find(|(_, ph)| ph.as_str() == "回车添加")
        .map(|(id, _)| *id)
        .expect("找不到流派输入框");
    let (ix, iy, _) = abs(&p, input_id).expect("输入框没有 frame");

    // 最后一个 chip 的右缘：× 按钮（label「移除流派」）右缘 + chip
    // 右内边距 4 ≈ chips 块右缘。
    let mut chip_right = 0.0f32;
    let mut chip_y = 0.0f32;
    for (id, label) in &p.labels {
        if label != "移除流派" {
            continue;
        }
        let (x, y, s) = abs(&p, *id).expect("× 按钮没有 frame");
        if x + s.width.0 > chip_right {
            chip_right = x + s.width.0;
            chip_y = y;
        }
    }
    assert!(chip_right > 0.0, "测试曲目应带流派 chip");
    let expect_x = chip_right + 4.0 + 6.0; // chip pr(4) + 根行 gap(6)
    assert!(
        (ix - expect_x).abs() <= 4.0,
        "输入框应紧跟最后一个 chip（期望 x≈{expect_x:.1}，实际 {ix:.1}）"
    );
    assert!(
        (iy - chip_y).abs() <= 8.0,
        "输入框应与 chips 同一行（iy={iy:.1}, chip_y={chip_y:.1}）"
    );

    // 底色（测试主题为浅色）：标题输入框 = card 白（不随封面染色）。
    let style_of = |label: &str| {
        p.labels
            .iter()
            .find(|(_, l)| l.as_str() == label)
            .and_then(|(id, _)| p.styles.get(id).copied())
            .unwrap_or_else(|| panic!("找不到 {label} 的样式"))
    };
    const WHITE: u32 = 0xFFFF_FFFF;
    let (input_bg, ..) = style_of("标题");
    assert_eq!(input_bg, WHITE, "输入框底色应为 card 白（不随封面染色）");

    // --- 文件卡片 ------------------------------------------------------------
    // 「更换封面…」按钮 → 操作行 → 信息列 → 卡片行；卡片行里除信息列
    // 外的孩子即封面框，96×96。语料有 sine.png 同名图片 → 默认「同名
    // 图片」目标，预览源就是同名文件本身。
    let btn_id = *p
        .labels
        .iter()
        .find(|(_, l)| l.as_str() == "更换封面…")
        .map(|(id, _)| id)
        .expect("找不到「更换封面…」按钮");
    let action_id = *p.parents.get(&btn_id).expect("按钮没有父节点");
    let col_id = *p.parents.get(&action_id).expect("操作行没有父节点");
    let card_id = *p.parents.get(&col_id).expect("信息列没有父节点");
    let cover_id = p
        .parents
        .iter()
        .filter(|(_, parent)| **parent == card_id)
        .map(|(id, _)| *id)
        .find(|id| *id != col_id)
        .expect("文件卡片里没有第二个孩子");
    assert_eq!(
        p.test_ids.get(&cover_id).map(String::as_str),
        Some("cover"),
        "卡片行的另一个孩子应是封面框"
    );
    let (_, _, cs) = abs(&p, cover_id).expect("封面框没有 frame");
    assert!(
        (cs.width.0 - 96.0).abs() <= 2.0 && (cs.height.0 - 96.0).abs() <= 2.0,
        "封面预览应为 96×96，实际 {:.0}×{:.0}",
        cs.width.0,
        cs.height.0
    );

    // 预览图源：封面框的后代 image 应直接取同名文件（sine.png）。
    let mut has_sidecar_img = false;
    for (id, src) in &p.images {
        let mut cur = *id;
        for _ in 0..8 {
            if cur == cover_id {
                has_sidecar_img |= src.contains("sine.png");
            }
            match p.parents.get(&cur) {
                Some(&parent) => cur = parent,
                None => break,
            }
        }
    }
    assert!(has_sidecar_img, "同名图片目标的预览应取同名文件 sine.png");

    // 「保存到」下拉存在且显示「同名图片」（同名文件已有时默认写它）；
    // 「删除封面」按钮也在卡片里。
    assert!(
        p.labels.values().any(|l| l == "封面保存位置"),
        "文件卡片应有「保存到」下拉"
    );
    assert!(
        p.texts.values().any(|t| t == "同名图片"),
        "已有同名图片时「保存到」应默认同名图片"
    );
    assert!(
        p.labels.values().any(|l| l == "删除封面"),
        "文件卡片应有「删除封面」按钮"
    );

    // 「保存到」弹层与 trigger 同宽：打开下拉，选项行宽 + 弹层内边距
    //（p(4)×2 = 8）≈ trigger 宽（110；旧行为下限 140 会差 ~30）。
    drop(p);
    app.press("label:封面保存位置").unwrap();
    app.wait_for("role:option[内嵌标签]").unwrap();
    app.wait(Duration::from_millis(150)).unwrap();
    let p = probed.lock().unwrap();
    let trig_w = p
        .labels
        .iter()
        .filter(|(_, l)| l.as_str() == "封面保存位置")
        .filter_map(|(id, _)| abs(&p, *id))
        .map(|(_, _, s)| s.width.0)
        .fold(0.0f32, f32::max);
    let item_w = p
        .labels
        .iter()
        .filter(|(_, l)| l.as_str() == "内嵌标签")
        .filter_map(|(id, _)| abs(&p, *id))
        .map(|(_, _, s)| s.width.0)
        .fold(0.0f32, f32::max);
    assert!(trig_w > 0.0 && item_w > 0.0, "trigger / 选项应有 frame");
    assert!(
        (item_w + 8.0 - trig_w).abs() <= 4.0,
        "弹层应与 trigger 同宽：选项 {item_w:.0}+8 vs trigger {trig_w:.0}"
    );
}

/// 欢迎页是整窗页面：根节点铺满视口；内容列水平居中、不超 640 宽；每一步的
/// 文字都在列内（「跳过」除外，它在右上角的标题栏一行里）、不压到标题栏
/// 一行；功能一览的六张卡片一行三张。
#[test]
fn welcome_page_layout_probe() {
    const W: f32 = 1200.0;
    const H: f32 = 800.0;
    const COLUMN: f32 = 640.0;
    isolate();
    let mut app = start(W, H);
    let probed = probe(&mut app);
    app.press("设置").unwrap();
    app.press("id:settings-nav >> role:button[关于]").unwrap();
    app.wait_for("id:settings-content >> role:button[重新查看]")
        .unwrap();
    app.press("id:settings-content >> role:button[重新查看]")
        .unwrap();
    app.wait_for("id:welcome >> text:欢迎使用 Zeo Player")
        .unwrap();

    // 页面铺满视口；该步的文字都在内容列里。
    let check = |app: &mut Headless, step: &str| {
        // 淡入（240ms）与指示点的过渡落定。
        app.wait(Duration::from_millis(500)).unwrap();
        let p = probed.lock().unwrap();
        let page = p
            .test_ids
            .iter()
            .find(|(_, t)| t.as_str() == "welcome")
            .map(|(id, _)| *id)
            .expect("找不到欢迎页");
        let (px_, py_, ps) = abs(&p, page).expect("欢迎页没有 frame");
        assert!(
            px_.abs() <= 0.5
                && py_.abs() <= 0.5
                && (ps.width.0 - W).abs() <= 0.5
                && (ps.height.0 - H).abs() <= 0.5,
            "{step}: 欢迎页应铺满视口：({px_:.1},{py_:.1},{:.1}x{:.1})",
            ps.width.0,
            ps.height.0
        );
        let col_left = (W - COLUMN) / 2.0;
        let mut checked = 0;
        for (id, text) in &p.texts {
            let mut cur = *id;
            let mut under = false;
            while let Some(parent) = p.parents.get(&cur) {
                if *parent == page {
                    under = true;
                    break;
                }
                cur = *parent;
            }
            let Some((x, y, s)) = abs(&p, *id).filter(|_| under) else {
                continue;
            };
            assert!(
                y >= 40.0 - 0.5 || text == "跳过",
                "{step}: 文字「{text}」压到标题栏一行（y={y:.1}）"
            );
            assert!(
                y + s.height.0 <= H + 0.5,
                "{step}: 文字「{text}」超出窗口底（y={y:.1} h={:.1}）",
                s.height.0
            );
            if text == "跳过" {
                // 右上角，贴窗口右缘（macOS 没有窗口按钮）。
                assert!(x > W - 120.0, "跳过应在右上角（x={x:.1}）");
                continue;
            }
            checked += 1;
            assert!(
                x >= col_left - 0.5 && x + s.width.0 <= col_left + COLUMN + 0.5,
                "{step}: 文字「{text}」越出内容列：x={x:.1} w={:.1}（列 {col_left:.0}..{:.0}）",
                s.width.0,
                col_left + COLUMN
            );
        }
        assert!(
            checked >= 3,
            "{step}: 页面里应探到文字节点（实际 {checked}）"
        );
    };

    check(&mut app, "欢迎");
    // 第 1 步：标题在内容列正中。
    {
        let p = probed.lock().unwrap();
        let (x, _, s) = p
            .texts
            .iter()
            .filter(|(_, t)| t.as_str() == "欢迎使用 Zeo Player")
            .find_map(|(id, _)| abs(&p, *id))
            .expect("找不到标题");
        assert!(
            (x + s.width.0 / 2.0 - W / 2.0).abs() <= 2.0,
            "标题应水平居中：中心 {:.1}",
            x + s.width.0 / 2.0
        );
    }
    app.press("id:welcome >> role:button[开始]").unwrap();
    app.wait_for("id:welcome >> text:添加音乐文件夹").unwrap();
    check(&mut app, "添加音乐文件夹");
    app.press("id:welcome >> role:button[下一步]").unwrap();
    app.wait_for("id:welcome >> role:radio[封面]").unwrap();
    check(&mut app, "主题色");
    app.press("id:welcome >> role:button[下一步]").unwrap();
    app.wait_for("id:welcome >> text:智能歌单").unwrap();
    check(&mut app, "功能一览");

    let p = probed.lock().unwrap();
    let title_pos = |title: &str| {
        p.texts
            .iter()
            .filter(|(_, t)| t.as_str() == title)
            .find_map(|(id, _)| abs(&p, *id))
            .map(|(x, y, _)| (x, y))
            .unwrap_or_else(|| panic!("找不到卡片 {title}"))
    };
    let titles = [
        "命令面板",
        "全屏播放与歌词",
        "元数据表与批量改名",
        "智能歌单",
        "十段均衡器与 ReplayGain",
        "托盘与全局快捷键",
    ];
    for row in titles.chunks(3) {
        let pos: Vec<_> = row.iter().map(|t| title_pos(t)).collect();
        for pair in pos.windows(2) {
            assert!(
                (pair[0].1 - pair[1].1).abs() <= 1.0 && pair[1].0 > pair[0].0 + 100.0,
                "{row:?} 应同一行并排：{pos:?}"
            );
        }
    }
    assert!(
        title_pos(titles[3]).1 > title_pos(titles[0]).1 + 40.0,
        "第二行应在第一行下方"
    );
}
