//! 右侧面板：单选（m==1）全字段编辑 + 顶栏内联改名 + 「音频参数」
//! 节（常开、值可复制）；多选（m≥2）批量修改（字段三态 + 音轨编号 /
//! 大小写 / 模板重命名工具 + 统一封面），写回走 `store::save_tags` /
//! `save_tags_batch`。操作都收在固定顶栏（单选「保存」/ 批量「应用」），
//! 状态行是滚动区第一条；滚动区末位不再放操作行。

pub(crate) use std::collections::{BTreeSet, HashMap};

pub(crate) use std::path::{Path, PathBuf};

pub(crate) use std::rc::Rc;

pub(crate) use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) use std::sync::{Arc, Mutex, OnceLock};

pub(crate) use nsh_core::prelude::*;

pub(crate) use zeo_ui_view_folders::stop_if_playing_under;

pub(crate) use super::MetaRow;

pub(crate) use zeo_library::config::library_store;

pub(crate) use zeo_metadata::rename::{self, Status};

pub(crate) use zeo_metadata::write::{self, CoverEdit, Edit, TagEdits, TagField};

pub(crate) use zeo_types::config::TrackEntry;

pub(crate) use zeo_ui_components::button::{
    RADIUS_MD, Size, Variant, icon_button, shell, text_button,
};

pub(crate) use zeo_ui_components::checkbox::checkbox;

pub(crate) use zeo_ui_components::cover::cover;

pub(crate) use zeo_ui_components::form::{SelectOption, input_field, select, trigger_chevron};

pub(crate) use zeo_ui_components::icons::{Icon, icon};

pub(crate) use zeo_ui_store::theme::*;

pub(crate) use zeo_ui_store::{self as store, poster};

mod batch;
mod batch_ops;
mod cache;
mod edits;
mod genre;
mod rows;
mod single;
mod tech;
#[cfg(test)]
mod tests;
mod widgets;

pub(crate) use batch::*;
pub(crate) use batch_ops::*;
pub(crate) use cache::*;
pub(crate) use edits::*;
pub(crate) use genre::*;
pub(crate) use rows::*;
pub(crate) use single::*;
pub(crate) use tech::*;
pub(crate) use widgets::*;
