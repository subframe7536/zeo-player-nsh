use super::*;

#[test]
fn smart_playlist_select_probe() {
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    let mut app = start(1200.0, 800.0);
    let probed = probe(&mut app);

    // 「新建智能歌单」对话框。
    app.press("label:添加歌单").unwrap();
    app.press("role:menuitem[新建智能歌单]").unwrap();
    app.wait_for("label:歌单名称").unwrap();
    app.wait(Duration::from_millis(200)).unwrap();

    // 字段换成「标签」：值框变成标签选择器（tag_select）。
    app.press("label:字段").unwrap();
    app.wait_for("role:option[标签]").unwrap();
    app.press("role:option[标签]").unwrap();
    app.wait(Duration::from_millis(200)).unwrap();

    // 值 trigger（label「值」）：弹性宽度，高度应仍是 h-8（32）。
    {
        let p = probed.lock().unwrap();
        let hs: Vec<f32> = p
            .labels
            .iter()
            .filter(|(_, l)| l.as_str() == "值")
            .filter_map(|(id, _)| abs(&p, *id))
            .map(|(_, _, s)| s.height.0)
            .collect();
        assert!(!hs.is_empty(), "找不到值 trigger");
        assert!(
            hs.iter().all(|h| *h >= 30.0),
            "值 trigger 高度应≈32，实际 {:?}",
            hs
        );
    }

    // 字段换成「文件大小」：列表不限高，整列一次铺开不滚动——末行
    // 「修改时间」也应立即可见。
    app.press("label:字段").unwrap();
    app.wait_for("role:option[修改时间]").unwrap();
    app.press("role:option[文件大小]").unwrap();
    app.wait(Duration::from_millis(200)).unwrap();

    // 再开弹层：仍整列铺开；列表比触发器上方空间高时弹层贴窗口边
    // （cover 对齐放不下就贴边，与原生菜单一致）——整列可见、弹层盖着
    // trigger 即可。
    app.press("label:字段").unwrap();
    app.wait_for("role:option[文件大小]").unwrap();
    app.wait_for("role:option[修改时间]").unwrap();
    app.wait(Duration::from_millis(300)).unwrap();
    let p = probed.lock().unwrap();
    let trig = p
        .labels
        .iter()
        .find(|(_, l)| l.as_str() == "字段")
        .map(|(id, _)| *id)
        .expect("找不到字段 trigger");
    let option = |title: &str| {
        p.labels
            .iter()
            .find(|(_, l)| l.as_str() == title)
            .and_then(|(id, _)| abs(&p, *id))
            .unwrap_or_else(|| panic!("选项 {title} 没有 frame"))
    };
    let (_, ty, ts) = abs(&p, trig).expect("trigger 没有 frame");
    let trig_mid = ty + ts.height.0 / 2.0;
    let (_, first_y, _) = option("标题");
    let (_, last_y, last_s) = option("修改时间");
    let (_, sel_y, sel_s) = option("文件大小");
    assert!(first_y >= 4.0, "弹层顶应贴窗口内：首行 y={first_y:.1}");
    assert!(
        last_y + last_s.height.0 <= 800.0,
        "末行应在窗口内：底={:.1}",
        last_y + last_s.height.0
    );
    assert!(
        first_y <= trig_mid && last_y + last_s.height.0 >= trig_mid,
        "弹层应覆盖 trigger：行区间 {first_y:.1}..{:.1} vs trigger 中 {trig_mid:.1}",
        last_y + last_s.height.0
    );
    assert!(
        sel_y >= first_y && sel_y + sel_s.height.0 <= last_y + last_s.height.0,
        "选中行应在列表内"
    );
}

/// 侧栏文件树各行的文件夹图标必须完全水平对齐：有子目录的行是「箭头
/// + 图标」，无子目录的行是「16pt 占位 + 图标」——行内最靠右的图像
/// 都是文件夹图标，x 应一致。
#[test]
fn sidebar_tree_alignment_probe() {
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    let mut app = start(1200.0, 800.0);
    let probed = probe(&mut app);

    // 切到「文件夹」
    app.press("label:切换视图").unwrap();
    app.wait_for("role:option[文件夹]").unwrap();
    app.press("role:option[文件夹]").unwrap();
    app.wait_for_within("text:测试歌曲", Duration::from_secs(20))
        .unwrap();

    let p = probed.lock().unwrap();

    // 两个根目录行：zeo-player-test-music 有子目录 sub，zz_empty_folder 没有子目录
    let corpus_root = dunce::canonicalize(CORPUS)
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let empty_root = "zz_empty_folder".to_string();

    let root1_id = p
        .labels
        .iter()
        .find(|(_, l)| l.as_str() == corpus_root)
        .map(|(id, _)| *id)
        .unwrap_or_else(|| panic!("找不到根目录行 {corpus_root}"));

    let root2_id = p
        .labels
        .iter()
        .find(|(_, l)| l.as_str() == empty_root)
        .map(|(id, _)| *id)
        .unwrap_or_else(|| panic!("找不到空目录行 {empty_root}"));

    let is_descendant = |mut id: NodeId, ancestor: NodeId| {
        for _ in 0..16 {
            if id == ancestor {
                return true;
            }
            match p.parents.get(&id) {
                Some(&par) => id = par,
                None => break,
            }
        }
        false
    };

    // 文件夹图标是行内最靠右的图像（有子目录的行左边还有折叠箭头）。
    let folder_x = |row: NodeId| {
        p.images
            .keys()
            .filter(|&&id| is_descendant(id, row))
            .filter_map(|&id| abs(&p, id).map(|(x, _, _)| x))
            .fold(f32::NEG_INFINITY, f32::max)
    };
    let x1 = folder_x(root1_id);
    let x2 = folder_x(root2_id);
    assert!(x1.is_finite() && x2.is_finite(), "行里找不到文件夹图标");

    assert_eq!(
        x1, x2,
        "有子目录的文件夹 ({x1:.1}) 与无子目录的文件夹 ({x2:.1}) 图标应完全水平对齐"
    );
}

#[test]
fn header_resize_indicator_probe() {
    let mut app = start(1400.0, 800.0);
    let probed = probe(&mut app);
    app.press("元数据").unwrap();
    app.wait_for("id:resize:title").unwrap();
    let p = probed.lock().unwrap();
    let mut found_count = 0;
    for (id, tid) in &p.test_ids {
        if tid.starts_with("resize:") {
            let handle_frame = p.frames.get(id).expect("resize handle 应具有 frame");
            assert_eq!(
                handle_frame.origin.y.0, 0.0,
                "{tid} 把手 origin.y 应为 0 (实际: {})",
                handle_frame.origin.y.0
            );
            assert_eq!(
                handle_frame.size.height.0, 36.0,
                "{tid} 把手 height 应为 36pt (实际: {})",
                handle_frame.size.height.0
            );

            // 查找指示条子节点
            for (&child, &par) in &p.parents {
                if par == *id {
                    let child_frame = p.frames.get(&child).expect("指示条应具有 frame");
                    assert_eq!(
                        child_frame.origin.y.0, 0.0,
                        "{tid} 的指示条 origin.y 应为 0 (实际: {})",
                        child_frame.origin.y.0
                    );
                    assert_eq!(
                        child_frame.size.height.0, 36.0,
                        "{tid} 的指示条 height 应为 36pt (实际: {})",
                        child_frame.size.height.0
                    );
                }
            }
            found_count += 1;
        }
    }
    assert!(found_count > 0, "应检测到表头列宽拖拽把手");
}
