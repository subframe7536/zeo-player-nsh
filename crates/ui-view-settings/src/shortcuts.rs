//! 设置 →「快捷键」：全部动作的可自定义键位（覆盖存 `library.json`
//! 的 `keybinds`，改后立即生效）。点键位胶囊进入捕获态，按下的下一个
//! 组合键即新绑定；重复的键位顶掉原动作的绑定（它变「未绑定」）。

use nsh_core::prelude::*;

use super::setting_section;
use zeo_ui_components::button::{Size, Variant, text_button};
use zeo_ui_store::theme::*;
use zeo_ui_store::{KeyAction, key_label_of, store};

/// 捕获态注册的临时绑定：`on_key` 是最新绑定优先，这些盖过全局快捷键
/// （含全屏页的）；Esc 取消、Backspace / Delete 解绑——最后注册，优先。
fn capture_keys(capture: Signal<Option<KeyAction>>) {
    for key in zeo_ui_store::capture_candidates() {
        on_key(&key, move |k| {
            if let Some(action) = capture.get_untracked() {
                zeo_ui_store::assign_keybind(action, k.to_owned());
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
/// 变「按下新快捷键…」。
fn key_pill(action: KeyAction, capture: Signal<Option<KeyAction>>) -> AnyElement {
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
        .rounded(px(6.0))
        .border_1()
        .border_color(move || if capturing() { primary() } else { border() })
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
                    } else if unbound() {
                        faint()
                    } else {
                        fg()
                    }
                }),
        )
        .into_element()
}

fn action_row(action: KeyAction, capture: Signal<Option<KeyAction>>) -> View {
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
                .text_color(fg()),
        )
        .child(key_pill(action, capture))
}

fn group_block(name: &'static str, capture: Signal<Option<KeyAction>>) -> View {
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
            KeyAction::ALL
                .iter()
                .filter(|a| a.group() == name)
                .map(|&a| action_row(a, capture)),
        )
}

pub(crate) fn shortcuts_section() -> View {
    let s = store();
    let capture = signal(None::<KeyAction>);
    // 捕获态的临时绑定挂在读 `capture` 的 dynamic 里：进出捕获换
    // owner，注册 / 注销都由生命周期收尾。捕获中整个界面只剩这组键。
    let recorder = dynamic(move || {
        if capture.get().is_some() {
            capture_keys(capture);
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
    let reset = text_button(
        Variant::Outline,
        Size::Sm,
        None,
        "还原默认",
        zeo_ui_store::reset_keybinds,
    );
    let reset = view()
        .opacity(move || if dirty() { 1.0f32 } else { 0.4f32 })
        .child(reset);
    setting_section(
        "快捷键",
        "点击键位胶囊后按下新的组合键即可换绑，改后立即生效；重复键位会顶掉原动作的绑定。捕获中按 Backspace 或 Delete 清除绑定、Esc 取消。",
        Some(reset.into_element()),
    )
    .child(
        view()
            .gap(px(16.0))
            .child(group_block("播放", capture))
            .child(group_block("界面", capture))
            .child(group_block("窗口", capture))
            .child(group_block("全屏播放中", capture)),
    )
    .child(
        text("macOS 上 ⌘ 为 Command、⌥ 为 Option；Windows 上对应为 Ctrl / Alt。文本框聚焦时，会输入文本的键位不触发快捷键。")
            .text_size(px(12.0))
            .text_color(muted_fg()),
    )
    .child(recorder)
}
