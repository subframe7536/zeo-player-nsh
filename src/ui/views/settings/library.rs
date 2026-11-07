//! 设置 →「曲库」分类的各节。

use std::collections::HashSet;

use nsh_core::prelude::*;

use super::super::super::components::button::{icon_button, outline_button, pill_button};
use super::super::super::components::icons::{Icon, icon};
use super::super::super::store::{format_total, scan, store};
use super::super::super::theme::*;
use super::super::{SEP, add_dropped, last_segment, total_duration, url_to_path};
use super::{note, pref_row, setting_section};
use crate::config::library_store;

pub(super) fn folders_section() -> View {
    let s = store();
    let message = signal(String::new());
    let dragging = signal(false);
    let picking = signal(false);

    let pick = move || {
        if picking.get_untracked() {
            return;
        }
        picking.set(true);
        spawn(async move {
            if let Some(result) = super::super::super::store::pick_folders().await {
                message.set(result);
            }
            picking.set(false);
        });
    };

    let counts = computed(move || {
        let roots = s.library.with(|l| l.config.roots.clone());
        let tracks = s.tracks.get();
        roots
            .iter()
            .map(|r| {
                let prefix = format!("{}{SEP}", r.trim_end_matches(SEP));
                tracks
                    .iter()
                    .filter(|t| t.path.starts_with(&prefix))
                    .count()
            })
            .collect::<Vec<_>>()
    });

    let rows = dynamic(move || {
        let roots = s.library.with(|l| l.config.roots.clone());
        if roots.is_empty() {
            return vec![
                view()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .h(px(120.0))
                    .rounded(px(12.0))
                    .border_2()
                    .border_dashed()
                    .border_color(move || if dragging.get() { primary() } else { border() })
                    .bg(move || {
                        if dragging.get() {
                            hover()
                        } else {
                            AdaptiveColor::TRANSPARENT
                        }
                    })
                    .child(icon(Icon::Folder, 24.0, faint))
                    .child(
                        text(move || {
                            if dragging.get() {
                                "松开以添加"
                            } else {
                                "还没有文件夹。点「添加文件夹…」，或把文件夹拖到这里"
                            }
                        })
                        .text_size(px(13.0))
                        .text_color(muted_fg()),
                    )
                    .into_element(),
            ];
        }
        roots
            .into_iter()
            .enumerate()
            .map(|(i, root)| {
                let remove = root.clone();
                let name = last_segment(&root).to_owned();
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(12.0))
                    .h(px(56.0))
                    .px(px(12.0))
                    .rounded(px(10.0))
                    .bg(primary().opacity(0.08))
                    .child(
                        view()
                            .size(px(36.0))
                            .rounded(px(9.0))
                            .bg(move || bg())
                            .items_center()
                            .justify_center()
                            .child(icon(Icon::Folder, 18.0, primary)),
                    )
                    .child(
                        view()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap(px(2.0))
                            .child(
                                text(name)
                                    .number_of_lines(1)
                                    .text_size(px(14.0))
                                    .line_height(px(14.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(fg()),
                            )
                            .child(
                                text(root.clone())
                                    .number_of_lines(1)
                                    .text_size(px(12.0))
                                    .line_height(px(12.0))
                                    .text_color(muted_fg()),
                            ),
                    )
                    .child(
                        text(move || {
                            counts.with(|c| c.get(i).map(|n| format!("{n} 首")).unwrap_or_default())
                        })
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .tabular_nums()
                        .text_color(faint()),
                    )
                    .child(icon_button(
                        || Icon::Close,
                        14.0,
                        "移除",
                        muted_fg,
                        move || {
                            library_store().remove_root(&remove);
                            message.set(format!("已移除 {remove}"));
                            scan();
                        },
                    ))
                    .into_element()
            })
            .collect()
    });

    let add = view()
        .style_when(move || picking.get(), |st| st.opacity(0.5))
        .child(pill_button(Icon::Plus, "添加文件夹…", pick));

    setting_section(
        "音乐文件夹",
        "Zeo Player 会扫描这些文件夹里的 MP3、FLAC、WAV、AIFF、M4A（AAC / ALAC）、OGG，包括子文件夹。也可以把文件夹拖到这一节上。",
        Some(add.into_element()),
    )
    // 拖放目标的描边：平时透明，文件拖入时亮主题色。
    .rounded(px(10.0))
    .border_1()
    .border_color(move || {
        if dragging.get() {
            primary()
        } else {
            AdaptiveColor::TRANSPARENT
        }
    })
    .transition_colors()
    .duration(300.0)
    .on_file_drag_enter(move || dragging.set(true))
    .on_file_drag_leave(move || dragging.set(false))
    .on_file_drop(move |event: &FileDropEvent| {
        dragging.set(false);
        let mut added = false;
        let results: Vec<String> = event
            .uris
            .iter()
            .map(|uri| {
                let dropped = add_dropped(&url_to_path(uri));
                added |= dropped.added;
                dropped.note
            })
            .collect();
        if added {
            scan();
        }
        message.set(results.join("\n"));
    })
    .child(view().gap(px(8.0)).child(rows))
    .child(note(message))
}
pub(super) fn library_section() -> View {
    let s = store();
    let progress = move || s.library.with(|l| l.progress.clone());
    let scanning = move || s.library.with(|l| l.progress.scanning);
    let fraction = move || {
        let p = progress();
        if p.total > 0 {
            p.done as f64 / p.total as f64
        } else {
            0.0
        }
    };
    let actions = view()
        .flex_row()
        .gap(px(8.0))
        .style_when(scanning, |st| st.opacity(0.4))
        .child(outline_button(Icon::Refresh, "扫描更新", scan))
        .child(outline_button(
            Icon::Disc,
            "重建曲库",
            super::super::super::store::rebuild,
        ));

    let stats = move || {
        view()
            .flex_row()
            .items_end()
            .gap(px(24.0))
            .child(stat(
                move || s.tracks.with(|t| t.len()).to_string(),
                "首曲目",
            ))
            .child(stat(
                move || {
                    s.tracks.with(|t| {
                        t.iter()
                            .map(|t| (t.album_artist.as_str(), t.album.as_str()))
                            .collect::<HashSet<_>>()
                            .len()
                            .to_string()
                    })
                },
                "张专辑",
            ))
            .child(stat(
                move || {
                    s.tracks.with(|t| {
                        t.iter()
                            .map(|t| t.artist.as_str())
                            .collect::<HashSet<_>>()
                            .len()
                            .to_string()
                    })
                },
                "位歌手",
            ))
            .child(stat(
                move || format_total(s.tracks.with(|t| total_duration(t))),
                "总时长",
            ))
    };

    let bar = move || {
        view()
            .gap(px(8.0))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        text(move || {
                            let p = progress();
                            if p.total == 0 {
                                "正在查找音频文件…".to_owned()
                            } else {
                                format!("正在读取 {} / {}", p.done, p.total)
                            }
                        })
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .tabular_nums()
                        .text_color(fg()),
                    )
                    .child(view().flex_1())
                    .child(
                        text(move || format!("{:.0}%", fraction() * 100.0))
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .tabular_nums()
                            .text_color(muted_fg()),
                    ),
            )
            .child(
                nsh_base::progress()
                    .accessibility_label("扫描进度")
                    .value(move || (fraction() * 100.0) as f32)
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
            .child(
                text(move || progress().current)
                    .number_of_lines(1)
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(faint()),
            )
    };

    setting_section(
        "曲库",
        "索引保存在本机，启动时自动扫描。「重建曲库」会清空标签和封面缓存，从文件重新读取全部曲目。",
        Some(actions.into_element()),
    )
    .child(when(scanning, bar).otherwise(stats))
    .child(view().h(px(1.0)).bg(border()))
    .child(watch_row())
}
/// 「删除文件」节：曲目右键「删除文件…」是否经过回收站。
pub(super) fn delete_section() -> View {
    let s = store();
    setting_section("删除文件", "曲目右键菜单「删除文件…」的行为。", None).child(pref_row(
        "直接删除文件",
        "开启后删除文件不经过回收站（不可恢复）；关闭时文件先移到回收站。",
        move || s.library.with(|l| l.config.delete_permanently),
        super::super::super::store::set_delete_permanently,
    ))
}
/// 文件夹监视开关与状态。
fn watch_row() -> View {
    let s = store();
    let on = move || s.library.with(|l| l.config.watch);
    let status = move || {
        s.library.with(|l| {
            let w = &l.watcher;
            if !l.config.watch {
                return "关闭时，文件夹里的变化要手动「扫描更新」才会出现。".to_owned();
            }
            if let Some(error) = &w.error {
                return error.clone();
            }
            let mut text = format!(
                "正在监视 {} 个文件夹，新增、修改、删除或改名音频文件后自动扫描",
                w.roots
            );
            if let Some(at) = w.last_rescan {
                let ago = crate::config::unix_now().saturating_sub(at);
                let when = match ago {
                    0..60 => "刚刚".to_owned(),
                    60..3600 => format!("{} 分钟前", ago / 60),
                    _ => format!("{} 小时前", ago / 3600),
                };
                text = format!("{text}。上次：{when}，{} 处变化", w.last_changes);
            }
            text
        })
    };
    let toggle = nsh_base::switch()
        .checked(on)
        .accessibility_label("监视文件夹变化")
        .on_change(move |next: &bool| super::super::super::store::set_watch(*next))
        .track_style(|st| {
            st.w(px(42.0))
                .h(px(24.0))
                .p(px(2.0))
                .rounded_full()
                .bg(muted())
                .border_1()
                .border_color(border())
                .transition_colors()
                .duration(150.0)
                .checked(|st| st.bg(move || primary()).border_color(move || primary()))
        })
        .thumb_style(|st| {
            st.size(px(18.0))
                .rounded_full()
                .bg(AdaptiveColor::rgb(0xffffff, 0xffffff))
                .shadow_sm()
                .transition_transform()
                .duration(150.0)
                .checked(|st| st.translate_x(px(18.0)))
        });
    view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(16.0))
        .child(
            view()
                .flex_1()
                .min_w(px(200.0))
                .gap(px(2.0))
                .child(
                    text("监视文件夹变化")
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg()),
                )
                .child(
                    text(status)
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(move || {
                            if s.library
                                .with(|l| l.config.watch && l.watcher.error.is_some())
                            {
                                nsh_base::theme().destructive
                            } else {
                                muted_fg()
                            }
                        }),
                ),
        )
        .child(toggle)
}
fn stat(value: impl Fn() -> String + 'static, label: &'static str) -> View {
    view()
        .gap(px(2.0))
        .child(
            text(value)
                .text_size(px(22.0))
                .line_height(px(22.0))
                .font_weight(FontWeight::BOLD)
                .tabular_nums()
                .text_color(fg()),
        )
        .child(
            text(label)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg()),
        )
}
/// 过滤：低于时长 / 大小阈值的音频不进曲库（改后自动重新扫描）。
pub(super) fn filter_section() -> View {
    let s = store();
    const DURATIONS: &[(u32, &str)] = &[
        (0, "不过滤"),
        (5, "5 秒"),
        (10, "10 秒"),
        (30, "30 秒"),
        (60, "1 分钟"),
    ];
    const SIZES: &[(u32, &str)] = &[
        (0, "不过滤"),
        (50, "50 KB"),
        (100, "100 KB"),
        (500, "500 KB"),
        (1024, "1 MB"),
    ];
    let row = |label: &'static str,
               options: &'static [(u32, &'static str)],
               field: fn(&crate::config::LibraryConfig) -> u32,
               set: fn(u32)| {
        view()
            .flex_row()
            .items_center()
            .flex_wrap()
            .gap(px(10.0))
            .child(
                text(label)
                    .w(px(80.0))
                    .flex_shrink_0()
                    .text_size(px(14.0))
                    .line_height(px(14.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg()),
            )
            .child(
                super::super::super::components::radio::group_row().children(options.iter().map(
                    |&(value, name)| {
                        super::super::super::components::radio::item(
                            move || s.library.with(|l| field(&l.config) == value),
                            name,
                            move || set(value),
                        )
                    },
                )),
            )
            .into_element()
    };
    setting_section(
        "音频过滤",
        "低于时长或大小阈值的音频不进曲库（采样、铃声、碎片文件等），修改后自动重新扫描。",
        None,
    )
    .child(row(
        "最小时长",
        DURATIONS,
        |c| c.min_duration_secs,
        super::super::super::store::set_min_duration_secs,
    ))
    .child(row(
        "最小大小",
        SIZES,
        |c| c.min_size_kb,
        super::super::super::store::set_min_size_kb,
    ))
}
