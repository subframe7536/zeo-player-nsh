//! 路由页外观：各视图在 `zeo-ui-view-*` crate 里，这里只做透出；
//! 「音乐库」整表页也在这（跨页共享件在 `zeo-ui-views-base`）。

pub use zeo_ui_views_base::*;

pub use zeo_ui_view_browse::{albums, artists, genres, tags_sidebar_list};
pub use zeo_ui_view_command_palette::dialog as command_palette_dialog;
pub use zeo_ui_view_edit_info::edit_info_dialog;
pub use zeo_ui_view_folders as folders;
pub use zeo_ui_view_folders::{folders, tree_sidebar_list};
pub use zeo_ui_view_home::home;
pub use zeo_ui_view_metadata as metadata;
pub use zeo_ui_view_playlists as playlists;
pub use zeo_ui_view_settings as settings;
pub use zeo_ui_view_settings::dialog as settings_dialog;

use std::rc::Rc;

use nsh_core::prelude::*;
use zeo_ui_components::button::pill_button;
use zeo_ui_components::icons::Icon;
use zeo_ui_components::page::page_header;
use zeo_ui_components::tracks::{Columns, track_columns};
use zeo_ui_store::{format_total, play_list, store};

/// 音乐库：展示全部歌曲（页头与列头固定粘滞在顶部，参考艺术家和专辑）。
pub fn library() -> View {
    let s = store();
    let items = computed(move || {
        let tracks = s.tracks.get();
        Rc::new((*tracks.0).clone())
    });
    let count = computed(move || items.with(|v| v.len()));
    let total_sec = computed(move || items.with(|v| total_duration(v)));
    view().flex_1().child(dynamic(move || {
        if s.tracks.with(|t| t.is_empty()) {
            return no_library().into_element();
        }
        let play_all = {
            let items = items.clone();
            pill_button(Icon::Play, "播放全部", move || {
                items.with_untracked(|v| play_list(v, 0));
            })
        };
        let header = view()
            .child(page_header(
                || "音乐库".into(),
                move || format!("{} 首曲目 · {}", count.get(), format_total(total_sec.get())),
                None,
                Some(play_all.into_element()),
            ))
            .child(track_columns(Columns::ALL));
        let list = track_list_bare(items, Columns::ALL);
        view()
            .flex_1()
            .min_w(px(0.0))
            .child(header)
            .child(list)
            .into_element()
    }))
}
