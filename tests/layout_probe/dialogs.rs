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
    // tab 的选中凸起画在列表的 indicator 上：tablist 里唯一无 label 的
    // 孩子，card 白底 + 1pt 边框，与选中 tab「基础」同 frame；tab 自身
    // 一律透明。
    let style_of = |label: &str| {
        p.labels
            .iter()
            .find(|(_, l)| l.as_str() == label)
            .and_then(|(id, _)| p.styles.get(id).copied())
            .unwrap_or_else(|| panic!("找不到 {label} 的样式"))
    };
    let tab_of = |p: &Probed, label: &str| {
        p.labels
            .iter()
            .find(|(_, l)| l.as_str() == label)
            .map(|(id, _)| *id)
            .unwrap_or_else(|| panic!("找不到 {label}"))
    };
    const WHITE: u32 = 0xFFFF_FFFF;
    let (input_bg, ..) = style_of("标题");
    assert_eq!(input_bg, WHITE, "输入框底色应为 card 白（不随封面染色）");
    let list_id = *p.parents.get(&tab_of(&p, "基础")).expect("tab 没有父节点");
    let indicator_id = p
        .parents
        .iter()
        .filter(|(_, parent)| **parent == list_id)
        .map(|(id, _)| *id)
        .find(|id| !p.labels.contains_key(id))
        .expect("tablist 里没有 indicator");
    let (ind_bg, ind_border, ind_bw) = p
        .styles
        .get(&indicator_id)
        .copied()
        .expect("indicator 没有样式");
    assert_eq!(ind_bg, WHITE, "indicator 应为 card 白");
    assert!(
        ind_bw >= 1.0 && ind_border != 0,
        "indicator 应有可见边框（bw={ind_bw}, color={ind_border:#x}）"
    );
    let same_frame = |p: &Probed, a: NodeId, b: NodeId| {
        let (ax, ay, asz) = abs(p, a).expect("没有 frame");
        let (bx, by, bsz) = abs(p, b).expect("没有 frame");
        (ax - bx).abs() <= 1.0
            && (ay - by).abs() <= 1.0
            && (asz.width.0 - bsz.width.0).abs() <= 1.0
            && (asz.height.0 - bsz.height.0).abs() <= 1.0
    };
    assert!(
        same_frame(&p, indicator_id, tab_of(&p, "基础")),
        "indicator 应与选中 tab「基础」同 frame"
    );
    let (unsel_bg, ..) = style_of("封面");
    assert_eq!(unsel_bg, 0, "未选中 tab 应透明");

    // --- 封面栏 ------------------------------------------------------------
    // 「保存到」+ 提示在顶部；预览 192×192 的（封面 + 按钮列）内容块在
    // 剩余高度里垂直居中、水平居中。语料有 sine.png 同名图片 → 默认
    // 「同名图片」目标，预览源就是同名文件本身。
    drop(p);
    app.press("label:封面").unwrap();
    app.wait_for("label:更换图片…").unwrap();
    // indicator 的跟随弹簧约 362ms，等它落定。
    app.wait(Duration::from_millis(500)).unwrap();
    let p = probed.lock().unwrap();

    // indicator 跟到新选中的「封面」，仍与同 frame。
    assert!(
        same_frame(&p, indicator_id, tab_of(&p, "封面")),
        "indicator 应跟到选中 tab「封面」"
    );

    // 「保存到」下拉存在且显示「同名图片」（同名文件已有时默认写它）。
    assert!(
        p.labels.values().any(|l| l == "封面保存位置"),
        "封面栏应有「保存到」下拉"
    );
    assert!(
        p.texts.values().any(|t| t == "同名图片"),
        "已有同名图片时「保存到」应默认同名图片"
    );

    // 「更换图片…」按钮 → 按钮列 → 行；行里除按钮列外的孩子即封面框。
    let btn_id = *p
        .labels
        .iter()
        .find(|(_, l)| l.as_str() == "更换图片…")
        .map(|(id, _)| id)
        .expect("找不到「更换图片…」按钮");
    let btn_col = *p.parents.get(&btn_id).expect("按钮没有父节点");
    let row_id = *p.parents.get(&btn_col).expect("按钮列没有父节点");
    let cover_id = p
        .parents
        .iter()
        .filter(|(_, parent)| **parent == row_id)
        .map(|(id, _)| *id)
        .find(|id| *id != btn_col)
        .expect("封面行里没有第二个孩子");
    let (_, cy, cs) = abs(&p, cover_id).expect("封面框没有 frame");
    assert!(
        (cs.width.0 - 192.0).abs() <= 2.0 && (cs.height.0 - 192.0).abs() <= 2.0,
        "封面预览应为 192×192，实际 {:.0}×{:.0}",
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

    // 祖先链上第一个带 ScrollContent 的节点 = 448 视口。
    let mut scroll_vp = None;
    let mut cur = row_id;
    for _ in 0..16 {
        if p.scrolls.contains_key(&cur) {
            scroll_vp = p.frames.get(&cur).map(|f| {
                let (x, y, _) = abs(&p, cur).unwrap_or_default();
                (x, y, f.size)
            });
            break;
        }
        match p.parents.get(&cur) {
            Some(&parent) => cur = parent,
            None => break,
        }
    }
    let (vx, vy, vs) = scroll_vp.expect("封面栏不在 scroll 里");

    // 顶部控件底缘 = 「保存到」下拉与各提示行里最低的那个。
    let mut top_ref = 0.0f32;
    if let Some((_, y, s)) = p
        .labels
        .iter()
        .find(|(_, l)| l.as_str() == "封面保存位置")
        .and_then(|(id, _)| abs(&p, *id))
    {
        top_ref = top_ref.max(y + s.height.0);
    }
    for (id, t) in &p.texts {
        let is_hint = [
            "保存到同名",
            "保存时新建",
            "同名图片在",
            "检测到同名",
            "保存后替换",
        ]
        .iter()
        .any(|pre| t.starts_with(pre));
        if is_hint && let Some((_, y, s)) = abs(&p, *id) {
            top_ref = top_ref.max(y + s.height.0);
        }
    }
    assert!(top_ref > 0.0, "封面栏顶部应有「保存到」控件");
    let top_gap = cy - top_ref;
    let bottom_gap = (vy + vs.height.0) - (cy + cs.height.0);
    assert!(
        (top_gap - bottom_gap).abs() <= 28.0,
        "封面内容块应在剩余高度居中：上 {top_gap:.0}，下 {bottom_gap:.0}"
    );

    // 水平方向也居中：行（封面 + 按钮列）的中心 ≈ 视口中心。
    let (rx, _, rs) = abs(&p, row_id).expect("封面行没有 frame");
    assert!(
        (rx + rs.width.0 / 2.0 - (vx + vs.width.0 / 2.0)).abs() <= 24.0,
        "封面内容块应水平居中：行中心 {:.0}，视口中心 {:.0}",
        rx + rs.width.0 / 2.0,
        vx + vs.width.0 / 2.0
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
