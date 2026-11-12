//! 快捷键动作清单与键位计算（叶子 crate，只依赖 `nsh-core`）：`KeyAction`
//! 的 id 落盘进 `library.json` 的 `keybinds` 覆盖表（动作 id → 按键文
//! 本，`nsh/keymap` 的写法；空串 = 明确解绑）。信号的生效表、配置读写与
//! 动作派发在 `zeo-ui-store`（`s.keybinds`、`run_keybind`）。

use std::collections::BTreeMap;

use nsh_core::prelude::*;

/// 一个可自定义快捷键的动作（`id` 落盘进配置，别改名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyAction {
    Toggle,
    Prev,
    Next,
    SeekBack,
    SeekFwd,
    VolDown,
    VolUp,
    Mute,
    Like,
    LoopMode,
    Palette,
    PaletteOpen,
    SettingsOpen,
    FullScreen,
    NavHome,
    NavLibrary,
    NavArtists,
    NavAlbums,
    NavMetadata,
    NavFolders,
    NavGenres,
    CloseWindow,
    FullClose,
    FullSeekBack,
    FullSeekFwd,
    FullSeekBackBig,
    FullSeekFwdBig,
    FullVolDown,
    FullVolUp,
}

impl KeyAction {
    /// 全部动作（设置页的展示顺序）。
    pub const ALL: &'static [KeyAction] = &[
        KeyAction::Toggle,
        KeyAction::Prev,
        KeyAction::Next,
        KeyAction::SeekBack,
        KeyAction::SeekFwd,
        KeyAction::VolDown,
        KeyAction::VolUp,
        KeyAction::Mute,
        KeyAction::Like,
        KeyAction::LoopMode,
        KeyAction::Palette,
        KeyAction::PaletteOpen,
        KeyAction::SettingsOpen,
        KeyAction::FullScreen,
        KeyAction::NavHome,
        KeyAction::NavLibrary,
        KeyAction::NavArtists,
        KeyAction::NavAlbums,
        KeyAction::NavMetadata,
        KeyAction::NavFolders,
        KeyAction::NavGenres,
        KeyAction::CloseWindow,
        KeyAction::FullClose,
        KeyAction::FullSeekBack,
        KeyAction::FullSeekFwd,
        KeyAction::FullSeekBackBig,
        KeyAction::FullSeekFwdBig,
        KeyAction::FullVolDown,
        KeyAction::FullVolUp,
    ];

    /// 配置里的键。
    pub fn id(self) -> &'static str {
        match self {
            KeyAction::Toggle => "toggle",
            KeyAction::Prev => "prev",
            KeyAction::Next => "next",
            KeyAction::SeekBack => "seek_back",
            KeyAction::SeekFwd => "seek_fwd",
            KeyAction::VolDown => "vol_down",
            KeyAction::VolUp => "vol_up",
            KeyAction::Mute => "mute",
            KeyAction::Like => "like",
            KeyAction::LoopMode => "loop_mode",
            KeyAction::Palette => "palette",
            KeyAction::PaletteOpen => "palette_open",
            KeyAction::SettingsOpen => "settings_open",
            KeyAction::FullScreen => "full_screen",
            KeyAction::NavHome => "nav_home",
            KeyAction::NavLibrary => "nav_library",
            KeyAction::NavArtists => "nav_artists",
            KeyAction::NavAlbums => "nav_albums",
            KeyAction::NavMetadata => "nav_metadata",
            KeyAction::NavFolders => "nav_folders",
            KeyAction::NavGenres => "nav_genres",
            KeyAction::CloseWindow => "close_window",
            KeyAction::FullClose => "full_close",
            KeyAction::FullSeekBack => "full_seek_back",
            KeyAction::FullSeekFwd => "full_seek_fwd",
            KeyAction::FullSeekBackBig => "full_seek_back_big",
            KeyAction::FullSeekFwdBig => "full_seek_fwd_big",
            KeyAction::FullVolDown => "full_vol_down",
            KeyAction::FullVolUp => "full_vol_up",
        }
    }

    /// 设置页的动作名。
    pub fn title(self) -> &'static str {
        match self {
            KeyAction::Toggle => "播放 / 暂停",
            KeyAction::Prev => "上一首",
            KeyAction::Next => "下一首",
            KeyAction::SeekBack => "快退 5 秒",
            KeyAction::SeekFwd => "快进 5 秒",
            KeyAction::VolDown => "降低音量",
            KeyAction::VolUp => "增大音量",
            KeyAction::Mute => "静音 / 取消静音",
            KeyAction::Like => "喜欢当前曲目",
            KeyAction::LoopMode => "切换循环模式",
            KeyAction::Palette => "命令面板（开关）",
            KeyAction::PaletteOpen => "命令面板（打开）",
            KeyAction::SettingsOpen => "偏好设置",
            KeyAction::FullScreen => "全屏播放",
            KeyAction::NavHome => "页面：首页",
            KeyAction::NavLibrary => "页面：音乐库",
            KeyAction::NavArtists => "页面：艺术家",
            KeyAction::NavAlbums => "页面：专辑",
            KeyAction::NavMetadata => "页面：元数据",
            KeyAction::NavFolders => "页面：文件夹",
            KeyAction::NavGenres => "页面：标签 / 流派",
            KeyAction::CloseWindow => "关闭窗口",
            KeyAction::FullClose => "收起全屏播放",
            KeyAction::FullSeekBack => "快退 5 秒",
            KeyAction::FullSeekFwd => "快进 5 秒",
            KeyAction::FullSeekBackBig => "快退 30 秒",
            KeyAction::FullSeekFwdBig => "快进 30 秒",
            KeyAction::FullVolDown => "降低音量",
            KeyAction::FullVolUp => "增大音量",
        }
    }

    /// 设置页的分组名。
    pub fn group(self) -> &'static str {
        match self {
            KeyAction::Toggle
            | KeyAction::Prev
            | KeyAction::Next
            | KeyAction::SeekBack
            | KeyAction::SeekFwd
            | KeyAction::VolDown
            | KeyAction::VolUp
            | KeyAction::Mute
            | KeyAction::Like
            | KeyAction::LoopMode => "播放",
            KeyAction::Palette
            | KeyAction::PaletteOpen
            | KeyAction::SettingsOpen
            | KeyAction::FullScreen
            | KeyAction::NavHome
            | KeyAction::NavLibrary
            | KeyAction::NavArtists
            | KeyAction::NavAlbums
            | KeyAction::NavMetadata
            | KeyAction::NavFolders
            | KeyAction::NavGenres => "界面",
            KeyAction::CloseWindow => "窗口",
            _ => "全屏播放中",
        }
    }

    /// 默认键位（`nsh/keymap` 写法；空串 = 默认不绑）。「喜欢」与「循环
    /// 模式」默认不绑：`mod+r` 是开发宿主的刷新键、裸 `mod+l` 太冷门，
    /// 需要的用户在设置里自己绑。
    pub fn default_key(self) -> &'static str {
        match self {
            KeyAction::Toggle => "space",
            KeyAction::Prev => "mod+left",
            KeyAction::Next => "mod+right",
            KeyAction::SeekBack => "mod+alt+left",
            KeyAction::SeekFwd => "mod+alt+right",
            KeyAction::VolDown => "mod+down",
            KeyAction::VolUp => "mod+up",
            KeyAction::Mute => "",
            KeyAction::Like => "",
            KeyAction::LoopMode => "",
            KeyAction::Palette => "mod+k",
            KeyAction::PaletteOpen => "mod+f",
            KeyAction::SettingsOpen => "mod+,",
            KeyAction::FullScreen => "mod+enter",
            KeyAction::NavHome => "mod+1",
            KeyAction::NavLibrary => "mod+2",
            KeyAction::NavArtists => "mod+3",
            KeyAction::NavAlbums => "mod+4",
            KeyAction::NavMetadata => "mod+5",
            KeyAction::NavFolders => "mod+6",
            KeyAction::NavGenres => "mod+7",
            KeyAction::CloseWindow => "mod+w",
            KeyAction::FullClose => "escape",
            KeyAction::FullSeekBack => "left",
            KeyAction::FullSeekFwd => "right",
            KeyAction::FullSeekBackBig => "shift+left",
            KeyAction::FullSeekFwdBig => "shift+right",
            KeyAction::FullVolDown => "down",
            KeyAction::FullVolUp => "up",
        }
    }

    /// 按住是否连发（快进 / 音量）。
    pub fn hold(self) -> bool {
        matches!(
            self,
            KeyAction::SeekBack
                | KeyAction::SeekFwd
                | KeyAction::VolDown
                | KeyAction::VolUp
                | KeyAction::FullSeekBack
                | KeyAction::FullSeekFwd
                | KeyAction::FullSeekBackBig
                | KeyAction::FullSeekFwdBig
                | KeyAction::FullVolDown
                | KeyAction::FullVolUp
        )
    }

    /// 绑定挂在全屏播放页（`ui-now-playing`）而不是主界面。
    pub fn in_full_player(self) -> bool {
        matches!(
            self,
            KeyAction::FullClose
                | KeyAction::FullSeekBack
                | KeyAction::FullSeekFwd
                | KeyAction::FullSeekBackBig
                | KeyAction::FullSeekFwdBig
                | KeyAction::FullVolDown
                | KeyAction::FullVolUp
        )
    }
}

