//! 「关于」页：应用信息（名称 / 版本 / 项目地址）与更新（检查、下载、
//! 安装、重启）。更新只在上游打包产物里可用——`zeo_updater` 读
//! `nsh package` 写的 `nsh-updater.json`，开发运行里 `supported()` 是
//! 假，更新区只留一句说明。

use nsh_core::prelude::*;

use super::{pref_row, setting_section};
use zeo_ui_components::button::{Size, Variant, text_button};
use zeo_ui_components::form::{dialog_content, dialog_footer, dialog_header, modal, switch};
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
                    zeo_updater::app_version(),
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
        text(status_text(&s.update.get()))
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
                // 检查 / 下载进行中：禁用并标出进度，不再是可再点的「检查更新」。
                UpdateState::Checking => {
                    text_button(Variant::Outline, Size::Sm, None, "检查中…", || {})
                        .disabled(|| true)
                }
                UpdateState::Downloading { received, total } => {
                    let label = match total {
                        Some(total) if total > 0 => {
                            format!("下载中 {:.0}%", received as f64 / total as f64 * 100.0)
                        }
                        _ => "下载中…".to_owned(),
                    };
                    text_button(Variant::Outline, Size::Sm, None, label, || {}).disabled(|| true)
                }
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

/// 「发现新版本」弹窗：检查到更新就弹（启动自动检查与「关于」页手动
/// 检查一样），列版本、更新说明与更新包大小；「自动检查更新」的开关
/// 就地可关。关掉只是不再打扰——`Store::update` 的结果仍在，「关于」
/// 页能继续下载安装；弹窗开着时下载 / 安装也在这里实时变成进度与
/// 「重启并更新」。
pub fn update_dialog() -> AnyElement {
    let s = store();
    let close = move || s.update_prompt.set(false);
    let content = dialog_content(460.0).child(dynamic(move || {
        let col = view().gap(px(16.0));
        match s.update.get() {
            UpdateState::Available {
                version,
                notes,
                url,
                can_install,
                can_download,
                size,
            } => {
                let size = size
                    .map(|n| format!(" · 更新包 {}", bytes(n)))
                    .unwrap_or_default();
                let confirm = if can_install && can_download {
                    text_button(
                        Variant::Default,
                        Size::Default,
                        None,
                        "下载并安装",
                        zeo_ui_store::download_and_install,
                    )
                    .into_element()
                } else {
                    text_button(
                        Variant::Default,
                        Size::Default,
                        None,
                        "打开下载页",
                        move || {
                            let url = url.clone();
                            spawn(async move {
                                open_url(url).await;
                            });
                            close();
                        },
                    )
                    .into_element()
                };
                col.child(dialog_header("发现新版本", None))
                    .child(
                        text(format!(
                            "v{} → v{version}{size}",
                            zeo_updater::app_version()
                        ))
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(muted_fg()),
                    )
                    .child(notes.and_then(|n| (!n.trim().is_empty()).then(|| notes_box(n))))
                    .child(auto_check_row())
                    .child(
                        dialog_footer()
                            .child(text_button(
                                Variant::Outline,
                                Size::Default,
                                None,
                                "稍后",
                                close,
                            ))
                            .child(confirm),
                    )
            }
            UpdateState::Downloading { received, total } => col
                .child(dialog_header("正在下载更新", None))
                .child(progress_bar(received, total))
                .child(dialog_footer().child(text_button(
                    Variant::Outline,
                    Size::Default,
                    None,
                    "后台下载",
                    close,
                ))),
            UpdateState::Ready { version } => col
                .child(dialog_header("更新已就绪", None))
                .child(
                    text(format!("v{version} 已安装，重启后生效"))
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(muted_fg()),
                )
                .child(
                    dialog_footer()
                        .child(text_button(
                            Variant::Outline,
                            Size::Default,
                            None,
                            "稍后重启",
                            close,
                        ))
                        .child(text_button(
                            Variant::Default,
                            Size::Default,
                            None,
                            "重启并更新",
                            zeo_ui_store::restart_to_update,
                        )),
                ),
            UpdateState::Error(e) => col
                .child(dialog_header("更新失败", None))
                .child(
                    text(e)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(muted_fg()),
                )
                .child(
                    dialog_footer()
                        .child(text_button(
                            Variant::Outline,
                            Size::Default,
                            None,
                            "关闭",
                            close,
                        ))
                        .child(text_button(
                            Variant::Default,
                            Size::Default,
                            None,
                            "重新检查",
                            zeo_ui_store::check_for_update,
                        )),
                ),
            _ => col
                .child(dialog_header("应用更新", None))
                .child(
                    text(status_text(&s.update.get()))
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .text_color(muted_fg()),
                )
                .child(dialog_footer().child(text_button(
                    Variant::Outline,
                    Size::Default,
                    None,
                    "关闭",
                    close,
                ))),
        }
        .into_element()
    }));
    modal(content, close)
}

