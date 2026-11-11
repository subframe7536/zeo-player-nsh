//! 设置 →「曲库」分类的各节。

pub(crate) use std::collections::HashSet;

pub(crate) use nsh_core::prelude::*;

pub(crate) use zeo_ui_views_base::{SEP, add_dropped, last_segment, total_duration, url_to_path};

pub(crate) use crate::{note, pref_row, setting_section};

pub(crate) use zeo_library::config::library_store;

pub(crate) use zeo_ui_components::button::{icon_button, outline_button, pill_button};
pub(crate) use zeo_ui_components::form;

pub(crate) use zeo_ui_components::icons::{Icon, icon};

pub(crate) use zeo_ui_store::theme::*;

pub(crate) use zeo_ui_store::{format_total, scan, store};

mod delete;
mod filter;
mod folders;
mod library;

pub(crate) use delete::*;
pub(crate) use filter::*;
pub(crate) use folders::*;
pub(crate) use library::*;