/// 可全局绑定的动作（窗口外也生效，`register_global_shortcut`）：全屏
/// 播放页内的动作（`in_full_player`）除外——它们只在全屏上下文有意义。
/// 媒体控制键（⏯ / ⏮ / ⏭）上游的 `Key` 表达不了，不在键位候选里。
pub const GLOBAL_ACTIONS: &[KeyAction] = &[
    KeyAction::Toggle,
    KeyAction::Prev,
    KeyAction::Next,
    KeyAction::SeekBack,
    KeyAction::SeekFwd,
    KeyAction::VolDown,
    KeyAction::VolUp,
    KeyAction::Mute,
    KeyAction::Like,
    KeyAction::LoopMode,
    KeyAction::Palette,
    KeyAction::PaletteOpen,
    KeyAction::SettingsOpen,
    KeyAction::FullScreen,
    KeyAction::NavHome,
    KeyAction::NavLibrary,
    KeyAction::NavArtists,
    KeyAction::NavAlbums,
    KeyAction::NavMetadata,
    KeyAction::NavFolders,
    KeyAction::NavGenres,
    KeyAction::CloseWindow,
];

/// 键位绑定的作用域：窗口内（`on_key`）还是全局（`register_global_
/// shortcut`）。设置页的捕获态按它分发写库。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BindScope {
    Local,
    Global,
}

