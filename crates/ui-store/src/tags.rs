use super::*;

// --- 标签写回 ------------------------------------------------------------------

/// 打开「编辑元数据」弹窗（曲目右键菜单 / 全屏「⋯」菜单）。
pub fn open_edit_info(path: &str) {
    store().edit_info.set(Some(path.to_owned()));
}

/// 「编辑元数据」→「保存」：后台线程把 `edits` 原子写回 `path`；成败都经
/// `done`（回界面侧执行）回报。正在播放的文件先释放解码器再替换、写完
/// 重载并恢复位置 / 播放态；索引与元数据缓存随后单路径刷新（曲库信号
/// 由轮询带上，不另外推）。写失败时原文件没动，照常恢复播放。
pub fn save_tags(
    path: String,
    edits: zeo_metadata::write::TagEdits,
    done: impl FnOnce(Result<(), String>) + Send + 'static,
) {
    let post = poster();
    std::thread::spawn(move || {
        let is_current = player().state().track.path.as_deref() == Some(path.as_str());
        // Release 同步等待解码器真的放下文件句柄，再开始写。
        let (position, playing) = if is_current {
            player().release_current()
        } else {
            (0.0, false)
        };
        let file = Path::new(&path);
        let snapshot = snapshot_before(&path, &edits);
        let result = zeo_metadata::write::write_tags(file, &edits);
        if result.is_ok() {
            record_history(
                "编辑元数据",
                &edits_summary([&edits]),
                snapshot.into_iter().collect(),
            );
            // 指纹变了：MetaCache 重读并把新条目排进待写队列；索引条目
            // 原地换新并重排。
            library_store().refresh_paths(std::slice::from_ref(&path));
        }
        if is_current {
            player().reload_current(position, playing);
        }
        let updated_meta = if result.is_ok() && is_current {
            MetaCache::global().get(Path::new(&path)).ok()
        } else {
            None
        };
        post.post(move || {
            // 当前曲目的详情元数据（`s.meta`）已在后台线程读取，直接写入信号避免 UI 线程读盘/查库。
            if let Some(meta) = updated_meta
                && store()
                    .track
                    .with(|t| t.path.as_deref() == Some(path.as_str()))
            {
                store().meta.set(Some(Same(meta)));
            }
            // 写同名图片时音频指纹没变，`COVER_STAMPS` 的 effect 不会
            // 重发封面请求——按已请求过的尺寸直接重发（缓存键折了侧车
            // 指纹，必然重新生成；内嵌写走音频指纹重发，这里是去重的）。
            if result.is_ok()
                && let Some((side, _)) = COVERS.with(|c| c.borrow().get(path.as_str()).copied())
            {
                zeo_metadata::covers::request(&path, side);
            }
            done(result);
        });
    });
}

/// 「批量保存」（元数据视图）：后台线程顺序写回每条 `TagEdits`；当前曲目
/// 在其中就先释放解码器再写、全部写完后重载恢复。成功的路径一次
/// `refresh_paths`；`done`（回界面侧）收到 [`BatchSaved`]。
pub fn save_tags_batch(
    items: Vec<(String, zeo_metadata::write::TagEdits)>,
    label: &str,
    done: impl FnOnce(BatchSaved) + Send + 'static,
) {
    let label = label.to_owned();
    let post = poster();
    std::thread::spawn(move || {
        let current = player().state().track.path.clone();
        // 同 save_tags：release 同步等解码器放下句柄再写，写完重载。
        let released = current
            .as_ref()
            .is_some_and(|cur| items.iter().any(|(p, _)| p == cur))
            .then(|| player().release_current());
        let mut ok_paths: Vec<String> = Vec::new();
        let mut errors: Vec<(String, String)> = Vec::new();
        let mut snapshots = Vec::new();
        // 摘要只算真正写了的改动（乱码修复解析后没乱码的文件是空改动，跳过）。
        let mut summarized: Vec<zeo_metadata::write::TagEdits> = Vec::new();
        for (path, mut edits) in items {
            // 乱码修复要读文件里的完整歌词 / 注释才知道改不改——放在这个后台线程里。
            if let Err(error) = zeo_metadata::write::resolve_redecode(Path::new(&path), &mut edits)
            {
                errors.push((path, error));
                continue;
            }
            if edits.is_empty() {
                continue;
            }
            let snapshot = snapshot_before(&path, &edits);
            summarized.push(zeo_metadata::write::TagEdits {
                fields: edits.fields.clone(),
                cover: edits
                    .cover
                    .as_ref()
                    .map(|_| zeo_metadata::write::CoverEdit::Remove),
                ..Default::default()
            });
            match zeo_metadata::write::write_tags(Path::new(&path), &edits) {
                Ok(()) => {
                    snapshots.extend(snapshot);
                    ok_paths.push(path);
                }
                Err(error) => errors.push((path, error)),
            }
        }
        record_history(&label, &edits_summary(&summarized), snapshots);
        if let Some((position, playing)) = released {
            player().reload_current(position, playing);
        }
        if !ok_paths.is_empty() {
            library_store().refresh_paths(&ok_paths);
        }
        let cur_meta = current.as_ref().and_then(|cur| {
            if ok_paths.iter().any(|p| p == cur) {
                MetaCache::global()
                    .get(Path::new(cur))
                    .ok()
                    .map(|m| (cur.clone(), m))
            } else {
                None
            }
        });
        post.post(move || {
            // 当前曲目写过：详情元数据已在后台读取，直接写信号避免 UI 线程同步 I/O。
            if let Some((cur_path, meta)) = cur_meta
                && store()
                    .track
                    .with(|t| t.path.as_deref() == Some(cur_path.as_str()))
            {
                store().meta.set(Some(Same(meta)));
            }
            // 已请求过的封面按原尺寸重发（同 save_tags：缓存键折了文件
            // 指纹必然重新生成）。
            for path in &ok_paths {
                if let Some((side, _)) = COVERS.with(|c| c.borrow().get(path.as_str()).copied()) {
                    zeo_metadata::covers::request(path, side);
                }
            }
            done(BatchSaved {
                written: ok_paths,
                errors,
            });
        });
    });
}

