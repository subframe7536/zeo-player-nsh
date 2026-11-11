//! 「首页」：时段问候头部、随机推荐、热门艺术家、新添加、最近播放。
//! 「播放了多少」按引擎实际播出的帧数（输出回调推进）算——拖动进度条
//! 不算听过；阈值（默认 60% 或 30 秒，任一满足）在设置 →「播放」→
//! 「播放统计」里调。

pub(crate) use std::collections::HashMap;

pub(crate) use std::rc::Rc;

pub(crate) use std::time::Duration;

pub(crate) use nsh_base::{ButtonElement, button};

pub(crate) use nsh_core::prelude::*;

pub(crate) use zeo_ui_view_browse::avatar;
pub(crate) use zeo_ui_views_base::{no_library, track_menu::track_menu};

pub(crate) use zeo_types::config::TrackEntry;

pub(crate) use zeo_ui_components::button::icon_button;

pub(crate) use zeo_ui_components::cover::{cover, cover_initial, initial_letter};

pub(crate) use zeo_ui_components::icons::{Icon, icon};

pub(crate) use zeo_ui_components::page::empty_state;

pub(crate) use zeo_ui_components::tracks::{Columns, content_break, track_columns, track_row};

pub(crate) use zeo_ui_store::Nav;

pub(crate) use zeo_ui_store::theme::*;

pub(crate) use zeo_ui_store::{cover_for, covers_on, play_list, player, queue_add, store};

mod sections;
mod shelf;
mod widgets;

pub(crate) use sections::*;
pub(crate) use shelf::*;
pub(crate) use widgets::*;

/// 千分位（页头与统计卡的大数）。

#[component]
pub fn Home() -> impl IntoElement {
    let s = store();

    // 随机推荐的一批：进页与曲库成员变化时各抽一次（批里的曲目还在
    // 曲库就不动），卡上 Shuffle 钮手动重抽。
    let featured = signal(Rc::new(Vec::<TrackEntry>::new()));
    effect(move || {
        let tracks = s.tracks.get();
        let stale = featured.with(|b| {
            b.is_empty() != tracks.is_empty()
                || b.iter().any(|t| !tracks.iter().any(|x| x.path == t.path))
        });
        if stale {
            featured.set(Rc::new(pick(&tracks, 6)));
        }
    });

    // 播放统计：进页先读一次；引擎每记一次播放（plays_gen +1）重读。
    // 两秒轮询代次——记一次的开销远小于每次整表重查。同一轮询顺手
    // 刷新 `now`：问候语跨整点、指标跨天都自动换。
    let stats = signal(zeo_db::PlayStats::default());
    let now = signal(zeo_types::config::unix_now());
    let reload = move || {
        spawn(async move {
            let now = zeo_types::config::unix_now();
            let offset = zeo_types::config::local_offset_seconds();
            let since = now.saturating_sub(7 * 86400);
            let week_start = zeo_types::config::local_week_start(now, offset);
            if let Ok(v) = blocking(move || zeo_db::play_stats(since, week_start, offset, 10)).await
            {
                stats.set(v);
            }
        });
    };
    reload();
    spawn(async move {
        let mut seen = player().plays_generation();
        let mut seen_added = zeo_db::added_generation();
        loop {
            timer(Duration::from_secs(2)).await;
            now.set(zeo_types::config::unix_now());
            let n = player().plays_generation();
            let a = zeo_db::added_generation();
            if n != seen || a != seen_added {
                (seen, seen_added) = (n, a);
                reload();
            }
        }
    });

    view().flex_1().child(dynamic(move || {
        if s.tracks.with(|t| t.is_empty()) {
            return no_library().into_element();
        }
        view()
            .flex_1()
            .child(hero(stats, now))
            .child(
                scroll().flex_1().child(
                    view()
                        .px(px(32.0))
                        .pb(px(BAR_SPACE))
                        .gap(px(28.0))
                        .child(featured_card(featured))
                        .child(recent(stats))
                        .child(hot_artists(stats))
                        .child(new_added(stats)),
                ),
            )
            .into_element()
    }))
}
