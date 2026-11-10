//! 设置 →「快捷键」：全部动作的可自定义键位（覆盖存 `library.json`
//! 的 `keybinds`，改后立即生效）。点键位胶囊进入捕获态，按下的下一个
//! 组合键即新绑定；撞上已有键位时不顶绑——同键双方都不能触发、红字
//! 置顶，改掉任一方后原绑定自动恢复。

use std::collections::BTreeSet;
use std::time::Duration;

use nsh_core::keymap::When;
use nsh_core::prelude::*;

use super::search_box;
use zeo_ui_components::button::{Size, Variant, icon_button, text_button};
use zeo_ui_components::icons::Icon;
use zeo_ui_store::theme::*;
use zeo_ui_store::{KeyAction, key_label_of, store};

/// 「按键搜索」录制态的临时绑定：`When::Always` 让输入框聚焦时文本键
/// 也被录制而不是输进框里；按下的组合键以平台标签（`⌘→` / `Ctrl+→`）
/// 写回搜索框（一次只录一组键），Esc 取消、Backspace / Delete 清空。
fn record_search_keys(recording: Signal<bool>, query: Signal<String>) {
    let always = KeyOptions {
        when: When::Always,
        ..KeyOptions::default()
    };
    for key in zeo_ui_store::capture_candidates() {
        on_key_with(&key, always, move |k| {
            query.set(key_label(k));
            recording.set(false);
        });
    }
    on_key_with("escape", always, move |_| recording.set(false));
    for key in ["backspace", "delete"] {
        on_key_with(key, always, move |_| {
            query.set(String::new());
            recording.set(false);
        });
    }
}

/// 捕获态注册的临时绑定：`on_key` 是最新绑定优先，这些盖过全局快捷键
/// （含全屏页的）；Esc 取消、Backspace / Delete 解绑——最后注册，优先。
fn capture_keys(capture: Signal<Option<KeyAction>>) {
    for key in zeo_ui_store::capture_candidates() {
        on_key(&key, move |k| {
            if let Some(action) = capture.get_untracked() {
                zeo_ui_store::set_keybind(action, Some(k.to_owned()));
            }
            capture.set(None);
        });
    }
    on_key("escape", move |_| capture.set(None));
    for key in ["backspace", "delete"] {
        on_key(key, move |_| {
            if let Some(action) = capture.get_untracked() {
                zeo_ui_store::set_keybind(action, Some(String::new()));
            }
            capture.set(None);
        });
    }
}

/// 一个动作的键位胶囊：常态显示当前键位（未绑定为「未绑定」）；捕获中
/// 变「按下新快捷键…」；冲突中红框红字。
fn key_pill(
    action: KeyAction,
    capture: Signal<Option<KeyAction>>,
    recording: Signal<bool>,
    conflicted: bool,
) -> AnyElement {
    let capturing = move || capture.get() == Some(action);
    let label = move || {
        if capturing() {
            "按下新快捷键…".to_owned()
        } else {
            let label = key_label_of(action);
            if label.is_empty() {
                "未绑定".to_owned()
            } else {
                label
            }
        }
    };
    let unbound = move || !capturing() && key_label_of(action).is_empty();
    nsh_base::button()
        .min_w(px(64.0))
        .h(px(26.0))
        .px(px(10.0))
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .border_1()
        .border_color(move || {
            if capturing() {
                primary()
            } else if conflicted {
                like_red()
            } else {
                border()
            }
        })
        .bg(move || {
            if capturing() {
                primary().opacity(0.12)
            } else {
                bg()
            }
        })
        .hover(|st| st.bg(hover()))
        .active(|st| st.opacity(0.7))
        .transition_colors()
        .duration(150.0)
        .accessibility_label(format!("重绑「{}」", action.title()))
        .on_press(move |_: &()| {
            recording.set(false);
            // 已在捕获别的动作时换目标；同一颗再点退出捕获。
            if capture.get_untracked() == Some(action) {
                capture.set(None);
            } else {
                capture.set(Some(action));
            }
        })
        .child(
            text(label)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || {
                    if capturing() {
                        primary()
                    } else if conflicted {
                        like_red()
                    } else if unbound() {
                        faint()
                    } else {
                        fg()
                    }
                }),
        )
        .into_element()
}

fn action_row(
    action: KeyAction,
    capture: Signal<Option<KeyAction>>,
    recording: Signal<bool>,
    conflicted: bool,
) -> View {
    view()
        .flex_row()
        .items_center()
        .h(px(32.0))
        .gap(px(12.0))
        .child(
            text(action.title())
                .flex_1()
                .min_w(px(0.0))
                .number_of_lines(1)
                .text_size(px(13.0))
                .line_height(px(13.0))
                .text_color(if conflicted { like_red() } else { fg() }),
        )
        .child(key_pill(action, capture, recording, conflicted))
}

