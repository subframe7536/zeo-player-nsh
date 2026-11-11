//! 「关于」页的应用更新：检查 / 下载 / 安装在后台线程跑（`ureq` 是同步
//! IO），状态经 `poster()` 回排进 `Store::update` 信号。安装好后由界面
//! 调 `element::desktop::restart()` 重启生效。

use std::sync::Mutex;

use super::*;

/// 最近一次检查到的更新（`download_and_install` 从它拿产物地址）。
static FOUND: Mutex<Option<zeo_updater::Update>> = Mutex::new(None);

/// 打包运行才有更新渠道（开发 / 测试里是 `false`，「关于」页只显示
/// 版本信息）。
pub fn updates_supported() -> bool {
    zeo_updater::supported()
}

/// 检查更新（手动按钮与启动自动检查共用；检查中重复点直接跳过）。
pub fn check_for_update() {
    let s = store();
    if !updates_supported()
        || s.update
            .with(|u| matches!(u, UpdateState::Checking | UpdateState::Downloading { .. }))
    {
        return;
    }
    s.update.set(UpdateState::Checking);
    let post = poster();
    std::thread::spawn(move || {
        let next = match zeo_updater::check() {
            Ok(Some(u)) => {
                *FOUND.lock().unwrap() = Some(u.clone());
                UpdateState::Available {
                    version: u.version,
                    notes: u.notes,
                    can_install: u.can_install,
                    url: u.url,
                    can_download: u.can_download,
                }
            }
            Ok(None) => UpdateState::Current,
            Err(e) => UpdateState::Error(e),
        };
        post.post(move || s.update.set(next));
    });
}

/// 下载并安装检查到的更新，完成后状态是 `Ready`——界面给「重启生效」。
pub fn download_and_install() {
    let s = store();
    let Some(found) = FOUND.lock().unwrap().clone() else {
        return;
    };
    let post = poster();
    let progress_post = post.clone();
    let version = found.version.clone();
    s.update.set(UpdateState::Downloading {
        received: 0,
        total: None,
    });
    std::thread::spawn(move || {
        let next = zeo_updater::download(&found, move |received, total| {
            progress_post.post(move || s.update.set(UpdateState::Downloading { received, total }));
        })
        .and_then(|archive| {
            zeo_updater::install(&archive)?;
            let _ = std::fs::remove_file(&archive);
            Ok(())
        });
        let next = match next {
            Ok(()) => UpdateState::Ready { version },
            Err(e) => UpdateState::Error(e),
        };
        post.post(move || store().update.set(next));
    });
}

/// 安装好的版本重启生效（`ready` 状态的「重启并更新」按钮）。
pub fn restart_to_update() {
    nsh_core::element::desktop::restart();
}

/// 启动时的自动检查（「自动检查更新」开着才跑；静默——只在「关于」
/// 页呈现，不打断）。
pub fn auto_check_updates() {
    let s = store();
    if s.library.with(|l| l.config.auto_check_updates) {
        check_for_update();
    }
}

/// 「自动检查更新」开关。
pub fn set_auto_check_updates(on: bool) {
    update_config(|c| c.auto_check_updates = on);
    std::thread::spawn(move || library_store().set_auto_check_updates(on));
}
