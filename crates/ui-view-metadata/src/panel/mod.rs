//! 右侧面板：单选（m==1）全字段编辑 + 顶栏内联改名 + 「音频参数」
//! 节（常开、值可复制）；多选（m≥2）批量修改（字段三态 + 统一封面），
//! 写回走 `store::save_tags` / `save_tags_batch`。操作都收在固定顶栏
//! （单选「保存」/ 批量「应用」），状态行是滚动区第一条；滚动区末位
//! 不再放操作行。乱码修复 / 批量重命名不在面板里：工具栏图标钮开独立
//! 弹窗，各自预览、各自应用（`tools.rs` / `batch_rename.rs`）。

pub(crate) use std::collections::{BTreeSet, HashMap};

pub(crate) use std::path::{Path, PathBuf};

pub(crate) use std::rc::Rc;

pub(crate) use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) use std::sync::{Arc, Mutex, OnceLock};

pub(crate) use nsh_core::prelude::*;

pub(crate) use zeo_ui_view_folders::stop_if_playing_under;

pub(crate) use super::MetaRow;

pub(crate) use zeo_library::config::library_store;

pub(crate) use zeo_metadata::export::fix_mojibake;

pub(crate) use zeo_metadata::rename::{self, Status};

pub(crate) use zeo_metadata::write::{self, CoverEdit, Edit, TagEdits, TagField};

pub(crate) use zeo_types::config::TrackEntry;

pub(crate) use zeo_ui_components::button::{
    RADIUS_MD, Size, Variant, foreground, icon_button, shell, text_button, tipped,
};

pub(crate) use zeo_ui_components::checkbox::checkbox;

pub(crate) use zeo_ui_components::cover::cover;

pub(crate) use zeo_ui_components::form::{SelectOption, input_field, select, trigger_chevron};

pub(crate) use zeo_icons::{Icon, icon, icon_dyn};

pub(crate) use zeo_ui_store::theme::*;

pub(crate) use zeo_ui_store::{self as store, poster};

mod batch;
mod batch_export;
mod batch_ops;
mod batch_preview;
mod batch_rename;
mod cache;
mod edits;
mod genre;
mod history;
mod rows;
mod single;
mod tech;
#[cfg(test)]
mod tests;
mod tools;
mod widgets;

pub(crate) use batch::*;
pub(crate) use batch_export::*;
pub(crate) use batch_ops::*;
pub(crate) use batch_preview::*;
pub(crate) use batch_rename::*;
pub(crate) use cache::*;
pub(crate) use edits::*;
pub(crate) use genre::*;
pub(crate) use history::*;
pub(crate) use rows::*;
pub(crate) use single::*;
pub(crate) use tech::*;
pub(crate) use tools::*;
pub(crate) use widgets::*;
