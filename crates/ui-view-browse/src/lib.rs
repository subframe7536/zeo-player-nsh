//! 「歌手 / 专辑 / 标签」页：曲库曲目按键分组，分组网格 + 详情列表
//! （详情页头与曲目列表是共享件）。

pub(crate) use std::collections::{HashMap, HashSet};

pub(crate) use std::rc::Rc;

pub(crate) use nsh_core::prelude::*;

pub(crate) use zeo_ui_views_base::{
    by_disc_track, compare, hero, no_library, total_duration, track_list,
};

pub(crate) use zeo_types::config::TrackEntry;

pub(crate) use zeo_ui_components::button::pill_button;

pub(crate) use zeo_ui_components::cover::cover_initial;

pub(crate) use zeo_ui_components::icons::{Icon, icon};

pub(crate) use zeo_ui_components::page::{empty_state, page_header};

pub(crate) use zeo_ui_components::tracks::{Columns, content_break, track_columns};

pub(crate) use zeo_ui_store::theme::*;

pub(crate) use zeo_ui_store::{cover_for, covers_on, format_total, play_list, store};

mod albums;
mod artists;
mod genres;
mod group;
mod sidebar;

pub use albums::*;
pub use artists::*;
pub use genres::*;
pub use group::avatar;
pub(crate) use group::*;
pub use sidebar::*;
