//! 纯文件模板重命名弹窗：模板输入（禁目录分隔符）、实时改名预览
//! （同目录改名 + 冲突拦截）、后台执行（音频 + 同名 `.lrc` / 封面
//! 侧车），成功后经 `LibraryStore::move_path` 换索引 / 歌单引用。

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use nsh_core::prelude::*;

use super::MetaRow;
use zeo_library::config::library_store;
use zeo_metadata::rename::{self, Planned, Status, Template};
use zeo_metadata::write::TagField;
use zeo_types::config::TrackEntry;
use zeo_ui_components::button::{Size, Variant, text_button};
use zeo_ui_components::form::{dialog_content, dialog_footer, dialog_header, input_field, modal};
use zeo_ui_store::theme::*;
use zeo_ui_store::{self as store, poster};
use zeo_ui_view_folders::stop_if_playing_under;

/// 对话框：`open` 为真时挂载（页面子树内的 modal 覆盖整窗）。
pub fn dialog(
    open: Signal<bool>,
    displayed: Memo<Rc<Vec<TrackEntry>>>,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    selected: Signal<BTreeSet<String>>,
) -> AnyElement {
    let input = signal("{track:02d} - {title}".to_owned());
    let running = signal(false);
    let errors = signal(Vec::<String>::new());
    let template = computed(move || Template::parse(&input.get()));
    // 已选路径按当前显示序；未加载的行回落 TrackEntry 索引值。
    let items = computed(move || {
        let Ok(tpl) = template.get() else {
            return Vec::<Planned>::new();
        };
        let set = selected.get();
        let map = rows.get();
        let items: Vec<(String, HashMap<TagField, String>)> = displayed
            .get()
            .iter()
            .filter(|t| set.contains(&t.path))
            .map(|t| (t.path.clone(), fields_of(t, &map)))
            .collect();
        rename::plan(&items, &tpl)
    });
    let ok_count = move || items.with(|v| v.iter().filter(|p| p.status == Status::Ok).count());
    let bad_count = move || {
        items.with(|v| {
            v.iter()
                .filter(|p| matches!(p.status, Status::Conflict(_) | Status::Invalid(_)))
                .count()
        })
    };
    let close = move || open.set(false);
    let execute = move || {
        let todo: Vec<Planned> = items
            .get_untracked()
            .into_iter()
            .filter(|p| p.status == Status::Ok)
            .collect();
        if todo.is_empty() {
            return;
        }
        running.set(true);
        errors.set(Vec::new());
        let post = poster();
        std::thread::spawn(move || {
            let mut results = Vec::new();
            for item in &todo {
                // 正播放的源先停（Windows 文件句柄占用），再改。
                stop_if_playing_under(&item.src);
                let (moves, error) = rename::apply(item);
                results.push((item.src.clone(), item.dest.clone(), moves, error));
            }
            post.post(move || {
                let mut failures = Vec::new();
                for (src, dest, moves, error) in &results {
                    if moves.iter().any(|(o, n)| o == src && n == dest) {
                        // 与 folders move_refs 同处（界面侧）：索引 +
                        // 歌单引用原位换路径。
                        library_store().move_path(src, dest);
                        store::move_playlist_paths(src, dest);
                        selected.update(|set| {
                            if set.remove(src) {
                                set.insert(dest.clone());
                            }
                        });
                        rows.update(|r| {
                            if let Some(row) = r.remove(src) {
                                r.insert(dest.clone(), row);
                            }
                        });
                    }
                    if let Some(error) = error {
                        failures.push(format!("{src}: {error}"));
                    }
                }
                running.set(false);
                if failures.is_empty() {
                    open.set(false);
                } else {
                    errors.set(failures);
                }
            });
        });
    };
    let content = dialog_content(560.0)
        .child(dialog_header(
            "重命名文件",
            Some("只修改文件名，文件留在原文件夹。同名 .lrc 与封面图片一起改名。"),
        ))
        .child(
            view()
                .gap(px(6.0))
                .child(
                    input_field(false)
                        .flex_1()
                        .label("模板")
                        .default_value("{track:02d} - {title}")
                        .placeholder("{track:02d} - {title}")
                        .on_change(move |t: &str| input.set(t.to_owned())),
                )
                .child(
                    text("可用占位符：{title} {artist} {album} {album_artist} {genre} {year} {track} {track_total} {disc} {disc_total} {composer} {conductor} {lyricist} {publisher} {filename}；补零 {track:02d}")
                        .text_size(px(11.0))
                        .line_height(px(15.0))
                        .text_color(faint()),
                )
                .child(when(
                    move || template.get().is_err(),
                    move || {
                        text(move || {
                            template.get().err().unwrap_or_default()
                        })
                        .text_size(px(12.0))
                        .line_height(px(16.0))
                        .text_color(nsh_base::theme().destructive)
                    },
                )),
        )
        .child(
            text(move || format!("{} 个将改名 · {} 个冲突", ok_count(), bad_count()))
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg()),
        )
        .child(scroll().max_h(px(320.0)).child(dynamic(move || {
            view()
                .gap(px(2.0))
                .children(items.get().iter().map(preview_row))
                .into_element()
        })))
        .child(when(
            move || errors.with(|v| !v.is_empty()),
            move || {
                view().gap(px(2.0)).children(
                    errors
                        .get()
                        .iter()
                        .map(|e| {
                            text(e.clone())
                                .text_size(px(12.0))
                                .line_height(px(16.0))
                                .text_color(nsh_base::theme().destructive)
                                .into_element()
                        })
                        .collect::<Vec<_>>(),
                )
            },
        ))
        .child(
            dialog_footer()
                .child(text_button(Variant::Outline, Size::Default, None, "取消", close))
                .child(
                    text_button(Variant::Default, Size::Default, None, "重命名", execute)
                        // 单选面板顶栏的铅笔也叫「重命名」——弹窗按钮另
                        // 取名，免得选择器同时命中两颗。
                        .accessibility_label("确认重命名")
                        .disabled(move || {
                            running.get() || ok_count() == 0 || template.get().is_err()
                        }),
                ),
        );
    modal(content, close)
}

