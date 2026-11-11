//! 「文件夹」页：左侧是曲库根目录的目录树（只列文件夹，可折叠；选中项
//! 的祖先总是展开），右侧是所选文件夹（含子文件夹）的全部曲目。把文件夹 / 音频
//! 文件拖进来会加进曲库：已在某个根目录下的只定位过去，否则它（或它
//! 所在的目录）加为新根。右侧的曲目行与树里的文件夹行也可以拖到某个
//! 树行上，把文件 / 文件夹真正移动过去（`fs::rename`，被移走的曲库根
//! 目录会同步改路径）。树行右键菜单做增删改：新建子文件夹、重命名、
//! 删除（回收站 / 直接删按「删除文件」设置）；曲目行也能重命名文件。

mod dialogs;
mod drag;
mod ops;
mod page;
mod row;
mod state;
#[cfg(test)]
mod tests;
mod tree;
mod view;

pub use ops::*;
pub use page::*;
pub use view::*;
pub use zeo_ui_store::FileOp;

pub(crate) use std::cell::{Cell, RefCell};
pub(crate) use std::collections::{BTreeMap, BTreeSet};
pub(crate) use std::path::Path;
pub(crate) use std::rc::Rc;
pub(crate) use std::sync::Arc;
pub(crate) use std::time::Duration;

pub(crate) use nsh_base::context_menu;
pub(crate) use nsh_core::prelude::*;

pub(crate) use zeo_library::config::library_store;
pub(crate) use zeo_playlists::Condition;
pub(crate) use zeo_shell::{REVEAL_LABEL, reveal};
pub(crate) use zeo_types::config::{TrackEntry, unix_now};
pub(crate) use zeo_ui_components::button::{
    Size, Variant, icon_button, outline_button, pill_button, text_button,
};
pub(crate) use zeo_ui_components::form::{
    dialog_content, dialog_footer, dialog_header, input_field, modal, tab_list, tab_trigger,
};
pub(crate) use zeo_ui_components::icons::{Icon, icon, icon_dyn};
pub(crate) use zeo_ui_components::page::{empty_state, page_header};
pub(crate) use zeo_ui_components::tracks::{
    Columns, ROW, format_bytes, track_columns, track_row_selectable,
};
pub(crate) use zeo_ui_store::theme::*;
pub(crate) use zeo_ui_store::{self as store, Same, play_list, player, scan, store};
pub(crate) use zeo_ui_views_base::{
    SEP, add_dropped, by_disc_track, compare, item, last_segment, no_library, popup, separator,
    url_to_path,
};

pub(crate) use dialogs::*;
pub(crate) use drag::*;
pub(crate) use row::*;
pub(crate) use state::*;
pub(crate) use tree::*;
