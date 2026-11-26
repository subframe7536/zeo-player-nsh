//! 全屏播放的歌词列：跟随时间滚动（当前行停在 [`ANCHOR`] 处），用户滚动
//! 后 3 秒恢复跟随；点击行 seek；离锚点附近的清晰带越远越模糊、上下边缘
//! 淡出（`blur` 的 shader，按视口位置逐像素算）。

pub(crate) use std::cell::RefCell;

pub(crate) use std::rc::Rc;

pub(crate) use std::time::Duration;

pub(crate) use nsh_core::prelude::*;

pub(crate) use nsh_base::popover;

pub(crate) use super::{Reveal, Swap};

pub(crate) use zeo_types::config::LyricsAlign;

pub(crate) use zeo_ui_components::button::{RADIUS_MD, icon_button, on_dark, tipped};

pub(crate) use zeo_icons::{Icon, icon};

pub(crate) use zeo_ui_components::slider::{SliderStyle, slider};

pub(crate) use zeo_ui_store::theme::*;

pub(crate) use zeo_ui_store::{
    format_time, nudge_lyrics_offset, seek_to, set_lyrics_offset, store,
};

mod anim;
mod blur;
mod rail;
mod scroller;
#[cfg(test)]
mod tests;

pub(crate) use anim::*;
pub(crate) use blur::*;
pub(crate) use rail::*;
pub(crate) use scroller::*;
