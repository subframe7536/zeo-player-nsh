//! 设置 →「快捷键」：窗口内动作的可自定义键位（覆盖存 `library.json`
//! 的 `keybinds`）与全局快捷键（`global_keybinds`，窗口外 / 隐藏到托
//! 盘也生效，默认全不绑），改后立即生效。点键位胶囊进入捕获态，按下
//! 的下一个组合键即新绑定；撞上已有键位时不顶绑——同键双方都不能触
//! 发、红字置顶，改掉任一方后原绑定自动恢复。全局键与窗口内键位撞键
//! 同样算冲突（窗口聚焦时全局键先响）。

use std::collections::BTreeSet;

use nsh_core::keymap::When;
use nsh_core::prelude::*;

use super::search_box;
use zeo_ui_components::button::{Size, Variant, icon_button, text_button};
use zeo_ui_components::icons::Icon;
use zeo_ui_store::theme::*;
use zeo_ui_store::{BindScope, KeyAction, key_label_of, store};

type Capture = Signal<Option<(BindScope, KeyAction)>>;

fn scope_label_of(scope: BindScope, action: KeyAction) -> String {
    match scope {
        BindScope::Local => key_label_of(action),
        BindScope::Global => zeo_ui_store::global_key_label_of(action),
    }
}

fn scope_key_of(scope: BindScope, action: KeyAction) -> String {
    match scope {
        BindScope::Local => zeo_ui_store::key_of(action),
        BindScope::Global => zeo_ui_store::global_key_of(action),
    }
}

fn set_scoped_bind(scope: BindScope, action: KeyAction, key: Option<String>) {
    match scope {
        BindScope::Local => zeo_ui_store::set_keybind(action, key),
        BindScope::Global => zeo_ui_store::set_global_keybind(action, key),
    }
}

