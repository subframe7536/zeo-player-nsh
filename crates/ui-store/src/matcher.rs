use std::path::PathBuf;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;

use zeo_matcher::{
    BatchConfig, BatchItemResult, BatchProgress, BatchTrackInput, MatchCandidate, MatchQuery,
    MatcherService, RankedCandidate,
};
use zeo_metadata::write::{CoverEdit, Edit, TagEdits, TagField};
use zeo_types::config::{MatchFields, MatchPrefs, MatchSource, WritePolicy};

use super::*;

static MATCHER_OVERRIDE: RwLock<Option<Arc<MatcherService>>> = RwLock::new(None);

/// 当前使用的匹配服务（测试可注入 mock，运行时默认走全局 [`zeo_matcher::default_service`]）。
pub fn matcher_service() -> Arc<MatcherService> {
    if let Ok(guard) = MATCHER_OVERRIDE.read()
        && let Some(svc) = guard.as_ref()
    {
        return svc.clone();
    }
    zeo_matcher::default_service()
}

/// 注入自定义匹配服务（无头 UI 测试用；传 `None` 恢复默认）。
pub fn set_matcher_service_override(service: Option<Arc<MatcherService>>) {
    if let Ok(mut guard) = MATCHER_OVERRIDE.write() {
        *guard = service;
    }
}

/// 当前持久化的元数据自动匹配偏好。
pub fn match_prefs() -> MatchPrefs {
    store().library.with(|l| l.config.match_prefs.clone())
}

/// 保存元数据自动匹配偏好（界面立即生效并后台落盘）。
pub fn set_match_prefs(prefs: MatchPrefs) {
    update_config(|c| c.match_prefs = prefs.clone());
    std::thread::spawn(move || library_store().set_match_prefs(prefs));
}

/// 跳转到「元数据」页并自动选中 `paths` 打开批量自动匹配弹窗（右键多选菜单入口）。
pub fn open_batch_match(paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    store().pending_batch_match.set(Some(paths));
    navigate(Nav::Metadata);
}

/// 后台搜索在线元数据候选，结果通过 `poster()` 回到主 UI 线程。
pub fn search_metadata(
    source: MatchSource,
    query: MatchQuery,
    limit: usize,
    done: impl FnOnce(Result<Vec<RankedCandidate>, String>) + Send + 'static,
) {
    let post = poster();
    let svc = matcher_service();
    std::thread::spawn(move || {
        let res = svc.search(source, &query, limit);
        post.post(move || done(res));
    });
}

/// 后台拉取选中候选的 LRC 歌词。
pub fn fetch_candidate_lyric(
    candidate: MatchCandidate,
    done: impl FnOnce(Result<Option<String>, String>) + Send + 'static,
) {
    let post = poster();
    let svc = matcher_service();
    std::thread::spawn(move || {
        let res = svc.fetch_lyric(&candidate);
        post.post(move || done(res));
    });
}

/// 后台下载在线封面至指定临时目录，返回 `(本地路径, 原始字节, MIME)`。
pub fn download_cover_temp(
    url: String,
    dir: PathBuf,
    stem: String,
    done: impl FnOnce(Result<(PathBuf, Vec<u8>, String), String>) + Send + 'static,
) {
    let post = poster();
    let svc = matcher_service();
    std::thread::spawn(move || {
        let res = svc.download_cover_to_file(&url, &dir, &stem);
        post.post(move || done(res));
    });
}

/// 后台运行批量自动匹配任务：每首完成即时经 `poster()` 推送 `on_progress`，全部完成后回调 `on_done`。
pub fn start_batch_match(
    tracks: Vec<BatchTrackInput>,
    config: BatchConfig,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(BatchProgress) + Send + Sync + 'static,
    on_done: impl FnOnce(Vec<BatchItemResult>) + Send + 'static,
) {
    let post = poster();
    let post_progress = post.clone();
    let svc = matcher_service();
    let on_progress = Arc::new(on_progress);
    std::thread::spawn(move || {
        let results = zeo_matcher::run_batch_match(svc, tracks, config, cancel, move |p| {
            let cb = on_progress.clone();
            post_progress.post(move || cb(p));
        });
        post.post(move || on_done(results));
    });
}