/// [`save_tags_batch`] 的结果：真正写了的路径（空改动的文件——比如乱码修复发现没乱码
/// ——不在其中），和失败的 `(路径, 报错)`。
#[derive(Debug, Clone, Default)]
pub struct BatchSaved {
    pub written: Vec<String>,
    pub errors: Vec<(String, String)>,
}

// --- 标签历史 ------------------------------------------------------------------

pub use zeo_db::TagBatch;

/// 最近的标签写回批次（新的在前；读数据库，别在热路径上调）。
pub fn tag_history() -> Vec<TagBatch> {
    zeo_db::tag_batches()
}

/// 这批改动动到了哪些东西（历史列表的摘要）：字段中文名去重、按字段顺序，
/// 超过 4 个折成「等 N 项」，动了封面追加「封面」。
pub fn edits_summary<'a>(
    edits: impl IntoIterator<Item = &'a zeo_metadata::write::TagEdits>,
) -> String {
    use zeo_metadata::write::{Edit, TagField};
    let mut fields: Vec<TagField> = Vec::new();
    let mut cover = false;
    for e in edits {
        cover |= e.cover.is_some();
        for (field, edit) in &e.fields {
            if *edit != Edit::Keep && !fields.contains(field) {
                fields.push(*field);
            }
        }
    }
    fields.sort();
    let mut names: Vec<String> = fields.iter().map(|f| f.label().to_owned()).collect();
    if names.len() > 4 {
        let rest = names.len() - 3;
        names.truncate(3);
        names.push(format!("等 {} 项", rest + 3));
    }
    if cover {
        names.push("封面".to_owned());
    }
    names.join("、")
}

/// 写回之前给文件拍快照：只含这次要动的字段的旧值，动封面才带旧封面。
/// 读不出（文件坏了 / 没改动）就不记。
fn snapshot_before(
    path: &str,
    edits: &zeo_metadata::write::TagEdits,
) -> Option<zeo_db::TagSnapshot> {
    use zeo_metadata::write::{Edit, read_fields};
    let touched: Vec<_> = edits
        .fields
        .iter()
        .filter(|(_, e)| *e != Edit::Keep)
        .map(|(f, _)| *f)
        .collect();
    if touched.is_empty() && edits.cover.is_none() {
        return None;
    }
    let current = read_fields(Path::new(path)).ok()?;
    Some(zeo_db::TagSnapshot {
        path: path.to_owned(),
        fields: touched
            .iter()
            .map(|f| (f.id(), current.get(f).cloned().unwrap_or_default()))
            .collect(),
        cover: edits.cover.as_ref().map(|_| {
            zeo_metadata::read_artwork(Path::new(path))
                .map(|(data, mime)| (data, mime.unwrap_or_else(|| "image/jpeg".to_owned())))
        }),
    })
}

/// 记一个历史批次（`摘要` 空就只写标签）。
fn record_history(label: &str, summary: &str, snapshots: Vec<zeo_db::TagSnapshot>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let text = if summary.is_empty() {
        label.to_owned()
    } else {
        format!("{label}：{summary}")
    };
    zeo_db::add_tag_batch(&text, now, &snapshots);
}

/// 把历史批次 `batch` 里属于 `only` 的文件还原成写回前的样子（字段旧值；动过
/// 封面的连封面一起，原来没有封面就删掉）。还原本身也走 [`save_tags_batch`]，
/// 所以同样会记成一个新历史批次（可再还原回去）。返回要还原的文件数；
/// 失败清单经 `done` 回界面侧。
pub fn restore_tag_batch(
    batch: i64,
    only: &std::collections::HashSet<String>,
    done: impl FnOnce(BatchSaved) + Send + 'static,
) -> usize {
    use zeo_metadata::write::{CoverEdit, Edit, TagEdits, TagField};
    let count = only.len();
    if count == 0 {
        done(BatchSaved::default());
        return 0;
    }
    let only = only.clone();
    let post = poster();
    // 将含封面 BLOB 的 SQLite 快照查询移至后台线程，避免阻塞 UI 线程。
    std::thread::spawn(move || {
        let items: Vec<(String, TagEdits)> = zeo_db::tag_snapshots(batch, &only)
            .into_iter()
            .map(|snap| {
                let fields = snap
                    .fields
                    .iter()
                    .filter_map(|(id, old)| {
                        let field = TagField::from_id(id)?;
                        let edit = if old.is_empty() {
                            Edit::Clear
                        } else {
                            Edit::Set(old.clone())
                        };
                        Some((field, edit))
                    })
                    .collect();
                let cover = snap.cover.map(|old| match old {
                    Some((data, mime)) => CoverEdit::Replace { data, mime },
                    None => CoverEdit::Remove,
                });
                (
                    snap.path,
                    TagEdits {
                        fields,
                        cover,
                        ..TagEdits::default()
                    },
                )
            })
            .collect();
        post.post(move || {
            if items.is_empty() {
                done(BatchSaved::default());
            } else {
                save_tags_batch(items, "还原", done);
            }
        });
    });
    count
}