/// 搜索匹配：标题含词（忽略大小写与分隔符）或键位命中——原始写法
/// （`mod-right`）、平台标签（`⌘→` / `Ctrl+→`）与 `mod` 的常见叫法
/// `cmd` / `ctrl` / `command` / `⌘` 都认。
fn action_matches(action: KeyAction, q: &str) -> bool {
    let norm = |s: &str| {
        s.chars()
            .filter(|c| !matches!(c, ' ' | '+' | '-' | '_' | '/'))
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let q = norm(q.trim());
    if q.is_empty() || norm(action.title()).contains(&q) {
        return true;
    }
    let key = zeo_ui_store::key_of(action);
    if key.is_empty() {
        return false;
    }
    let mut hay = vec![key.clone(), key_label_of(action)];
    for alias in ["cmd", "ctrl", "command", "⌘"] {
        hay.push(key.replacen("mod", alias, 1));
    }
    hay.iter().any(|h| norm(h).contains(&q))
}

fn group_block(
    name: &'static str,
    set: &BTreeSet<KeyAction>,
    q: &str,
    capture: Signal<Option<KeyAction>>,
    recording: Signal<bool>,
) -> Option<View> {
    let rows: Vec<KeyAction> = KeyAction::ALL
        .iter()
        .copied()
        .filter(|a| a.group() == name && !set.contains(a) && action_matches(*a, q))
        .collect();
    (!rows.is_empty()).then(|| {
        view()
            .gap(px(2.0))
            .child(
                text(name)
                    .text_size(px(11.0))
                    .line_height(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(faint())
                    .pb(px(4.0)),
            )
            .children(
                rows.iter()
                    .map(|&a| action_row(a, capture, recording, false)),
            )
    })
}

pub(crate) fn shortcuts_section() -> View {
    let s = store();
    let query = signal(String::new());
    let capture = signal(None::<KeyAction>);
    // 搜索词一变就退出捕获态：输入框聚焦时文本键进搜索框、修饰键却
    // 仍会进捕获，继续捕获只会误绑。
    effect(move || {
        query.get();
        capture.set(None);
    });
    // 捕获态的临时绑定挂在读 `capture` 的 dynamic 里：进出捕获换
    // owner，注册 / 注销都由生命周期收尾。捕获中整个界面只剩这组键。
    let recorder = dynamic(move || {
        if capture.get().is_some() {
            capture_keys(capture);
        }
        view()
    });
    // 「按键搜索」录制态同理：录制中只剩这组键，按下的组合键写回搜索框。
    let recording = signal(false);
    let key_recorder = dynamic(move || {
        if recording.get() {
            record_search_keys(recording, query);
        }
        view()
    });
    let dirty = move || {
        s.keybinds.with(|m| {
            KeyAction::ALL
                .iter()
                .any(|a| m.get(a).is_some_and(|k| k != a.default_key()))
        })
    };
    // 二次确认：第一下进 3 秒确认态（红色「确认还原？」），再点执行；
    // 超时或执行后退回。`stamp` 令牌让过期的定时器不取消新一轮确认。
    let confirming = signal(false);
    let stamp = signal(0u32);
    let reset = view()
        .opacity(move || if dirty() { 1.0f32 } else { 0.4f32 })
        .child(dynamic(move || {
            if confirming.get() {
                text_button(
                    Variant::Destructive,
                    Size::Sm,
                    None,
                    "确认还原？",
                    move || {
                        stamp.update(|g| *g += 1);
                        confirming.set(false);
                        zeo_ui_store::reset_keybinds();
                    },
                )
                .into_element()
            } else {
                text_button(
                    Variant::Outline,
                    Size::Sm,
                    None,
                    "还原默认",
                    move || {
                        confirming.set(true);
                        let g = stamp.get() + 1;
                        stamp.set(g);
                        spawn(async move {
                            timer(Duration::from_secs(3)).await;
                            if stamp.get() == g {
                                confirming.set(false);
                            }
                        });
                    },
                )
                .into_element()
            }
        }));
    // 顶部一行：搜索框吃剩余空间（标题与键位都能搜），「还原默认」居右。
    // 框尾的键盘图标是「按键搜索」：点它进录制态（图标变主色），按下的
    // 组合键自动填回搜索框；再点一下、Esc 或开始改绑都会退出录制。
    let record = icon_button(
        || Icon::Keyboard,
        12.0,
        "按键搜索",
        move || {
            if recording.get() { primary() } else { faint() }
        },
        move || {
            capture.set(None);
            recording.update(|r| *r = !*r);
        },
    );
    let head = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            search_box(query, Some(record.into_element()))
                .flex_1()
                .min_w(px(0.0)),
        )
        .child(reset);
    // 冲突行顶置 + 各分组（冲突中的动作从原分组拿掉）：`conflicted_
    // actions` 读 `s.keybinds`，dynamic 订阅它与搜索词，任一变即重排。
    let list = dynamic(move || {
        let set = zeo_ui_store::conflicted_actions();
        let q = query.get();
        let hit: Vec<KeyAction> = set
            .iter()
            .copied()
            .filter(|a| action_matches(*a, &q))
            .collect();
        let groups: Vec<View> = ["播放", "界面", "窗口", "全屏播放中"]
            .iter()
            .filter_map(|&g| group_block(g, &set, &q, capture, recording))
            .collect();
        let none = !q.trim().is_empty() && hit.is_empty() && groups.is_empty();
        view()
            .gap(px(16.0))
            .child((!hit.is_empty()).then(|| {
                view()
                    .gap(px(2.0))
                    .children(hit.iter().map(|&a| action_row(a, capture, recording, true)))
            }))
            .children(groups)
            .child(none.then(|| {
                text("没有匹配的快捷键")
                    .w_full()
                    .text_center()
                    .text_size(px(13.0))
                    .text_color(faint())
            }))
    });
    view()
        .w_full()
        .gap(px(16.0))
        .child(
            view()
                .gap(px(8.0))
                .child(head)
                .child(
                    text("macOS 上 ⌘ 为 Command、⌥ 为 Option；Windows 上对应为 Ctrl / Alt。文本框聚焦时，会输入文本的键位不触发快捷键。")
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .text_color(muted_fg()),
                ),
        )
        .child(list)
        .child(recorder)
        .child(key_recorder)
}
