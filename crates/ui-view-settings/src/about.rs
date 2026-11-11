//! 「关于」页：应用信息（名称 / 版本 / 项目地址）与更新（检查、下载、
//! 安装、重启）。更新只在上游打包产物里可用——`zeo_updater` 读
//! `nsh package` 写的 `nsh-updater.json`，开发运行里 `supported()` 是
//! 假，更新区只留一句说明。

use nsh_core::prelude::*;

use super::{pref_row, setting_section};
use zeo_ui_components::button::{Size, Variant, text_button};
use zeo_ui_store::theme::*;
use zeo_ui_store::{UpdateState, store};

/// 项目主页（`git remote` 的仓库；`nsh.json` 的 `updater.github` 同源）。
const REPO: &str = "https://github.com/subframe7536/zeo-player-nsh";

/// 「应用信息」节：名称、版本、bundle id、项目地址。
pub(crate) fn app_section() -> View {
    let open_repo = || {
        spawn(async move {
            open_url(REPO).await;
        });
    };
    setting_section(
        "应用信息",
        "本地优先的桌面音乐播放器，纯 Rust NSH 应用。",
        Some(text_button(Variant::Outline, Size::Sm, None, "项目主页", open_repo).into_element()),
    )
    .child(
        view()
            .gap(px(4.0))
            .child(
                text(format!(
                    "{}  v{}",
                    zeo_updater::app_name(),
                    zeo_updater::app_version()
                ))
                .text_size(px(15.0))
                .line_height(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg()),
            )
            .child(
                text(format!(
                    "构建 {} · {}",
                    env!("CARGO_PKG_VERSION"),
                    zeo_updater::app_id()
                ))
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg()),
            ),
    )
}

/// 「应用更新」节：当前版本、自动检查开关、检查 / 下载 / 安装 / 重启的
/// 状态与按钮。
pub(crate) fn update_section() -> View {
    let s = store();
    let status = move || {
        text(match s.update.get() {
            UpdateState::Idle => "尚未检查".into(),
            UpdateState::Checking => "检查更新中…".into(),
            UpdateState::Current => "已是最新版本".into(),
            UpdateState::Available { ref version, .. } => {
                format!("发现新版本 v{version}")
            }
            UpdateState::Downloading { received, total } => match total {
                Some(total) if total > 0 => {
                    format!("下载中 {} / {}", bytes(received), bytes(total))
                }
                _ => format!("下载中 {}", bytes(received)),
            },
            UpdateState::Ready { ref version } => {
                format!("v{version} 已安装，重启后生效")
            }
            UpdateState::Error(ref e) => format!("更新失败：{e}"),
        })
        .text_size(px(13.0))
        .line_height(px(13.0))
        .text_color(muted_fg())
    };
    // 版本说明（有就显示，限 3 行）。
    let notes = move || {
        dynamic(move || {
            let UpdateState::Available {
                notes: Some(ref n), ..
            } = s.update.get()
            else {
                return view().into_element();
            };
            text(n.clone())
                .text_size(px(12.0))
                .number_of_lines(3)
                .text_color(muted_fg())
                .into_element()
        })
        .into_element()
    };
    let action = move || {
        dynamic(move || {
            if !zeo_ui_store::updates_supported() {
                return view().into_element();
            }
            let button = match s.update.get() {
                UpdateState::Available {
                    can_install: true,
                    can_download: true,
                    ..
                } => text_button(
                    Variant::Default,
                    Size::Sm,
                    None,
                    "下载并安装",
                    zeo_ui_store::download_and_install,
                ),
                UpdateState::Available { ref url, .. } => {
                    let url = url.clone();
                    text_button(
                        Variant::Outline,
                        Size::Sm,
                        None,
                        "打开下载页",
                        move || {
                            let url = url.clone();
                            spawn(async move {
                                open_url(url).await;
                            });
                        },
                    )
                }
                UpdateState::Ready { .. } => text_button(
                    Variant::Default,
                    Size::Sm,
                    None,
                    "重启并更新",
                    zeo_ui_store::restart_to_update,
                ),
                _ => text_button(
                    Variant::Outline,
                    Size::Sm,
                    None,
                    "检查更新",
                    zeo_ui_store::check_for_update,
                ),
            };
            button.into_element()
        })
        .into_element()
    };
    let desc = move || {
        if zeo_ui_store::updates_supported() {
            "从 GitHub Releases 检查新版本；下载的归档校验 SHA-256 后原地替换，重启生效。"
        } else {
            "开发运行没有更新渠道（打包构建里从 GitHub Releases 检查新版本）。"
        }
    };
    let section = setting_section(
        "应用更新",
        "新版本从 GitHub Releases 下载，校验后原地替换。",
        Some(action()),
    );
    let section = if zeo_ui_store::updates_supported() {
        section.child(status()).child(notes()).child(pref_row(
            "自动检查更新",
            "启动时在后台检查一次（不打扰、可关）。",
            move || s.library.with(|l| l.config.auto_check_updates),
            zeo_ui_store::set_auto_check_updates,
        ))
    } else {
        section.child(text(desc()).text_size(px(13.0)).text_color(muted_fg()))
    };
    section
}

/// 字节数的人读写法（下载进度）。
fn bytes(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / 1048576.0)
    } else {
        format!("{:.0} KB", n as f64 / 1024.0)
    }
}
