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

#[path = "layout_probe/dialogs.rs"]
mod dialogs;
#[path = "layout_probe/probes.rs"]
mod probes;
