//! 界面测试：无头引擎运行根组件 `App`（`cargo test` / `nsh test` 都会跑）。
//!
//! 曲库用独立的数据目录；有 `.nsh/tmp/zeo-player-test-music` 语料（`scripts/make-test-music.sh`）
//! 时把它设为根文件夹，没有时只跑不依赖曲目的部分。

use std::sync::Once;
use std::time::Duration;

use nsh_core::geometry::{point, px, size};
use nsh_core::platform::Os;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use zeo_player::App;

const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/.nsh/tmp/zeo-player-test-music"
);

/// 隔离的数据 / 缓存目录（整个测试进程一次；曲库存储是全局的）。
fn isolate() -> bool {
    static ONCE: Once = Once::new();
    let corpus = std::path::Path::new(CORPUS).is_dir();
    ONCE.call_once(|| {
        let data = std::env::temp_dir().join(format!("zeo-player-ui-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        // 关掉「记住窗口大小和位置」：曲库存储是进程全局的，并行用例的
        // 窗口位置会互相写 / 互相恢复（恢复位置时窗口状态上报，见下
        // `set_viewport` 的回种注释）。
        let mut config = serde_json::json!({
            "remember_frame": false,
        });
        if corpus {
            // 语料只有 1–3 秒、几十 KB：library.json 里把过滤阈值写成 0。
            config = serde_json::json!({
                "roots": [dunce::canonicalize(CORPUS).unwrap()],
                "min_duration_secs": 0,
                "min_size_kb": 0,
                "remember_frame": false,
            });
        }
        std::fs::write(data.join("library.json"), config.to_string()).unwrap();
        // SAFETY: 在任何线程读这些变量之前设置（Once 内，测试开始前）。
        unsafe {
            std::env::set_var("ZEO_PLAYER_DATA_DIR", &data);
        }
    });
    corpus
}

/// 播放器是进程内全局单例：`init` 的 `restore_last` 会换掉共享的当前
/// 曲目。播放敏感的用例持锁跑全程；其它用例起 app 时短暂取锁，撞上了
/// 就等它放完。
static PLAY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 持锁的用例 panic 会毒化 Mutex——锁里只有顺序语义没有数据，照拿。
fn play_lock() -> std::sync::MutexGuard<'static, ()> {
    PLAY_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// 桌面尺寸（宽 × 高，点）启动应用，主题与 nsh.json 相同。
fn start(width: f32, height: f32) -> Headless {
    let _guard = play_lock();
    start_inner(width, height)
}

fn start_inner(width: f32, height: f32) -> Headless {
    isolate();
    zeo_player::config::library_store()
        .set_sidebar_section(zeo_player::config::SidebarSection::Playlists);
    let theme = nsh_base::Theme::from_json(include_str!("../theme.json")).expect("theme.json");
    let mut config = EngineConfig::new(Root::of::<App>().tokens(theme.into_tokens()));
    config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
    config.viewport = size(px(width), px(height));
    config.environment.os = Os::MacOs;
    // 静音（在挂载前）：按压与恢复的播放会真的走 cpal 输出，测试不要出声。
    zeo_player::engine::Player::global().set_volume(0.0);
    let viewport = config.viewport;
    let mut app = Headless::start(config).expect("start the app");
    // 驱动侧缓存的 environment.window 从 0 起（engine 才拿 config.viewport
    // 填）：主窗口状态一上报整份 environment 就把引擎的 env.window 盖成 0，
    // 列宽断点全当窄窗口——先按真实尺寸回种一次。
    app.set_viewport(viewport).unwrap();
    app.wait_for("首页").unwrap();
    app
}

fn goto_folders(app: &mut Headless) {
    app.press("label:切换视图").unwrap();
    app.wait_for("role:option[文件夹]").unwrap();
    app.press("role:option[文件夹]").unwrap();
}

fn texts(app: &Headless) -> Vec<String> {
    app.screen()
        .texts()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn has(app: &Headless, s: &str) -> bool {
    app.screen().texts().contains(&s)
}

/// 双击播放 `sel`：行的双击窗口是 450ms，并行负载下两次 `press` 的往返
/// 间隔可能超过它、退化成两次单击——连按到有相邻一对落进窗口为止（有上
/// 界，超时说明播放真的没发生）。退出要看「选中且在播」，不能只看选中：
/// `restore_last` 会把上次暂停的曲目恢复成暂停的当前行（selected 为真），
/// 那时一次单击什么都不会触发。
fn double_press(app: &mut Headless, sel: &str) {
    for _ in 0..12 {
        if app.screen().selected(sel).unwrap_or(false)
            && zeo_player::engine::Player::global().state().is_playing
        {
            return;
        }
        app.press(sel).unwrap();
        std::thread::sleep(Duration::from_millis(40));
    }
    panic!("双击 {sel} 未播放: {:?}", texts(app));
}

/// 测试语料根文件夹在文件夹页里的名字。
fn root_name() -> String {
    dunce::canonicalize(CORPUS)
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[path = "ui/appearance.rs"]
mod appearance;
#[path = "ui/folders.rs"]
mod folders;
#[path = "ui/home.rs"]
mod home;
#[path = "ui/metadata.rs"]
mod metadata;
#[path = "ui/settings.rs"]
mod settings;
#[path = "ui/views.rs"]
mod views;