/// 批量写回前的准备 + 原子保存：后台拉取所需封面与歌词，按 `fields` / `write_policy`
/// 组装 `TagEdits`，最终统一走 [`save_tags_batch`] 写盘（自动释放/重载播放曲目并记录标签历史）。
pub fn apply_batch_matches(
    plans: Vec<(String, MatchCandidate)>,
    fields: MatchFields,
    write_policy: WritePolicy,
    artist_sep: String,
    on_prepare: impl Fn(usize, usize) + Send + Sync + 'static,
    on_done: impl FnOnce(BatchSaved) + Send + 'static,
) {
    let post = poster();
    let post_prep = post.clone();
    let svc = matcher_service();
    let on_prepare = Arc::new(on_prepare);
    std::thread::spawn(move || {
        let total = plans.len();
        if total == 0 {
            post.post(move || {
                on_done(BatchSaved {
                    written: Vec::new(),
                    errors: Vec::new(),
                })
            });
            return;
        }
        // 受控并发（最多 4 线程）+ 批次内按封面 URL 去重缓存，避免同专辑重复下载高清封面。
        let workers = total.min(4);
        let next_idx = std::sync::atomic::AtomicUsize::new(0);
        let done_cnt = std::sync::atomic::AtomicUsize::new(0);
        let results: std::sync::Mutex<Vec<Option<(String, TagEdits)>>> =
            std::sync::Mutex::new(vec![None; total]);
        let cover_cache: std::sync::Mutex<HashMap<String, Option<(Vec<u8>, String)>>> =
            std::sync::Mutex::new(HashMap::new());

        std::thread::scope(|scope| {
            for _ in 0..workers {
                let svc = &svc;
                let plans = &plans;
                let fields = &fields;
                let artist_sep = &artist_sep;
                let next_idx = &next_idx;
                let done_cnt = &done_cnt;
                let results = &results;
                let cover_cache = &cover_cache;
                let on_prepare = &on_prepare;
                let post_prep = &post_prep;
                scope.spawn(move || loop {
                    let idx = next_idx.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if idx >= total {
                        break;
                    }
                    let (path, cand) = &plans[idx];
                    let edits = build_batch_edits(
                        svc,
                        Path::new(path),
                        cand,
                        fields,
                        write_policy,
                        artist_sep,
                        cover_cache,
                    );
                    if !edits.is_empty() {
                        results.lock().unwrap()[idx] = Some((path.clone(), edits));
                    }
                    let done = done_cnt.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    let cb = on_prepare.clone();
                    post_prep.post(move || cb(done, total));
                });
            }
        });

        let items: Vec<(String, TagEdits)> = results
            .into_inner()
            .unwrap()
            .into_iter()
            .flatten()
            .collect();
        if items.is_empty() {
            post.post(move || {
                on_done(BatchSaved {
                    written: Vec::new(),
                    errors: Vec::new(),
                })
            });
            return;
        }
        post.post(move || {
            save_tags_batch(items, "自动匹配元数据", on_done);
        });
    });
}

fn build_batch_edits(
    svc: &MatcherService,
    path: &Path,
    cand: &MatchCandidate,
    fields: &MatchFields,
    policy: WritePolicy,
    artist_sep: &str,
    cover_cache: &std::sync::Mutex<HashMap<String, Option<(Vec<u8>, String)>>>,
) -> TagEdits {
    let existing = if policy == WritePolicy::FillEmpty {
        zeo_metadata::write::read_fields(path).unwrap_or_default()
    } else {
        HashMap::new()
    };
    let should_set = |f: TagField, val: &str| -> bool {
        if val.trim().is_empty() {
            return false;
        }
        match policy {
            WritePolicy::Overwrite => true,
            WritePolicy::FillEmpty => existing
                .get(&f)
                .map(|v| v.trim().is_empty())
                .unwrap_or(true),
        }
    };

    let mut edits = TagEdits::default();
    let mut push_if = |enabled: bool, f: TagField, val: Option<String>| {
        if !enabled {
            return;
        }
        if let Some(v) = val
            && should_set(f, &v)
        {
            edits.fields.push((f, Edit::Set(v.trim().to_owned())));
        }
    };

    push_if(fields.title, TagField::Title, Some(cand.title.clone()));
    let artist_str = cand.joined_artists(artist_sep);
    push_if(
        fields.artist,
        TagField::Artist,
        (!artist_str.is_empty()).then_some(artist_str),
    );
    push_if(fields.album, TagField::Album, cand.album.clone());
    push_if(
        fields.album_artist,
        TagField::AlbumArtist,
        cand.album_artist.clone(),
    );
    push_if(fields.year, TagField::Year, cand.year.clone());
    push_if(
        fields.track,
        TagField::Track,
        cand.track_number.map(|n| n.to_string()),
    );
    push_if(
        fields.disc,
        TagField::Disc,
        cand.disc_number.map(|n| n.to_string()),
    );

    if fields.cover
        && let Some(url) = cand.best_cover_url()
    {
        let need_cover = match policy {
            WritePolicy::Overwrite => true,
            WritePolicy::FillEmpty => {
                if fields.cover_sidecar {
                    zeo_metadata::covers::find_sidecar(path).is_none()
                } else {
                    !MetaCache::global()
                        .get(path)
                        .map(|m| m.has_art)
                        .unwrap_or(false)
                }
            }
        };
        if need_cover {
            let cached = cover_cache.lock().unwrap().get(url).cloned();
            let fetched = match cached {
                Some(v) => v,
                None => {
                    let v = svc.download_cover(url).ok();
                    cover_cache.lock().unwrap().insert(url.to_owned(), v.clone());
                    v
                }
            };
            if let Some((data, mime)) = fetched {
            let edit = CoverEdit::Replace { data, mime };
                if fields.cover_sidecar {
                    edits.cover_file = Some(edit);
                } else {
                    edits.cover = Some(edit);
                }
            }
        }
    }

    if fields.lyrics {
        let need_lyrics = match policy {
            WritePolicy::Overwrite => true,
            WritePolicy::FillEmpty => {
                if fields.lyrics_sidecar {
                    let lrc = path.with_extension("lrc");
                    !lrc.exists()
                } else {
                    existing
                        .get(&TagField::Lyrics)
                        .map(|v| v.trim().is_empty())
                        .unwrap_or(true)
                }
            }
        };
        if need_lyrics
            && let Ok(Some(lrc)) = svc.fetch_lyric(cand)
            && !lrc.trim().is_empty()
        {
            if fields.lyrics_sidecar {
                edits.lrc = Some(Edit::Set(lrc));
            } else {
                edits.fields.push((TagField::Lyrics, Edit::Set(lrc)));
            }
        }
    }

    edits
}