/// 一条预览：`旧文件名 → 新文件名` + 状态（正常 / 无变化 / 原因）。
fn preview_row(item: &Planned) -> impl IntoElement {
    let name = |p: &str| {
        Path::new(p)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let (status, bad) = match &item.status {
        Status::Ok => ("正常".to_owned(), false),
        Status::Unchanged => ("无变化".to_owned(), false),
        Status::Conflict(m) | Status::Invalid(m) => (m.clone(), true),
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text(format!("{} → {}", name(&item.src), name(&item.dest)))
                .flex_1()
                .number_of_lines(1)
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(fg()),
        )
        .child(
            text(status)
                .text_size(px(11.0))
                .line_height(px(11.0))
                .text_color(move || {
                    if bad {
                        nsh_base::theme().destructive
                    } else {
                        faint()
                    }
                }),
        )
}

/// 一条的模板字段：行已加载直接用，未加载回落 TrackEntry 索引值（避免界面线程同步走磁盘 I/O 读文件）。
fn fields_of(t: &TrackEntry, map: &HashMap<String, Arc<MetaRow>>) -> HashMap<TagField, String> {
    if let Some(row) = map.get(&t.path) {
        return row.fields.clone();
    }
    let mut fields = HashMap::new();
    if !t.title.is_empty() {
        fields.insert(TagField::Title, t.title.clone());
    }
    if !t.artist.is_empty() {
        fields.insert(TagField::Artist, t.artist.clone());
    }
    if !t.album.is_empty() {
        fields.insert(TagField::Album, t.album.clone());
    }
    if !t.meta.album_artist.is_empty() {
        fields.insert(TagField::AlbumArtist, t.meta.album_artist.clone());
    }
    if !t.genres.is_empty() {
        fields.insert(TagField::Genre, t.genres.join("; "));
    }
    if let Some(y) = t.year {
        fields.insert(TagField::Year, y.to_string());
    }
    if let Some(trk) = t.track {
        fields.insert(TagField::Track, trk.to_string());
    }
    if let Some(tot) = t.track_total {
        fields.insert(TagField::TrackTotal, tot.to_string());
    }
    if let Some(d) = t.disk {
        fields.insert(TagField::Disc, d.to_string());
    }
    if let Some(tot) = t.disk_total {
        fields.insert(TagField::DiscTotal, tot.to_string());
    }
    if !t.composers.is_empty() {
        fields.insert(TagField::Composer, t.composers.join("; "));
    }
    fields
}
