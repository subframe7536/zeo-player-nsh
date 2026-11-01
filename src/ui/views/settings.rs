//! 「设置」页：各设置节（曲库 / 播放 / 歌词 / 窗口 / 标签 / 外观 / 字体），
//! 无卡片包装，节与节之间靠页面间距分开。

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use nsh_core::prelude::*;

use super::super::components::button::{
    Size, Variant, icon_button, outline_button, pill_button, text_button,
};
use super::super::components::icons::{Icon, icon};
use super::super::components::page::page_header;
use super::super::store::{Same, format_total, scan, store};
use super::super::theme::*;
use super::{SEP, last_segment, total_duration};
use crate::config::library_store;

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
                            .bg(bg())
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
                            .bg(primary())
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
                .checked(|st| st.bg(primary()).border_color(primary()))
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
            .selected_style(|st| st.border_color(primary()).bg(primary().opacity(0.12)))
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
                .bg(bg())
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
        "流派标签",
        "一个流派标签里有多个值时，按下面选中的分隔符拆开，并去掉首尾空格。修改后立即生效，不用重新扫描。",
        None,
    )
    .child(view().flex_row().flex_wrap().gap(px(8.0)).child(chips))
    .child(custom)
    .child(preview)
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
        .selected_style(|st| st.bg(primary().opacity(0.12)))
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

#[component]
pub fn Settings() -> impl IntoElement {
    view()
        .flex_1()
        .child(page_header(
            || "设置".into(),
            || "曲库、播放、窗口、标签、外观与字体".into(),
            None,
            None,
        ))
        .child(
            scroll().flex_1().child(
                view()
                    .px(px(32.0))
                    .pt(px(8.0))
                    .pb(px(BAR_SPACE))
                    .gap(px(36.0))
                    .max_w(px(820.0))
                    .child(folders_section())
                    .child(library_section())
                    .child(playback_section())
                    .child(lyrics_section())
                    .child(window_section())
                    .child(genres_section())
                    .child(appearance_section())
                    .child(font_section()),
            ),
        )
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