/// 「按键搜索」录制态的临时绑定：`When::Always` 让输入框聚焦时文本键
/// 也被录制而不是输进框里；按下的组合键以带引号的平台标签（`"⌘→"` /
/// `"Ctrl+→"`）写回搜索框——引号让搜索走键位的等值匹配（一次只录
/// 一组键），Esc 取消、Backspace / Delete 清空。平台保留键（macOS 的
/// ⌘Q / ⌘H / ⌘M / ⌃⌘F，Windows 的 Ctrl+Q / Alt+F4 / F11）`on_key`
/// 绑定即 panic，拦不到，见 upstream.md。
fn record_search_keys(recording: Signal<bool>, query: Signal<String>) {
    let always = KeyOptions {
        when: When::Always,
        ..KeyOptions::default()
    };
    for key in zeo_ui_store::capture_candidates() {
        on_key_with(&key, always, move |k| {
            query.set(format!("\"{}\"", key_label(k)));
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

/// 捕获态注册的临时绑定：`on_key` 是最新绑定优先，这些盖过窗口内绑定
/// （含全屏页的）；全局快捷键则由 `s.key_capture` 联动整体注销，按到
/// 自己已注册的全局键也照常被捕获。Esc 取消、Backspace / Delete 解绑
/// ——最后注册，优先。
fn capture_keys(capture: Capture) {
    for key in zeo_ui_store::capture_candidates() {
        on_key(&key, move |k| {
            if let Some((scope, action)) = capture.get_untracked() {
                set_scoped_bind(scope, action, Some(k.to_owned()));
            }
            capture.set(None);
        });
    }
    on_key("escape", move |_| capture.set(None));
    for key in ["backspace", "delete"] {
        on_key(key, move |_| {
            if let Some((scope, action)) = capture.get_untracked() {
                set_scoped_bind(scope, action, Some(String::new()));
            }
            capture.set(None);
        });
    }
}

/// 一个动作的键位胶囊：常态显示当前键位（未绑定为「未绑定」）；捕获中
/// 变「按下新快捷键…」；冲突中红框红字。
fn key_pill(
    scope: BindScope,
    action: KeyAction,
    capture: Capture,
    recording: Signal<bool>,
    conflicted: bool,
) -> AnyElement {
    let capturing = move || capture.get() == Some((scope, action));
    let label = move || {
        if capturing() {
            "按下新快捷键…".to_owned()
        } else {
            let label = scope_label_of(scope, action);
            if label.is_empty() {
                "未绑定".to_owned()
            } else {
                label
            }
        }
    };
    let unbound = move || !capturing() && scope_label_of(scope, action).is_empty();
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
            if capture.get_untracked() == Some((scope, action)) {
                capture.set(None);
            } else {
                capture.set(Some((scope, action)));
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
    scope: BindScope,
    action: KeyAction,
    capture: Capture,
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
        .child(key_pill(scope, action, capture, recording, conflicted))
}

/// 搜索匹配：标题按子串模糊匹配（忽略大小写与分隔符）；键位走等值
/// 语义——`"…"` 包住的词（录制回的写法）只与键位等值比较，裸词
/// 除整组等值外还认尾键等值（`right` 命中 `mod-right` 的尾键，
/// `⌘a` 却不会误中 `⌥⌘←`）。键位认原始写法（`mod-right`）、平台
/// 标签（`⌘→` / `Ctrl+→`）与 `mod` 的常见叫法 `cmd` / `ctrl` /
/// `command` / `⌘`。
fn action_matches(scope: BindScope, action: KeyAction, q: &str) -> bool {
    let norm = |s: &str| {
        s.chars()
            .filter(|c| !matches!(c, ' ' | '+' | '-' | '_' | '/'))
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let q = q.trim();
    if q.is_empty() {
        return true;
    }
    let exact = q.len() > 2 && q.starts_with('"') && q.ends_with('"');
    let nq = norm(q.trim_matches('"'));
    if nq.is_empty() {
        return true;
    }
    if !exact && norm(action.title()).contains(&nq) {
        return true;
    }
    let key = scope_key_of(scope, action);
    if key.is_empty() {
        return false;
    }
    let mut hay = vec![key.clone(), scope_label_of(scope, action)];
    for alias in ["cmd", "ctrl", "command", "⌘"] {
        hay.push(key.replacen("mod", alias, 1));
    }
    hay.iter().any(|h| {
        norm(h) == nq
            || (!exact
                && h.rsplit(|c: char| matches!(c, '+' | '-' | ' '))
                    .next()
                    .is_some_and(|tail| !tail.is_empty() && norm(tail) == nq))
    })
}

fn group_block(
    name: &'static str,
    set: &BTreeSet<KeyAction>,
    q: &str,
    capture: Capture,
    recording: Signal<bool>,
) -> Option<View> {
    let rows: Vec<KeyAction> = KeyAction::ALL
        .iter()
        .copied()
        .filter(|a| {
            a.group() == name && !set.contains(a) && action_matches(BindScope::Local, *a, q)
        })
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
                    .map(|&a| action_row(BindScope::Local, a, capture, recording, false)),
            )
    })
}

/// 全局快捷键分组：可全局绑定的动作全集（`GLOBAL_ACTIONS`），冲突行
/// 同样顶置。组头附一行说明（窗口外生效、媒体键由平台接管）。
fn global_block(
    set: &BTreeSet<(BindScope, KeyAction)>,
    q: &str,
    capture: Capture,
    recording: Signal<bool>,
) -> Option<View> {
    let rows: Vec<KeyAction> = zeo_ui_store::GLOBAL_ACTIONS
        .iter()
        .copied()
        .filter(|a| {
            !set.contains(&(BindScope::Global, *a)) && action_matches(BindScope::Global, *a, q)
        })
        .collect();
    (!rows.is_empty()).then(|| {
        view()
            .gap(px(2.0))
            .child(
                text("全局快捷键")
                    .text_size(px(11.0))
                    .line_height(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(faint())
                    .pb(px(4.0)),
            )
            .child(
                text("窗口最小化到托盘也生效；媒体控制键由系统接管（Windows 低级键盘钩子 / macOS 媒体会话），不在此列。")
                    .text_size(px(11.0))
                    .line_height(px(15.0))
                    .text_color(faint())
                    .pb(px(6.0)),
            )
            .children(
                rows.iter()
                    .map(|&a| action_row(BindScope::Global, a, capture, recording, false)),
            )
    })
}

pub(crate) fn shortcuts_section() -> View {
    let s = store();
    let query = signal(String::new());
    // 捕获态存在 store 上：`install` 的全局快捷键 effect 靠它联动（捕
    // 获中注销全部全局键，候选键才不会被自己注册的键吃掉）。
    let capture = s.key_capture;
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
        s.global_keybinds
            .with(|m| m.values().any(|k| !k.is_empty()))
            || s.keybinds.with(|m| {
                KeyAction::ALL
                    .iter()
                    .any(|a| m.get(a).is_some_and(|k| k != a.default_key()))
            })
    };
    // 还原前弹系统确认框（同删除歌单的确认路径），确认才清覆盖表
    // （窗口内与全局一起清）。
    let reset = view()
        .opacity(move || if dirty() { 1.0f32 } else { 0.4f32 })
        .child(text_button(
            Variant::Outline,
            Size::Sm,
            None,
            "还原默认",
            move || {
                let answer = nsh_base::alert_dialog()
                    .title("还原默认快捷键？")
                    .description("所有自定义键位与全局快捷键都会被清除，恢复为默认绑定。")
                    .cancel("cancel", "取消")
                    .destructive("reset", "还原")
                    .show();
                spawn(async move {
                    if answer.await.map(|r| r.action) == Ok(Some("reset".into())) {
                        zeo_ui_store::reset_keybinds();
                    }
                });
            },
        ));
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
            search_box(query, Some(record.into_element()), move || recording.get())
                .flex_1()
                .min_w(px(0.0)),
        )
        .child(reset);
    // 冲突行顶置 + 各分组（冲突中的动作从原分组拿掉）：冲突集读两个
    // 键位信号，dynamic 订阅它们与搜索词，任一变即重排。全局组排在最
    // 后（动作多、默认全未绑定）。
    let list = dynamic(move || {
        let local = zeo_ui_store::conflicted_actions();
        let global = zeo_ui_store::global_conflicted_actions();
        let set: BTreeSet<(BindScope, KeyAction)> = local
            .iter()
            .map(|&a| (BindScope::Local, a))
            .chain(global.iter().map(|&a| (BindScope::Global, a)))
            .collect();
        let q = query.get();
        let hit: Vec<(BindScope, KeyAction)> = set
            .iter()
            .copied()
            .filter(|(scope, a)| action_matches(*scope, *a, &q))
            .collect();
        let mut groups: Vec<View> = ["播放", "界面", "窗口", "全屏播放中"]
            .iter()
            .filter_map(|&g| {
                let local_set: BTreeSet<KeyAction> = local.clone();
                group_block(g, &local_set, &q, capture, recording)
            })
            .collect();
        groups.extend(global_block(&set, &q, capture, recording));
        let none = !q.trim().is_empty() && hit.is_empty() && groups.is_empty();
        view()
            .gap(px(16.0))
            .child((!hit.is_empty()).then(|| {
                view().gap(px(2.0)).children(
                    hit.iter()
                        .map(|&(scope, a)| action_row(scope, a, capture, recording, true)),
                )
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
            view().gap(px(8.0)).child(head).child(
                text("⌘ / ⌥ 对应 Windows 的 Ctrl / Alt；输入框内打字不触发快捷键。")
                    .text_size(px(12.0))
                    .line_height(px(12.0))
                    .text_color(faint()),
            ),
        )
        .child(list)
        .child(recorder)
        .child(key_recorder)
}
