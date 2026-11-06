//! 「编辑元数据」弹窗布局回归：用挂载批次还原控件绝对位置与底色——
//! 流派输入框紧跟最后一个 chip（同一行）；输入框与选中 tab 用不随封面
//! 染色的 card 底色，未选中 tab 透明。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use nsh_core::geometry::{Bounds, Points, Size, px, size};
use nsh_core::platform::Os;
use nsh_core::{MountItem, NodeId};
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
            let config = serde_json::json!({
                "roots": [dunce::canonicalize(CORPUS).unwrap()],
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
    let theme = nsh_base::Theme::from_json(include_str!("../theme.json")).expect("theme.json");
    let mut config = EngineConfig::new(Root::of::<App>().tokens(theme.into_tokens()));
    config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
    config.viewport = size(px(width), px(height));
    config.environment.os = Os::MacOs;
    zeo_player::engine::Player::global().set_volume(0.0);
    let mut app = Headless::start(config).expect("start the app");
    app.wait_for("文件夹").unwrap();
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
    scrolls: HashSet<NodeId>,
    styles: HashMap<NodeId, (u32, u32, f32)>, // bg, border color, border width
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
                }
                MountItem::Frame { id, frame } => {
                    p.frames.insert(*id, *frame);
                }
                MountItem::ViewProps { id, props } => {
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
                MountItem::ScrollContent { id, .. } => {
                    p.scrolls.insert(*id);
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

/// 沿父链累加 frame 原点，得到相对根的绝对位置（无滚动偏移的前提：
/// 弹窗内容不用滚）。
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
        match p.parents.get(&cur) {
            Some(&parent) => cur = parent,
            None => break,
        }
    }
    size.map(|s| (x, y, s))
}

#[test]
fn edit_dialog_layout_probe() {
    if !isolate() {
        return eprintln!("跳过：没有 {CORPUS}");
    }
    let mut app = start(1200.0, 800.0);
    let probed = probe(&mut app);

    app.press("文件夹").unwrap();
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

    // 底色（测试主题为浅色）：标题输入框 / 选中 tab「基础」= card
    // 白（不随封面染色），选中 tab 带 1pt 边框；未选中 tab「封面」透明。
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
    let (sel_bg, sel_border, sel_bw) = style_of("基础");
    assert_eq!(sel_bg, WHITE, "选中 tab 应为 card 白");
    assert!(
        sel_bw >= 1.0 && sel_border != 0,
        "选中 tab 应有可见边框（bw={sel_bw}, color={sel_border:#x}）"
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
    app.wait(Duration::from_millis(200)).unwrap();
    let p = probed.lock().unwrap();

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
        if p.scrolls.contains(&cur) {
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
