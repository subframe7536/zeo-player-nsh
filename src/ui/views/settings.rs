//! 「设置」弹窗：左栏是搜索框与分类，右栏是所选分类的各节（曲库 / 播放 /
//! 歌词 / 窗口 / 标签 / 外观）。各节无卡片包装，靠间距分开。

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use nsh_core::element::input;
use nsh_core::prelude::*;

use super::super::components::button::{
    Size, Variant, icon_button, outline_button, pill_button, text_button,
};
use super::super::components::icons::{Icon, icon, icon_dyn};
use super::super::store::{Same, format_total, scan, set_theme_color, store};
use super::super::theme::*;
use super::{SEP, last_segment, total_duration};
use crate::config::{ThemeColor, library_store};

/// file:// URL → 路径（百分号解码）。Windows 的 `file:///C:/Music` 去掉
/// 盘符前的 `/`，并换成 `\\`。
fn url_to_path(uri: &str) -> String {
    let path = decode_url_path(uri);
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        return path[1..].replace('/', "\\");
    }
    path
}

fn decode_url_path(uri: &str) -> String {
    let rest = uri.strip_prefix("file://").unwrap_or(uri);
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 添加为根目录：目录直接加；文件取所在目录。
fn add_as_root(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches(['/', '\\']);
    let lib = library_store();
    match lib.add_root(trimmed) {
        Ok(_) => {
            scan();
            format!("已添加 {trimmed}")
        }
        Err(error) => {
            let parent = std::path::Path::new(trimmed)
                .parent()
                .map(|p| p.to_string_lossy().into_owned());
            match parent.map(|p| lib.add_root(&p).map(|_| p)) {
                Some(Ok(p)) => {
                    scan();
                    format!("已添加 {p}")
                }
                _ => error,
            }
        }
    }
}

/// 设置里的一节：标题、说明、右上角操作，然后是内容（无卡片包装，
/// 节与节之间靠页面间距分开）。
fn setting_section(title: &'static str, body: &'static str, action: Option<AnyElement>) -> View {
    view().w_full().gap(px(16.0)).child(
        view()
            .flex_row()
            .flex_wrap()
            .items_start()
            .gap(px(16.0))
            .child(
                view()
                    .flex_1()
                    .min_w(px(240.0))
                    .gap(px(4.0))
                    .child(
                        text(title)
                            .text_size(px(15.0))
                            .line_height(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg()),
                    )
                    .child(text(body).text_size(px(13.0)).text_color(muted_fg())),
            )
            .child(action.map(|a| view().flex_row().gap(px(8.0)).child(a))),
    )
}

/// 一行说明文字（操作结果）。
fn note(message: Signal<String>) -> AnyElement {
    dynamic(move || {
        let now = message.get();
        (!now.is_empty()).then(|| {
            text(now)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg())
        })
    })
}

fn folders_section() -> View {
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
            if let Some(result) = super::super::store::pick_folders().await {
                message.set(result);
            }
            picking.set(false);
        });
    };

    let counts = computed(move || {
        let roots = s.library.with(|l| l.roots.clone());
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
        let roots = s.library.with(|l| l.roots.clone());
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
                    .bg(muted())
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
        "Zeo Player 会扫描这些文件夹里的 MP3、FLAC 和 WAV，包括子文件夹。也可以把文件夹拖到这一节上。",
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
        let results: Vec<String> = event.uris.iter().map(|uri| add_as_root(&url_to_path(uri))).collect();
        message.set(results.join("\n"));
    })
    .child(view().gap(px(8.0)).child(rows))
    .child(note(message))
}