/// 弹窗里的更新说明：限高的滚动块（`bg-muted` 圆角盒）。
fn notes_box(notes: String) -> View {
    view()
        .max_h(px(180.0))
        .p(px(12.0))
        .rounded(px(8.0))
        .bg(muted())
        .child(scroll().child(text(notes).text_size(px(13.0)).text_color(fg())))
}

/// 弹窗里的「自动检查更新」快关行：就地关掉后启动不再自动检查
/// （仍可随时在「设置 › 关于」改回）。
fn auto_check_row() -> View {
    let s = store();
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            view()
                .flex_1()
                .gap(px(4.0))
                .child(
                    text("自动检查更新")
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg()),
                )
                .child(
                    text("启动时在后台检查新版本")
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(switch(
            move || s.library.with(|l| l.config.auto_check_updates),
            "自动检查更新",
            zeo_ui_store::set_auto_check_updates,
        ))
}

/// 弹窗里的下载进度：百分比 + 进度条（`nsh_base::progress`，外观同
/// 「设置 › 曲库」的扫描进度）。
fn progress_bar(received: u64, total: Option<u64>) -> View {
    let fraction = match total {
        Some(total) if total > 0 => received as f64 / total as f64,
        _ => 0.0,
    };
    view()
        .gap(px(8.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(
                    text(match total {
                        Some(total) if total > 0 => {
                            format!("下载中 {} / {}", bytes(received), bytes(total))
                        }
                        _ => format!("下载中 {}", bytes(received)),
                    })
                    .text_size(px(13.0))
                    .line_height(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .tabular_nums()
                    .text_color(fg()),
                )
                .child(view().flex_1())
                .child(
                    text(format!("{:.0}%", fraction * 100.0))
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .tabular_nums()
                        .text_color(muted_fg()),
                ),
        )
        .child(
            nsh_base::progress()
                .accessibility_label("下载进度")
                .value(move || (fraction * 100.0) as f32)
                .track_style(|st| {
                    st.h(px(6.0))
                        .w_full()
                        .rounded_full()
                        .bg(muted())
                        .overflow_hidden()
                })
                .indicator_style(|st| {
                    st.h_full()
                        .rounded_full()
                        .bg(move || primary())
                        .transition_all()
                        .duration(150.0)
                }),
        )
}

/// 更新状态的一行读写法（「关于」节的状态行与弹窗兜底共用）。
fn status_text(u: &UpdateState) -> String {
    match u {
        UpdateState::Idle => "尚未检查".into(),
        UpdateState::Checking => "检查更新中…".into(),
        UpdateState::Current => "已是最新版本".into(),
        UpdateState::Available { version, .. } => format!("发现新版本 v{version}"),
        UpdateState::Downloading { received, total } => match total {
            Some(total) if *total > 0 => format!("下载中 {} / {}", bytes(*received), bytes(*total)),
            _ => format!("下载中 {}", bytes(*received)),
        },
        UpdateState::Ready { version } => format!("v{version} 已安装，重启后生效"),
        UpdateState::Error(e) => format!("更新失败：{e}"),
    }
}

/// 「欢迎指南」节：关掉设置，重新打开首次启动时的欢迎指南。
pub(crate) fn welcome_section() -> View {
    let reopen = || {
        store().settings.set(false);
        zeo_ui_store::open_welcome();
    };
    setting_section(
        "欢迎指南",
        "重新查看首次启动时的引导：添加音乐文件夹、选择主题色与功能一览。",
        Some(text_button(Variant::Outline, Size::Sm, None, "重新查看", reopen).into_element()),
    )
}

/// 字节数的人读写法（下载进度）。
fn bytes(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / 1048576.0)
    } else {
        format!("{:.0} KB", n as f64 / 1024.0)
    }
}
