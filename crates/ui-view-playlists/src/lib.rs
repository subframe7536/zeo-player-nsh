//! 歌单界面：侧栏的歌单区、歌单页、曲目的右键菜单（添加到歌单 / 从歌单
//! 移除）、新建与编辑弹窗（智能歌单的条件编辑器）。

pub(crate) use std::collections::BTreeMap;

pub(crate) use std::rc::Rc;

pub(crate) use nsh_base::{context_menu, menu};

pub(crate) use nsh_core::prelude::*;

pub(crate) use zeo_ui_views_base::{item, popup, separator};

pub(crate) use zeo_playlists::{Condition, Field, FieldType, Op, Playlist, Rule, Sort};

pub(crate) use zeo_types::config::unix_now;

pub(crate) use zeo_ui_components::button::{
    Size, Variant, icon_button, outline_button, pill_button, text_button,
};

pub(crate) use zeo_ui_components::form::{
    SelectOption, cover_realign, cover_realign_flex, dialog_content, dialog_footer, dialog_header,
    input_field, modal, select, select_trigger, tab_list, tab_trigger,
};

pub(crate) use zeo_ui_components::icons::{Icon, icon};

pub(crate) use zeo_ui_components::page::{empty_state, page_header};

pub(crate) use zeo_ui_components::tracks::{Columns, track_columns};

pub(crate) use zeo_ui_store::Nav;

pub(crate) use zeo_ui_store::theme::*;

pub(crate) use zeo_ui_store::{self as store, Draft, format_total, play_list, store};

mod editor;
mod menus;
mod page;

pub use editor::*;
pub use menus::*;
pub use page::*;

// 曲目菜单移到了 `zeo-ui-views-base`（共享列表要用），原位透出。
pub use zeo_ui_views_base::track_menu::{add_to_submenu, track_menu, track_menu_selectable};

// --- 菜单部件 -----------------------------------------------------------------