fn library_section() -> View {
    let s = store();
    let progress = move || s.library.with(|l| l.progress.clone());
    let scanning = move || s.library.with(|l| l.scanning);
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
            super::super::store::rebuild,
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
fn delete_section() -> View {
    let s = store();
    setting_section("删除文件", "曲目右键菜单「删除文件…」的行为。", None).child(pref_row(
        "直接删除文件",
        "开启后删除文件不经过回收站（不可恢复）；关闭时文件先移到回收站。",
        move || s.library.with(|l| l.delete_permanently),
        super::super::store::set_delete_permanently,
    ))
}

/// 封面来源：本地同名图片 / 内嵌封面的优先序。
fn covers_source_section() -> View {
    let s = store();
    setting_section(
        "封面来源",
        "封面可以来自曲目同目录的同名图片，或音频文件的内嵌封面。",
        None,
    )
    .child(pref_row(
        "本地图片优先",
        "使用同目录的同名图片（jpg / jpeg / png / webp）作为封面；关闭则优先内嵌封面。",
        move || s.library.with(|l| l.prefer_local_covers),
        super::super::store::set_prefer_local_covers,
    ))
}

/// 「本地优先」开关行：标题 + 说明 + 右侧开关。
fn pref_row(
    title: &'static str,
    desc: &'static str,
    checked: impl Fn() -> bool + 'static,
    set: fn(bool),
) -> View {
    let toggle = nsh_base::switch()
        .checked(checked)
        .accessibility_label(title)
        .on_change(move |next: &bool| set(*next))
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
                    text(title)
                        .text_size(px(14.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg()),
                )
                .child(
                    text(desc)
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(toggle)
}

/// 文件夹监视开关与状态。
fn watch_row() -> View {
    let s = store();
    let on = move || s.library.with(|l| l.watching);
    let status = move || {
        s.library.with(|l| {
            let w = &l.watch;
            if !l.watching {
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
        .on_change(move |next: &bool| super::super::store::set_watch(*next))
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
                            if s.library.with(|l| l.watching && l.watch.error.is_some()) {
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

/// 常用分隔符（可直接点选）。
const PRESETS: &[(&str, &str)] = &[
    (";", "分号"),
    ("/", "斜杠"),
    (",", "逗号"),
    ("、", "顿号"),
    ("|", "竖线"),
    ("&", "和号"),
];

fn separator_label(sep: &str) -> String {
    match sep {
        " " => "空格".into(),
        s if s.trim().is_empty() => format!("{:?}", s),
        s => s.to_owned(),
    }
}

fn genres_section() -> View {
    let s = store();
    let draft = signal(String::new());
    let separators = move || s.library.with(|l| l.separators.clone());
    let toggle = move |sep: String| {
        let mut list = separators();
        if let Some(i) = list.iter().position(|x| *x == sep) {
            list.remove(i);
        } else {
            list.push(sep);
        }
        super::super::store::set_separators(list);
    };
    let add_custom = move || {
        let sep = draft.get_untracked();
        if sep.is_empty() {
            return;
        }
        let mut list = separators();
        if !list.contains(&sep) {
            list.push(sep);
            super::super::store::set_separators(list);
        }
        draft.set(String::new());
    };

    let chip = move |sep: String, caption: Option<&'static str>| {
        let on = {
            let sep = sep.clone();
            move || separators().contains(&sep)
        };
        let press = sep.clone();
        nsh_base::button()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .h(px(32.0))
            .px(px(12.0))
            .rounded_full()
            .border_1()
            .border_color(border())
            .selected(on)
            .selected_style(|st| {
                st.border_color(move || primary())
                    .bg(move || primary().opacity(0.12))
            })
            .transition_colors()
            .duration(300.0)
            .accessibility_label(caption.unwrap_or("自定义分隔符"))
            .hover(|st| st.bg(hover()))
            .active(|st| st.opacity(0.7))
            .on_press(move |_: &()| toggle(press.clone()))
            .child(
                text(separator_label(&sep))
                    .text_size(px(14.0))
                    .line_height(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(fg()),
            )
            .child(caption.map(|c| {
                text(c)
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(muted_fg())
            }))
    };

    let chips = dynamic(move || {
        let current = separators();
        let mut out: Vec<AnyElement> = PRESETS
            .iter()
            .map(|(sep, name)| chip((*sep).to_owned(), Some(*name)).into_element())
            .collect();
        for sep in current
            .iter()
            .filter(|c| !PRESETS.iter().any(|(p, _)| p == c))
        {
            out.push(
                chip(sep.clone(), None)
                    .child(icon(Icon::Close, 11.0, muted_fg))
                    .into_element(),
            );
        }
        out
    });

    let custom = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            super::super::components::form::input_field(true)
                .w(px(160.0))
                .value(move || draft.get())
                .placeholder("其它分隔符，如 ・")
                .on_change(move |text: &str| draft.set(text.to_owned()))
                .on_submit(move |_: &str| add_custom()),
        )
        .child(outline_button(Icon::Plus, "添加", add_custom))
        .child(view().flex_1())
        .child(text_button(
            Variant::Ghost,
            Size::Sm,
            None,
            "恢复默认",
            || {
                super::super::store::set_separators(
                    crate::config::DEFAULT_SEPARATORS
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect(),
                )
            },
        ));

    // 预览：曲库里最常见的、含多个值的原始流派标签，按当前分隔符拆开。
    let samples = computed(move || {
        let tracks = s.tracks.get();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for t in tracks.iter() {
            if let Ok(meta) =
                crate::metadata::MetaCache::global().get(std::path::Path::new(&t.path))
            {
                for raw in &meta.genres {
                    *counts.entry(raw.clone()).or_default() += 1;
                }
            }
        }
        let mut list: Vec<(String, usize)> = counts.into_iter().collect();
        let busy = |raw: &str| {
            raw.chars()
                .filter(|c| !c.is_alphanumeric() && !c.is_whitespace())
                .count()
        };
        list.sort_by(|a, b| {
            busy(&b.0)
                .cmp(&busy(&a.0))
                .then(b.1.cmp(&a.1))
                .then(a.0.cmp(&b.0))
        });
        list.into_iter()
            .take(4)
            .map(|(raw, _)| raw)
            .collect::<Vec<_>>()
    });
    let preview = dynamic(move || {
        let seps = separators();
        let rows: Vec<AnyElement> = samples
            .get()
            .into_iter()
            .map(|raw| {
                let parts = crate::config::split_genres(std::slice::from_ref(&raw), &seps);
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0))
                    .min_h(px(32.0))
                    .child(
                        text(raw)
                            .number_of_lines(1)
                            .w(px(220.0))
                            .text_size(px(13.0))
                            .line_height(px(13.0))
                            .text_color(muted_fg()),
                    )
                    .child(icon(Icon::ChevronRight, 12.0, faint))
                    .child(
                        view()
                            .flex_1()
                            .flex_row()
                            .flex_wrap()
                            .gap(px(6.0))
                            .children(parts.into_iter().map(|g| {
                                view()
                                    .rounded_full()
                                    .bg(muted())
                                    .px(px(10.0))
                                    .py(px(3.0))
                                    .child(
                                        text(g)
                                            .text_size(px(12.0))
                                            .line_height(px(12.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(fg()),
                                    )
                            })),
                    )
                    .into_element()
            })
            .collect();
        (!rows.is_empty()).then(|| {
            view()
                .gap(px(4.0))
                .p(px(12.0))
                .rounded(px(10.0))
                .bg(move || bg())
                .child(
                    text(move || {
                        let total: HashSet<String> = s
                            .tracks
                            .with(|t| t.iter().flat_map(|t| t.genres.iter().cloned()).collect());
                        format!("预览 · 当前共 {} 个标签", total.len())
                    })
                    .text_size(px(11.0))
                    .line_height(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .letter_spacing(px(0.6))
                    .text_color(faint())
                    .pb(px(4.0)),
                )
                .children(rows)
        })
    });

    setting_section(
        "标签",
        "标签解析自曲目的流派（genre）信息。一个流派里有多个值时，按下面选中的分隔符拆开，并去掉首尾空格。修改后立即生效，不用重新扫描。",
        None,
    )
    .child(view().flex_row().flex_wrap().gap(px(8.0)).child(chips))
    .child(custom)
    .child(preview)
}

/// 过滤：低于时长 / 大小阈值的音频不进曲库（改后自动重新扫描）。
fn filter_section() -> View {
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
               field: fn(&super::super::store::LibraryState) -> u32,
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
                super::super::components::radio::group_row().children(options.iter().map(
                    |&(value, name)| {
                        super::super::components::radio::item(
                            move || s.library.with(|l| field(l) == value),
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
        |l| l.min_duration_secs,
        super::super::store::set_min_duration_secs,
    ))
    .child(row(
        "最小大小",
        SIZES,
        |l| l.min_size_kb,
        super::super::store::set_min_size_kb,
    ))
}

/// 外观：跟随系统 / 固定浅色 / 固定深色（同一行右侧的下拉）。
fn appearance_section() -> View {
    setting_section(
        "外观",
        "跟随系统的浅色 / 深色外观，或为界面固定一种。",
        Some(appearance_select()),
    )
}

/// 「外观」的下拉：触发器显示当前值，菜单列三个带图标的选项。
fn appearance_select() -> AnyElement {
    use crate::config::Appearance;
    use nsh_base::menu;
    let s = store();
    const OPTIONS: &[(Appearance, Icon, &str)] = &[
        (Appearance::System, Icon::Monitor, "系统"),
        (Appearance::Light, Icon::Sun, "浅色"),
        (Appearance::Dark, Icon::Moon, "深色"),
    ];
    let current_mode = move || {
        OPTIONS
            .iter()
            .find(|(m, _, _)| *m == s.library.with(|l| l.appearance))
            .copied()
    };
    let current = move || current_mode().map_or("系统", |(_, _, l)| l).to_owned();
    let trigger = super::super::components::form::select_trigger(
        current,
        move || current_mode().map(|(_, g, _)| g),
        "外观",
        110.0,
        "",
    );
    let mut menu = menu()
        .offset(px(4.0))
        .margin(px(8.0))
        .popup_style(|st| {
            st.min_w(px(140.0))
                .p(px(4.0))
                .rounded(px(10.0))
                .overflow_hidden()
                .border_1()
                .border_color(border())
                .bg(nsh_base::theme().popover)
                .shadow_lg()
        })
        .trigger(trigger);
    for &(mode, glyph, label) in OPTIONS {
        menu = menu.child(super::playlists::item(label, Some(glyph), move || {
            super::super::store::set_appearance(mode)
        }));
    }
    menu.into_element()
}

/// 主题色：一排预设色 + 跟随当前曲目封面（强调色，立即生效）。
fn accent_section() -> View {
    const OPTIONS: &[(ThemeColor, &str)] = &[
        (ThemeColor::Neutral, "黑白"),
        (ThemeColor::Red, "红"),
        (ThemeColor::Orange, "橙"),
        (ThemeColor::Green, "绿"),
        (ThemeColor::Blue, "蓝"),
        (ThemeColor::Purple, "紫"),
        (ThemeColor::Cover, "封面"),
    ];
    let mut swatches = view().flex_row().flex_wrap().gap(px(8.0));
    for &(mode, label) in OPTIONS {
        swatches = swatches.child(accent_swatch(mode, label));
    }
    setting_section(
        "主题色",
        "按钮与选中态的强调色：默认跟随当前曲目封面（并轻调界面底色），也可钉住黑白或预设色。",
        Some(swatches.into_element()),
    )
}

/// 主题色色块：圆形预设色，选中时显示对勾。「封面」块未生效时为灰底
/// 音符，生效后实时显示当前封面的主色调。
fn accent_swatch(mode: ThemeColor, label: &'static str) -> nsh_base::ButtonElement {
    use super::super::store::accent;
    let preset = preset_accent(mode);
    let on = move || store().library.with(|l| l.theme_color) == mode;
    nsh_base::button()
        .size(px(24.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .bg(move || match mode {
            ThemeColor::Neutral => nsh_base::theme().primary,
            // 封面色：只在本选项生效时实时显示，否则灰底。
            ThemeColor::Cover => accent().filter(|_| on()).unwrap_or_else(muted),
            _ => preset.unwrap(),
        })
        .transition_colors()
        .duration(300.0)
        .accessibility_label(SharedString::from(format!("主题色：{label}")))
        .selected(on)
        .hover(|st| st.opacity(0.85))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| set_theme_color(mode))
        .child(
            icon_dyn(
                move || {
                    if on() { Icon::Check } else { Icon::Note }
                },
                12.0,
                move || {
                    if !on() {
                        fg()
                    } else if mode == ThemeColor::Neutral {
                        nsh_base::theme().primary_foreground
                    } else {
                        AdaptiveColor::rgb(0xffffff, 0xffffff)
                    }
                },
            )
            .opacity(move || (on() || mode == ThemeColor::Cover) as i32 as f32),
        )
}

/// 字体：系统字体或任一已安装字体（同一行右侧的下拉，带搜索与逐字体预览）。
/// 字体列表懒加载：打开弹层才读取已安装字体；关闭或离开设置页时整块销毁
/// （列表数据、读取任务、各行的字体预览一起释放）。
fn font_section() -> View {
    setting_section(
        "字体",
        "整个界面（包括歌词）换用所选字体，立即生效。",
        Some(font_select()),
    )
}

/// 「字体」的下拉：触发器用当前字体写自己的名字；弹层里是搜索框 +
/// 「系统字体」+ 已安装字体列表。`open` 由这里持有（选一个后关上）。
fn font_select() -> AnyElement {
    use nsh_base::{PopoverEvent, popover};
    let s = store();
    let open = signal(false);
    let current = move || s.library.with(|l| l.font.clone());
    // 触发器是普通 view（popover 接管按压）：select_trigger 的外壳 +
    // chevron，名字用所选字体渲染，所见即所得。
    let trigger = super::super::components::form::trigger_shell("字体", 200.0)
        .child(
            text(move || current().unwrap_or_else(|| "系统字体".to_owned()))
                .flex_1()
                .min_w(px(0.0))
                .number_of_lines(1)
                .text_size(px(14.0))
                .line_height(px(14.0))
                .font_family(move || {
                    SharedString::from(current().unwrap_or_else(|| "system-ui".to_owned()))
                })
                .text_color(fg()),
        )
        .child(super::super::components::form::trigger_chevron());

    popover()
        .open(move || Some(open.get()))
        .on_open_change(move |e: &PopoverEvent| {
            if let PopoverEvent::OpenChange(next, _) = e {
                open.set(*next);
            }
        })
        .placement(Placement::Bottom)
        .align(Align::End)
        .offset(px(4.0))
        .margin(px(8.0))
        .popup_style(|st| {
            st.w(px(300.0))
                .p(px(4.0))
                .rounded(px(10.0))
                .overflow_hidden()
                .border_1()
                .border_color(border())
                .bg(nsh_base::theme().popover)
                .shadow_lg()
        })
        .trigger(trigger)
        // 面板随 open 建 / 拆：打开才枚举已安装字体，关闭整块释放。
        .child(when(move || open.get(), move || font_panel(open)))
        .into_element()
}

/// 字体选项的一行：显示名用它自己的字体渲染 + 「歌词 Aa あ」样例，
/// 选中行有主题色底与对勾。
fn font_choice(
    label: impl Fn() -> String + Copy + 'static,
    family: impl Fn() -> String + Copy + 'static,
    selected: impl Fn() -> bool + Copy + 'static,
    press: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    nsh_base::button()
        .h(px(40.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .accessibility_label(move || SharedString::from(label()))
        .selected(selected)
        .selected_style(|st| st.bg(move || primary().opacity(0.12)))
        .transition_colors()
        .duration(300.0)
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| press())
        .child(
            text(move || format!("{} · 歌词 Aa あ", label()))
                .flex_1()
                .min_w(px(0.0))
                .number_of_lines(1)
                .font_family(move || SharedString::from(family()))
                .text_size(px(14.0))
                .line_height(px(14.0))
                .text_color(fg()),
        )
        .child(when(selected, || icon(Icon::Check, 14.0, primary)))
}

/// 「字体」弹层：搜索框、「系统字体」、已安装字体（懒加载：打开才读，
/// 关闭整块销毁）。
fn font_panel(open: Signal<bool>) -> View {
    let s = store();
    let query = signal(String::new());
    let current = move || s.library.with(|l| l.font.clone());
    // None：还在读取。任务属于这一块，块销毁时一起取消。
    let installed = signal(None::<Same<Vec<String>>>);
    spawn(async move {
        if let Ok(list) = blocking(super::super::fonts::installed_families).await {
            installed.set(Some(Same(std::sync::Arc::new(list))));
        }
    });
    let loading = move || installed.with(Option::is_none);

    // 按搜索词过滤。
    let rows = computed(move || {
        let q = query.get().trim().to_lowercase();
        let out: Vec<String> = installed.with(|list| {
            list.iter()
                .flat_map(|list| list.iter())
                .filter(|f| q.is_empty() || f.to_lowercase().contains(&q))
                .cloned()
                .collect()
        });
        Rc::new(out)
    });
    let count = computed(move || rows.with(|r| r.len()));

    let list = virtual_list(count, move |at| {
        let family =
            computed(move || rows.with(|r| r.get(at.index()).cloned().unwrap_or_default()));
        let family = move || family.get();
        let selected = move || current().as_deref() == Some(family().as_str());
        font_choice(family, family, selected, move || {
            super::super::store::set_font(Some(family()));
            open.set(false);
        })
    })
    .estimated_item_size(px(40.0))
    .empty(
        view().h(px(64.0)).items_center().justify_center().child(
            text(move || {
                if loading() {
                    "正在读取字体…"
                } else {
                    "没有匹配的字体"
                }
            })
            .text_size(px(13.0))
            .line_height(px(13.0))
            .text_color(muted_fg()),
        ),
    )
    .flex_1();

    view()
        .gap(px(4.0))
        .child(
            super::super::components::form::input_field(true)
                .w_full()
                .value(move || query.get())
                .placeholder("搜索已安装的字体")
                .on_change(move |text: &str| query.set(text.to_owned())),
        )
        .child(font_choice(
            || "系统字体".to_owned(),
            || "system-ui".to_owned(),
            move || current().is_none(),
            move || {
                super::super::store::set_font(None);
                open.set(false);
            },
        ))
        .child(view().h(px(1.0)).bg(border()))
        .child(view().h(px(216.0)).child(list))
}

/// 播放：淡入淡出时长。
fn playback_section() -> View {
    let s = store();
    const STEPS: &[(u32, &str)] = &[
        (0, "关闭"),
        (100, "0.1 秒"),
        (200, "0.2 秒"),
        (400, "0.4 秒"),
        (800, "0.8 秒"),
        (1500, "1.5 秒"),
    ];
    setting_section(
        "淡入淡出",
        "播放、暂停、拖动进度和切歌时，音量在这段时间里渐变，不会突然响起或戛然而止。一首自然播完接下一首时不淡变。",
        None,
    )
    .child(super::super::components::radio::group_row().children(STEPS.iter().map(|&(ms, label)| {
        super::super::components::radio::item(move || s.library.with(|l| l.fade_ms == ms), label, move || super::super::store::set_fade_ms(ms))
    })))
}

/// 全屏歌词的对齐方式。
fn lyrics_section() -> View {
    use crate::config::LyricsAlign;
    let s = store();
    const OPTIONS: &[(LyricsAlign, &str)] = &[
        (LyricsAlign::Left, "左对齐"),
        (LyricsAlign::Center, "居中"),
        (LyricsAlign::Right, "右对齐"),
    ];
    setting_section("歌词对齐", "全屏播放页里歌词的对齐方式。", None).child(
        super::super::components::radio::group_row().children(OPTIONS.iter().map(
            |&(align, label)| {
                super::super::components::radio::item(
                    move || s.library.with(|l| l.lyrics_align == align),
                    label,
                    move || super::super::store::set_lyrics_align(align),
                )
            },
        )),
    )
}

/// 歌词来源：本地同名 .lrc / 内嵌歌词的优先序。
fn lyrics_source_section() -> View {
    let s = store();
    setting_section(
        "歌词来源",
        "歌词可以来自曲目同目录的同名 .lrc 文件，或音频文件的内嵌歌词。",
        None,
    )
    .child(pref_row(
        "本地歌词优先",
        "使用同目录的同名 .lrc 文件作为歌词；关闭则优先内嵌歌词。",
        move || s.library.with(|l| l.prefer_local_lyrics),
        super::super::store::set_prefer_local_lyrics,
    ))
}

/// 关闭窗口时：每次询问 / 最小化到托盘 / 退出。
fn window_section() -> View {
    use crate::config::CloseAction;
    let s = store();
    let option = move |action: CloseAction, label: &'static str, hint: &'static str| {
        super::super::components::radio::card(
            move || s.library.with(|l| l.close_action == action),
            label,
            hint,
            move || super::super::store::set_close_action(action),
        )
    };
    let supported = cfg!(any(target_os = "macos", target_os = "windows"));
    setting_section(
        "关闭窗口时",
        if supported {
            "最小化到托盘后音乐继续播放，界面释放以节省内存；托盘图标可以打开窗口、播放 / 暂停、切歌和退出。"
        } else {
            "这个平台还没有托盘，关闭窗口即退出。"
        },
        None,
    )
    .child(supported.then(|| {
        super::super::components::radio::group_cards()
            .child(option(CloseAction::Ask, "每次询问", "第一次关闭时选择"))
            .child(option(CloseAction::Tray, "最小化到托盘", "继续播放"))
            .child(option(CloseAction::Quit, "退出", "停止播放并退出"))
    }))
}

// --- 弹窗 -------------------------------------------------------------------

/// 设置的分类（左栏）。选择在 `global` 信号里，关上重开时保留。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Pane {
    #[default]
    Appearance,
    Library,
    Playback,
    Lyrics,
    Covers,
    Window,
    Tags,
}

impl Pane {
    /// 左栏分组（小标题 + 分类，不折叠）：通用是外观 / 曲库 / 播放 /
    /// 窗口；歌词、封面、标签这些描述曲目信息的归「元数据」。
    const GROUPS: [(&'static str, &'static [Pane]); 2] = [
        (
            "通用",
            &[
                Pane::Appearance,
                Pane::Library,
                Pane::Playback,
                Pane::Window,
            ],
        ),
        ("元数据", &[Pane::Lyrics, Pane::Covers, Pane::Tags]),
    ];

    /// 全部分类（显示顺序）。
    fn all() -> impl Iterator<Item = Pane> {
        Pane::GROUPS
            .iter()
            .flat_map(|(_, panes)| panes.iter().copied())
    }

    fn title(self) -> &'static str {
        match self {
            Pane::Appearance => "外观",
            Pane::Library => "曲库",
            Pane::Playback => "播放",
            Pane::Lyrics => "歌词",
            Pane::Covers => "封面",
            Pane::Window => "窗口",
            Pane::Tags => "标签",
        }
    }

    fn glyph(self) -> Icon {
        match self {
            Pane::Library => Icon::Folder,
            Pane::Playback => Icon::Play,
            Pane::Lyrics => Icon::Note,
            Pane::Covers => Icon::Disc,
            Pane::Window => Icon::Monitor,
            Pane::Tags => Icon::Tag,
            Pane::Appearance => Icon::Sun,
        }
    }
}

/// 一节设置与它的搜索词：左栏搜索按 `title` / `keys` 命中（分类名命中时
/// 该分类的节全部算命中）。
struct Section {
    pane: Pane,
    title: &'static str,
    keys: &'static str,
    build: fn() -> View,
}

const SECTIONS: &[Section] = &[
    Section {
        pane: Pane::Library,
        title: "音乐文件夹",
        keys: "添加 文件夹 目录 拖放 根目录 mp3 flac wav",
        build: folders_section,
    },
    Section {
        pane: Pane::Library,
        title: "曲库",
        keys: "扫描更新 重建曲库 索引 监视 文件夹变化 自动 统计 曲目 专辑 歌手 总时长 缓存",
        build: library_section,
    },
    Section {
        pane: Pane::Library,
        title: "音频过滤",
        keys: "过滤 阈值 时长 大小 短音频 小文件 秒 KB 采样 铃声",
        build: filter_section,
    },
    Section {
        pane: Pane::Library,
        title: "删除文件",
        keys: "删除 文件 回收站 永久删除 直接删除 曲目 右键 菜单",
        build: delete_section,
    },
    Section {
        pane: Pane::Covers,
        title: "封面来源",
        keys: "封面 图片 本地 同名 内嵌 优先 来源 cover jpg jpeg png webp 专辑",
        build: covers_source_section,
    },
    Section {
        pane: Pane::Playback,
        title: "淡入淡出",
        keys: "播放 音量 渐变 切歌 暂停",
        build: playback_section,
    },
    Section {
        pane: Pane::Lyrics,
        title: "歌词对齐",
        keys: "歌词 全屏 左对齐 居中 右对齐 对齐方式",
        build: lyrics_section,
    },
    Section {
        pane: Pane::Lyrics,
        title: "歌词来源",
        keys: "歌词 本地 lrc 同名 内嵌 优先 来源",
        build: lyrics_source_section,
    },
    Section {
        pane: Pane::Window,
        title: "关闭窗口时",
        keys: "关闭 窗口 最小化 托盘 退出 询问",
        build: window_section,
    },
    Section {
        pane: Pane::Tags,
        title: "标签",
        keys: "流派 标签 分隔符 分号 顿号 斜杠 拆分",
        build: genres_section,
    },
    Section {
        pane: Pane::Appearance,
        title: "外观",
        keys: "浅色 深色 系统 主题 颜色 跟随",
        build: appearance_section,
    },
    Section {
        pane: Pane::Appearance,
        title: "主题色",
        keys: "主题色 颜色 强调色 黑白 红 橙 绿 蓝 紫 封面 自动",
        build: accent_section,
    },
    Section {
        pane: Pane::Appearance,
        title: "字体",
        keys: "字体 系统字体 已安装 预览 字形",
        build: font_section,
    },
];

/// 搜索词是否命中一段文本（调用方已把 `q` 转成小写并去掉两端空格）。
fn matches(q: &str, s: &str) -> bool {
    s.to_lowercase().contains(q)
}

/// 分类命中：名字匹配，或它任一节的标题 / 关键词匹配。
fn pane_hits(q: &str, p: Pane) -> bool {
    matches(q, p.title())
        || SECTIONS
            .iter()
            .any(|s| s.pane == p && (matches(q, s.title) || matches(q, s.keys)))
}

/// 一节在右栏显示：分类名命中时整节显示，否则按自身标题 / 关键词。
fn section_hits(q: &str, s: &Section) -> bool {
    matches(q, s.pane.title()) || matches(q, s.title) || matches(q, s.keys)
}

/// 左栏顶部的搜索框（外观同主侧栏的搜索框；有词时出现清空按钮）。
fn search_box(query: Signal<String>) -> View {
    let empty = move || query.with(String::is_empty);
    view()
        .h(px(28.0))
        .flex_row()
        .items_center()
        .pl(px(10.0))
        .pr(px(1.0))
        .rounded(px(14.0))
        .border_1()
        .border_color(border())
        .bg(move || bg())
        .child(icon(Icon::Search, 13.0, faint).flex_shrink_0())
        .child(
            input()
                .flex_1()
                .min_w(px(0.0))
                .h(px(26.0))
                .px(px(6.0))
                .text_size(px(13.0))
                .text_color(fg())
                .value(move || query.get())
                .placeholder("搜索")
                .label("搜索设置")
                .on_change(move |t: &str| query.set(t.to_owned())),
        )
        .child(when(
            move || !empty(),
            move || {
                icon_button(
                    || Icon::Close,
                    10.0,
                    "清除搜索",
                    faint,
                    move || query.set(String::new()),
                )
                .flex_shrink_0()
            },
        ))
}

/// 左栏分类行（外观同主侧栏的导航行）。
fn pane_row(p: Pane, pane: Signal<Pane>) -> nsh_base::ButtonElement {
    let active = move || pane.get() == p;
    nsh_base::button()
        .h(px(32.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(10.0))
        .rounded(px(8.0))
        .accessibility_label(p.title())
        .selected(active)
        .selected_style(|st| st.bg(move || primary().opacity(0.12)))
        .hover(|st| st.bg(move || primary().opacity(0.08)))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| pane.set(p))
        .child(icon(p.glyph(), 15.0, move || {
            if active() { primary() } else { sidebar_fg() }
        }))
        .child(
            text(p.title())
                .text_size(px(13.0))
                .line_height(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if active() { fg() } else { sidebar_fg() }),
        )
}

/// 搜索结果行：节的标题 + 所属分类，按下选中该分类（保留搜索词，右栏
/// 只显示命中的节）。
fn result_row(s: &'static Section, pane: Signal<Pane>) -> nsh_base::ButtonElement {
    nsh_base::button()
        .h(px(40.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(10.0))
        .rounded(px(8.0))
        .accessibility_label(s.title)
        .hover(|st| st.bg(move || primary().opacity(0.08)))
        .active(|st| st.opacity(0.7))
        .on_press(move |_: &()| pane.set(s.pane))
        .child(icon(s.pane.glyph(), 15.0, sidebar_fg))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(1.0))
                .child(
                    text(s.title)
                        .text_size(px(13.0))
                        .line_height(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(sidebar_fg()),
                )
                .child(
                    text(s.pane.title())
                        .text_size(px(11.0))
                        .line_height(px(11.0))
                        .text_color(faint()),
                ),
        )
}

/// 左栏分组的小标题（首个与搜索框之间不留大间隔，其余组间拉开）。
fn group_heading(name: &'static str, spaced: bool) -> View {
    view()
        .px(px(10.0))
        .pt(if spaced { px(18.0) } else { px(4.0) })
        .pb(px(4.0))
        .child(
            text(name)
                .text_size(px(11.0))
                .line_height(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(faint()),
        )
}

/// 左栏：顶部搜索框，下面是分类列表（有搜索词时为命中的节列表）。
fn nav_column(pane: Signal<Pane>, query: Signal<String>) -> View {
    let rows = dynamic(move || {
        let q = query.get().trim().to_lowercase();
        if q.is_empty() {
            return Pane::GROUPS
                .iter()
                .enumerate()
                .flat_map(|(i, &(name, panes))| {
                    std::iter::once(group_heading(name, i > 0).into_element())
                        .chain(panes.iter().map(|&p| pane_row(p, pane).into_element()))
                })
                .collect::<Vec<_>>();
        }
        SECTIONS
            .iter()
            .filter(|s| section_hits(&q, s))
            .map(|s| result_row(s, pane).into_element())
            .collect()
    });
    view()
        .w(px(220.0))
        .flex_shrink_0()
        .bg(move || sidebar())
        .transition_colors()
        .duration(300.0)
        .test_id("settings-nav")
        .child(view().p(px(10.0)).child(search_box(query)))
        .child(
            scroll()
                .flex_1()
                .child(view().px(px(10.0)).pb(px(10.0)).gap(px(2.0)).child(rows)),
        )
}

/// 右栏：当前分类的标题与关闭按钮，下面是它的各节；有搜索词时只显示
/// 命中的节，所选分类没命中时落到第一个有命中的分类。
fn content_column(pane: Signal<Pane>, query: Signal<String>) -> View {
    let s = store();
    let shown = move || {
        let q = query.get().trim().to_lowercase();
        let hits: Vec<Pane> = Pane::all().filter(|p| pane_hits(&q, *p)).collect();
        let cur = pane.get();
        hits.iter()
            .find(|&&p| p == cur)
            .or(hits.first())
            .copied()
            .map(|p| (p, q))
    };
    view()
        .flex_1()
        .min_w(px(0.0))
        .test_id("settings-content")
        .child(dynamic(move || {
            let Some((p, q)) = shown() else {
                return view()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .p(px(32.0))
                    .child(icon(Icon::Search, 24.0, faint))
                    .child(
                        text("没有匹配的设置")
                            .text_size(px(14.0))
                            .line_height(px(14.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg()),
                    )
                    .child(
                        text("换个关键词，或清空搜索看全部分类。")
                            .text_size(px(12.0))
                            .line_height(px(12.0))
                            .text_color(muted_fg()),
                    )
                    .into_element();
            };
            view()
                .flex_1()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .h(px(52.0))
                        .pl(px(24.0))
                        .pr(px(14.0))
                        .border_b_1()
                        .border_b_color(border())
                        .child(
                            text(p.title())
                                .text_size(px(15.0))
                                .line_height(px(15.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(fg()),
                        )
                        .child(view().flex_1())
                        .child(icon_button(
                            || Icon::Close,
                            14.0,
                            "关闭设置",
                            muted_fg,
                            move || s.settings.set(false),
                        )),
                )
                .child(
                    scroll().flex_1().child(
                        view()
                            .px(px(24.0))
                            .pt(px(20.0))
                            .pb(px(28.0))
                            .gap(px(36.0))
                            .children(
                                SECTIONS
                                    .iter()
                                    .filter(|sec| sec.pane == p && section_hits(&q, sec))
                                    .map(|sec| (sec.build)()),
                            ),
                    ),
                )
                .into_element()
        }))
}

/// 设置弹窗（nsh-base `dialog`，Esc / 点遮罩关闭）。
pub(crate) fn dialog() -> AnyElement {
    let pane = global::<Pane>();
    let query = signal(String::new());
    let content = view()
        .w(px(920.0))
        .max_w(move || px((window().width.0 - 32.0).max(400.0)))
        .h(px(640.0))
        .max_h(move || px((window().height.0 - 60.0).max(400.0)))
        .flex_row()
        .overflow_hidden()
        .rounded(px(super::super::components::form::RADIUS_LG))
        .border_1()
        .border_color(border())
        .bg(move || bg())
        .transition_colors()
        .duration(300.0)
        .shadow_lg()
        // 点在内容上不会穿到背后的遮罩。
        .on_press(|| {})
        .child(nav_column(pane, query))
        .child(view().w(px(1.0)).bg(border()).flex_shrink_0())
        .child(content_column(pane, query));
    super::super::components::form::modal(content, move || store().settings.set(false))
}

/// 打开设置弹窗并选中 `pane`（侧栏「设置」只置 `Store::settings`，保留
/// 上次的选择；空曲库的「添加文件夹…」要落在曲库分类所以走这里）。
pub(super) fn open(pane: Pane) {
    global::<Pane>().set(pane);
    store().settings.set(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_decode() {
        assert_eq!(
            url_to_path("file:///Users/me/My%20Music/"),
            "/Users/me/My Music/"
        );
        assert_eq!(url_to_path("file:///tmp/%E9%9F%B3%E4%B9%90"), "/tmp/音乐");
        assert_eq!(
            url_to_path("file:///C:/Users/me/My%20Music"),
            "C:\\Users\\me\\My Music"
        );
        assert_eq!(last_segment("C:\\Users\\me\\Music\\"), "Music");
        assert_eq!(last_segment("/Users/me/Music"), "Music");
    }
}
