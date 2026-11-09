//! 全屏播放的左列：封面、曲名与「更多」、歌手 / 专辑、标签、细波形进度
//! 与音频信息、线框控制按钮、音量。

pub(crate) use nsh_base::{menu, menu_item, menu_separator};

pub(crate) use nsh_core::prelude::*;

pub(crate) use super::Swap;

pub(crate) use zeo_engine::audio::LoopMode;

pub(crate) use zeo_metadata::MetaInfo;

pub(crate) use zeo_shell::{REVEAL_LABEL, reveal};

pub(crate) use zeo_ui_components::button::{on_dark, tipped};

pub(crate) use zeo_ui_components::icons::{Icon, icon, icon_dyn};

pub(crate) use zeo_ui_components::slider::{SliderStyle, scrub, slider};

pub(crate) use zeo_ui_store::theme::*;

pub(crate) use zeo_ui_store::{
    format_time, open_edit_info, player, progress_fraction, seek_to, set_volume, shown_time, store,
    toggle_mute,
};

pub(crate) use zeo_ui_views::playlists;

mod info;
mod left;
mod menu;
#[cfg(test)]
mod tests;
mod volume;
mod wave;

pub(crate) use info::*;
pub(crate) use left::*;
pub(crate) use menu::*;
pub(crate) use volume::*;
pub(crate) use wave::*;
