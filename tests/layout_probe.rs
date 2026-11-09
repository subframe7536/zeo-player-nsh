//! 「编辑元数据」弹窗与「新建智能歌单」条件编辑器的布局回归：用挂载
//! 批次还原控件绝对位置与底色——流派输入框紧跟最后一个 chip（同一
//! 行）；输入框与选中 tab 的 indicator 用不随封面染色的 card 底色，
//! tab 自身透明。条件行里的「值」触发器高度应是 h-8（select 列容器
//! 里 flex_1 长纵轴会把它压没）；弹层重开时选中行盖在触发器上
//! （cover 对齐，滚动收敛前的采样由宽度脉冲重采修正）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use nsh_core::geometry::{Bounds, Points, Size, px, size};
use nsh_core::platform::Os;
use nsh_core::{Command, MountItem, NodeId};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use zeo_player::App;

const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/.nsh/tmp/zeo-player-test-music"
);

fn isolate() -> bool {
    static ONCE: Once = Once::new();
    let corpus = std::path::Path::new(CORPUS).is_dir();
    ONCE.call_once(|| {
        let data = std::env::temp_dir().join(format!("zeo-player-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        if corpus {
            let empty_dir = data.join("zz_empty_folder");
            let _ = std::fs::create_dir_all(&empty_dir);
            let config = serde_json::json!({
                "roots": [
                    dunce::canonicalize(CORPUS).unwrap(),
                    dunce::canonicalize(&empty_dir).unwrap(),
                ],
                "min_duration_secs": 0,
                "min_size_kb": 0,
            });
            std::fs::write(data.join("library.json"), config.to_string()).unwrap();
        }
        unsafe {
            std::env::set_var("ZEO_PLAYER_DATA_DIR", &data);
        }
    });
    corpus
}

fn start(width: f32, height: f32) -> Headless {
    isolate();
    // 侧栏视图是全局持久化状态：同进程跑过的用例可能把它留在
    // 「文件夹」/「标签」——每个实例起前统一收回歌单，别继承上一个的残局。
    zeo_player::config::library_store()
        .set_sidebar_section(zeo_player::config::SidebarSection::Playlists);
    let theme = nsh_base::Theme::from_json(include_str!("../theme.json")).expect("theme.json");
    let mut config = EngineConfig::new(Root::of::<App>().tokens(theme.into_tokens()));
    config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
    config.viewport = size(px(width), px(height));
    config.environment.os = Os::MacOs;
    zeo_player::engine::Player::global().set_volume(0.0);
    let mut app = Headless::start(config).expect("start the app");
    app.wait_for("首页").unwrap();
    app
}

#[derive(Default)]
struct Probed {
    parents: HashMap<NodeId, NodeId>,
    frames: HashMap<NodeId, Bounds<Points>>,
    labels: HashMap<NodeId, String>,
    placeholders: HashMap<NodeId, String>,
    texts: HashMap<NodeId, String>,
    images: HashMap<NodeId, String>,
    scrolls: HashMap<NodeId, Size<Points>>,
    scroll_by: HashMap<NodeId, f32>,
    styles: HashMap<NodeId, (u32, u32, f32)>, // bg, border color, border width
    test_ids: HashMap<NodeId, String>,
}

fn probe(app: &mut Headless) -> Arc<Mutex<Probed>> {
    let probed = Arc::new(Mutex::new(Probed::default()));
    let p = probed.clone();
    app.observe(move |_, batch| {
        let mut p = p.lock().unwrap();
        for item in &batch.items {
            match item {
                MountItem::Insert { parent, child, .. } => {
                    p.parents.insert(*child, *parent);
                }
                // 节点可复用：卸载 / 销毁时清掉，否则切栏后拿到旧数据。
                MountItem::Remove { id } | MountItem::Delete { id } => {
                    p.parents.remove(id);
                    p.frames.remove(id);
                    p.labels.remove(id);
                    p.placeholders.remove(id);
                    p.texts.remove(id);
                    p.images.remove(id);
                    p.styles.remove(id);
                    p.scrolls.remove(id);
                    p.scroll_by.remove(id);
                    p.test_ids.remove(id);
                }
                MountItem::Frame { id, frame } => {
                    p.frames.insert(*id, *frame);
                }
                MountItem::ViewProps { id, props } => {
                    if let Some(t) = &props.test_id {
                        p.test_ids.insert(*id, t.to_string());
                    }
                    if let Some(l) = &props.accessibility.label {
                        p.labels.insert(*id, l.to_string());
                    }
                }
                MountItem::AccessibilityLabel { id, label } => match label {
                    Some(l) => {
                        p.labels.insert(*id, l.to_string());
                    }
                    None => {
                        p.labels.remove(id);
                    }
                },
                MountItem::Input { id, config } => {
                    p.placeholders.insert(*id, config.placeholder.clone());
                }
                MountItem::Text { id, text, .. } => {
                    p.texts.insert(*id, text.text.clone());
                }
                MountItem::Image { id, source, .. } => {
                    p.images.insert(*id, format!("{source:?}"));
                }
                MountItem::ScrollContent { id, size } => {
                    p.scrolls.insert(*id, *size);
                }
                MountItem::ScrollBy { id, dy, .. } => {
                    *p.scroll_by.entry(*id).or_default() += dy.0;
                }
                MountItem::Command {
                    id,
                    command: Command::ScrollTo { y, .. },
                } => {
                    let end = scroll_end(&p, *id);
                    p.scroll_by.insert(*id, y.0.clamp(0.0, end));
                }
                MountItem::Command {
                    id,
                    command: Command::ScrollToEnd { .. },
                } => {
                    let end = scroll_end(&p, *id);
                    p.scroll_by.insert(*id, end);
                }
                MountItem::ViewStyle { id, style } => {
                    p.styles.insert(
                        *id,
                        (
                            style.background_color.0,
                            style.borders.colors[0].0,
                            style.borders.widths.top.0,
                        ),
                    );
                }
                _ => {}
            }
        }
    });
    probed
}

/// 沿父链累加 frame 原点并减去祖先滚动偏移，得到相对根的绝对位置
/// （同 headless 的 bounds_in_root）。
fn abs(p: &Probed, id: NodeId) -> Option<(f32, f32, Size<Points>)> {
    let (mut x, mut y, mut size) = (0.0f32, 0.0f32, None);
    let mut cur = id;
    for _ in 0..64 {
        if let Some(f) = p.frames.get(&cur) {
            if size.is_none() {
                size = Some(f.size);
            }
            x += f.origin.x.0;
            y += f.origin.y.0;
        }
        if let Some(&off) = p.scroll_by.get(&cur) {
            y -= off;
        }
        match p.parents.get(&cur) {
            Some(&parent) => cur = parent,
            None => break,
        }
    }
    size.map(|s| (x, y, s))
}

/// 滚动尽头：内容高 - 视口高（同 headless 的 scroll_end，垂直向）。
fn scroll_end(p: &Probed, id: NodeId) -> f32 {
    let content = p.scrolls.get(&id).map(|s| s.height.0).unwrap_or(0.0);
    let viewport = p.frames.get(&id).map(|f| f.size.height.0).unwrap_or(0.0);
    (content - viewport).max(0.0)
}

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