/// 键位表（动作 → 生效键位，空串 = 未绑定）。
pub type Keybinds = BTreeMap<KeyAction, String>;

/// 由覆盖表算出生效键位：缺的回默认；手改过配置文件的非法键位
/// （语法错、平台保留键）直接丢弃——`on_key` 绑它们会 panic。
pub fn effective_keybinds(overrides: &BTreeMap<String, String>) -> Keybinds {
    let apple = try_apple_keys().unwrap_or(false);
    let os = try_os();
    let valid = |key: &&String| {
        key.is_empty()
            || nsh_core::keymap::Keystroke::parse_with(key, apple)
                .is_ok_and(|k| os.is_none_or(|os| k.reserved(os).is_none()))
    };
    KeyAction::ALL
        .iter()
        .map(|a| {
            let key = overrides
                .get(a.id())
                .filter(valid)
                .cloned()
                .unwrap_or_else(|| a.default_key().to_owned());
            (*a, key)
        })
        .collect()
}

/// 全局快捷键的生效键位：只看 `GLOBAL_ACTIONS`，默认全不绑（
/// `default_key` 不参与）；非法键位（语法错、平台保留键）同
/// `effective_keybinds` 丢弃。
pub fn effective_global_keybinds(overrides: &BTreeMap<String, String>) -> Keybinds {
    let apple = try_apple_keys().unwrap_or(false);
    let os = try_os();
    let valid = |key: &&String| {
        nsh_core::keymap::Keystroke::parse_with(key, apple)
            .is_ok_and(|k| os.is_none_or(|os| k.reserved(os).is_none()))
    };
    GLOBAL_ACTIONS
        .iter()
        .map(|a| {
            let key = overrides
                .get(a.id())
                .filter(|k| !k.is_empty() && valid(k))
                .cloned()
                .unwrap_or_default();
            (*a, key)
        })
        .collect()
}

/// 「按下新键」捕获态能绑的键位清单（修饰组合 × 键）。Esc 留给「取消」、
/// Tab 留给焦点循环（绑了会断 Tab 导航）；平台保留键（macOS 的 ⌘Q /
/// ⌘H / ⌘M / ⌃⌘F，Windows 的 Ctrl+Q / Alt+F4 / F11）`on_key` 绑了会
/// panic，也排除。组件上下文里调用（读 `apple_keys` / `os`）。
pub fn capture_candidates() -> Vec<String> {
    const MODS: &[&str] = &[
        "",
        "shift",
        "alt",
        "ctrl",
        "mod",
        "alt+shift",
        "ctrl+shift",
        "mod+shift",
        "ctrl+alt",
        "mod+alt",
        "ctrl+mod",
        "ctrl+alt+shift",
        "mod+alt+shift",
        "ctrl+mod+shift",
        "ctrl+alt+mod",
        "ctrl+alt+shift+mod",
    ];
    const CHARS: &[char] = &[
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
        '-', '=', '[', ']', '\\', ';', '\'', ',', '.', '/', '`',
    ];
    const NAMED: &[&str] = &[
        "space",
        "enter",
        "backspace",
        "delete",
        "insert",
        "up",
        "down",
        "left",
        "right",
        "home",
        "end",
        "pageup",
        "pagedown",
        "f1",
        "f2",
        "f3",
        "f4",
        "f5",
        "f6",
        "f7",
        "f8",
        "f9",
        "f10",
        "f11",
        "f12",
    ];
    let apple = apple_keys();
    let os = os();
    let mut out = Vec::with_capacity(MODS.len() * (CHARS.len() + NAMED.len()));
    for mods in MODS {
        for c in CHARS {
            let text = if mods.is_empty() {
                c.to_string()
            } else {
                format!("{mods}+{c}")
            };
            push_candidate(&mut out, text, apple, os);
        }
        for name in NAMED {
            let text = if mods.is_empty() {
                (*name).to_owned()
            } else {
                format!("{mods}+{name}")
            };
            push_candidate(&mut out, text, apple, os);
        }
    }
    out
}

fn push_candidate(out: &mut Vec<String>, text: String, apple: bool, os: nsh_core::platform::Os) {
    let Ok(key) = nsh_core::keymap::Keystroke::parse_with(&text, apple) else {
        return;
    };
    if key.reserved(os).is_none() {
        out.push(text);
    }
}
