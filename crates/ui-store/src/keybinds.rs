//! 可自定义快捷键：动作与键位计算在 `zeo_keybinds`（叶子 crate），这里
//! 是界面侧的生效表（`store().keybinds` 信号）、配置读写与派发。绑定由
//! 注册点（根组件、全屏播放页）放进读 `s.keybinds` 的 `dynamic`：换绑
//! 即换 owner，旧绑定连 owner 销毁、按新键位重绑，不用重启。

use std::collections::BTreeMap;

use super::*;

pub use zeo_keybinds::{KeyAction, Keybinds, capture_candidates, effective_keybinds};

/// 当前生效键位表（配置实时值）。
pub fn keybinds_map() -> Keybinds {
    effective_keybinds(&library_store().keybinds())
}

/// 配置写入后把生效表同步进信号（`install` 与各 setter 调用）。
pub(crate) fn refresh_keybinds() {
    store().keybinds.set(Same(Arc::new(keybinds_map())));
}

/// 一个动作的生效键位（空串 = 未绑定）。
pub fn key_of(action: KeyAction) -> String {
    store()
        .keybinds
        .with(|m| m.get(&action).cloned().unwrap_or_default())
}

/// 一个动作的键位标签（`⌘→` / `Ctrl+→`；未绑定为空串）。
pub fn key_label_of(action: KeyAction) -> String {
    let key = key_of(action);
    if key.is_empty() {
        String::new()
    } else {
        key_label(&key)
    }
}

/// 提示文本：`"上一首 ⌘←"`；未绑定时只有标题。
pub fn key_tip(title: &'static str, action: KeyAction) -> String {
    let label = key_label_of(action);
    if label.is_empty() {
        title.to_owned()
    } else {
        format!("{title} {label}")
    }
}

/// 改 `action` 的键位：`Some(key)` 覆盖（空串 = 解绑），`None` 回默认。
pub fn set_keybind(action: KeyAction, key: Option<String>) {
    library_store().set_keybind(action.id(), key.as_deref());
    refresh_keybinds();
}

/// 生效表里握着同一个键（非空）的动作集合：≥2 即冲突。冲突双方键盘
/// 触发全部取消（`bind_key` 不注册）、设置页红字置顶；改掉任一方后
/// 剩余动作的绑定自动恢复——冲突只是状态，不动配置。
pub fn conflicted_actions() -> std::collections::BTreeSet<KeyAction> {
    store().keybinds.with(|map| {
        let mut counts = BTreeMap::<&str, usize>::new();
        for k in map.values() {
            if !k.is_empty() {
                *counts.entry(k.as_str()).or_default() += 1;
            }
        }
        map.iter()
            .filter(|(_, k)| !k.is_empty() && counts[k.as_str()] > 1)
            .map(|(a, _)| *a)
            .collect()
    })
}

/// 全部键位回默认（清空覆盖）。
pub fn reset_keybinds() {
    library_store().reset_keybinds();
    refresh_keybinds();
}

/// 在当前 owner 上按生效键位绑 `action`：`run` 在按键时执行。
/// 放在读 `s.keybinds` 的 `dynamic` 里调用——读即订阅，换绑自动重绑。
/// 键位为空（用户解绑）或与其他动作同键（冲突）时不绑——冲突中双方
/// 都不能触发，改掉任一方后剩余方自动恢复。
pub fn bind_key(action: KeyAction, run: impl Fn() + 'static) {
    let key = key_of(action);
    if key.is_empty() || conflicted_actions().contains(&action) {
        return;
    }
    if action.hold() {
        let options = KeyOptions {
            repeat: true,
            ..KeyOptions::default()
        };
        on_key_with(&key, options, move |_| run());
    } else {
        on_key(&key, move |_| run());
    }
}

/// 触发 `action` 的效果。`FullClose` 不在此列（要拿全屏页的 `close`
/// 闭包）：`ui-now-playing` 自己处理它。
pub fn run_keybind(action: KeyAction) {
    let s = store();
    let volume_step = |d: f64| set_volume(s.playback.with_untracked(|p| p.volume) + d);
    match action {
        KeyAction::Toggle => toggle(),
        KeyAction::Prev => player().previous(),
        KeyAction::Next => player().next(),
        KeyAction::SeekBack => player().seek_by(-5.0),
        KeyAction::SeekFwd => player().seek_by(5.0),
        KeyAction::VolDown => volume_step(-0.05),
        KeyAction::VolUp => volume_step(0.05),
        KeyAction::Mute => toggle_mute(),
        KeyAction::Like => toggle_like_current(),
        KeyAction::LoopMode => cycle_loop_mode(),
        KeyAction::Palette => s.search_open.update(|open| *open = !*open),
        KeyAction::PaletteOpen => s.search_open.set(true),
        KeyAction::SettingsOpen => s.settings.set(true),
        KeyAction::FullScreen => s.full_screen.update(|on| *on = !*on),
        KeyAction::NavHome => navigate(Nav::Home),
        KeyAction::NavLibrary => navigate(Nav::Library),
        KeyAction::NavArtists => navigate(Nav::Artists),
        KeyAction::NavAlbums => navigate(Nav::Albums),
        KeyAction::NavMetadata => navigate(Nav::Metadata),
        KeyAction::NavFolders => navigate(Nav::Folders),
        KeyAction::NavGenres => navigate(Nav::Genres),
        // 先关最上层的面板（命令面板、设置）；都没有才与点窗口关闭按钮走
        // 同一条路径：按「关闭窗口」设置最小化到托盘 / 询问 / 退出。
        KeyAction::CloseWindow => {
            if s.search_open.get_untracked() {
                s.search_open.set(false);
            } else if s.settings.get_untracked() {
                s.settings.set(false);
            } else if zeo_shell::close_requested() {
                zeo_shell::quit();
            }
        }
        KeyAction::FullClose => {}
        KeyAction::FullSeekBack => player().seek_by(-5.0),
        KeyAction::FullSeekFwd => player().seek_by(5.0),
        KeyAction::FullSeekBackBig => player().seek_by(-30.0),
        KeyAction::FullSeekFwdBig => player().seek_by(30.0),
        KeyAction::FullVolDown => volume_step(-0.05),
        KeyAction::FullVolUp => volume_step(0.05),
    }
}
