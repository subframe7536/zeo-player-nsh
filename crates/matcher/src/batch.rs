//! 批量自动匹配调度器：支持多源瀑布回退 / 智能聚合并发查询、工作线程限流、
//! 礼貌请求间隔、实时进度回报与取消令牌。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use zeo_types::config::MatchSource;

use crate::MatcherService;
use crate::model::{
    BatchConfig, BatchItemResult, BatchMatchStatus, BatchProgress, BatchTrackInput, RankedCandidate,
};
use crate::rank::{build_query, passes_batch_threshold};

/// 批量匹配入口：最多使用 `config.concurrency`（钳到 1..=4）个工作线程处理 `tracks`。
/// 每完成一首曲目立即触发 `on_progress`；若 `cancel` 置为 true，工作线程会尽快退出。
pub fn run_batch_match<F>(
    service: Arc<MatcherService>,
    tracks: Vec<BatchTrackInput>,
    config: BatchConfig,
    cancel: Arc<AtomicBool>,
    on_progress: F,
) -> Vec<BatchItemResult>
where
    F: Fn(BatchProgress) + Send + Sync + 'static,
{
    let total = tracks.len();
    if total == 0 {
        return Vec::new();
    }
    let sources: Arc<Vec<MatchSource>> = Arc::new(
        config
            .sources
            .iter()
            .copied()
            .filter(|s| *s != MatchSource::Smart)
            .collect(),
    );
    let max_workers = config.concurrency.clamp(1, 4);
    // 智能聚合模式每首歌会并发请求所有源：按总并发预算收敛外层工作线程数，避免嵌套放大。
    let workers = if config.smart_aggregate && sources.len() > 1 {
        (max_workers / sources.len()).max(1).min(total)
    } else {
        max_workers.min(total)
    };
    let tracks = Arc::new(tracks);
    let next_idx = Arc::new(AtomicUsize::new(0));
    let results: Arc<Mutex<Vec<Option<BatchItemResult>>>> = Arc::new(Mutex::new(vec![None; total]));
    let counters = Arc::new(Mutex::new((0usize, 0usize, 0usize, 0usize, 0usize)));
    let on_progress = Arc::new(on_progress);
    let config = Arc::new(config);

    std::thread::scope(|scope| {
        for worker_id in 0..workers {
            let service = service.clone();
            let tracks = tracks.clone();
            let next_idx = next_idx.clone();
            let results = results.clone();
            let counters = counters.clone();
            let cancel = cancel.clone();
            let on_progress = on_progress.clone();
            let config = config.clone();
            let sources = sources.clone();

            scope.spawn(move || {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let idx = next_idx.fetch_add(1, Ordering::Relaxed);
                    if idx >= total {
                        break;
                    }
                    let track = &tracks[idx];
                    let item = match_one(&service, track, &config, &sources, &cancel);
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    {
                        let mut guard = results.lock().unwrap_or_else(|e| e.into_inner());
                        guard[idx] = Some(item.clone());
                    }
                    let progress = {
                        let mut c = counters.lock().unwrap_or_else(|e| e.into_inner());
                        c.0 += 1;
                        match item.status {
                            BatchMatchStatus::Matched => c.1 += 1,
                            BatchMatchStatus::NeedsReview => c.2 += 1,
                            BatchMatchStatus::NotFound => c.3 += 1,
                            BatchMatchStatus::Failed => c.4 += 1,
                        }
                        BatchProgress {
                            done: c.0,
                            total,
                            matched: c.1,
                            needs_review: c.2,
                            not_found: c.3,
                            failed: c.4,
                            latest: item,
                        }
                    };
                    on_progress(progress);

                    if config.min_delay_ms > 0 && !cancel.load(Ordering::Relaxed) {
                        // 轻量确定性抖动 + 分段睡眠以快速响应取消信号。
                        let jitter = ((idx + worker_id * 7) % 5) as u64 * 30;
                        let mut rem = config.min_delay_ms + jitter;
                        while rem > 0 && !cancel.load(Ordering::Relaxed) {
                            let step = rem.min(50);
                            std::thread::sleep(Duration::from_millis(step));
                            rem -= step;
                        }
                    }
                }
            });
        }
    });

    let mut guard = results.lock().unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *guard).into_iter().flatten().collect()
}

fn match_one(
    service: &MatcherService,
    track: &BatchTrackInput,
    config: &BatchConfig,
    sources: &[MatchSource],
    cancel: &AtomicBool,
) -> BatchItemResult {
    let query = build_query(
        &track.file_name,
        &track.title,
        &track.artist,
        &track.album,
        track.duration_ms,
    );
    if query.keyword.trim().is_empty() {
        return BatchItemResult {
            path: track.path.clone(),
            status: BatchMatchStatus::NotFound,
            candidates: Vec::new(),
            selected: None,
            error: Some("缺少可用搜索词".to_owned()),
        };
    }

    if sources.is_empty() {
        return BatchItemResult {
            path: track.path.clone(),
            status: BatchMatchStatus::Failed,
            candidates: Vec::new(),
            selected: None,
            error: Some("未选择任何数据源".to_owned()),
        };
    }

    if config.smart_aggregate {
        match service.search_multi(sources, &query, 8) {
            Ok(mut ranked) => {
                ranked.truncate(5);
                return classify(&track.path, ranked, config.strictness, None);
            }
            Err(err) => {
                return BatchItemResult {
                    path: track.path.clone(),
                    status: BatchMatchStatus::Failed,
                    candidates: Vec::new(),
                    selected: None,
                    error: Some(err),
                };
            }
        }
    }

    // 瀑布回退：按用户优先级顺序逐源查询，一旦达到阈值即停止。
    let mut best_fallback: Vec<RankedCandidate> = Vec::new();
    let mut any_ok = false;
    let mut last_err: Option<String> = None;

    for source in sources {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        match service.search(*source, &query, 10) {
            Ok(mut ranked) => {
                any_ok = true;
                if let Some(top) = ranked.first()
                    && passes_batch_threshold(top, config.strictness)
                {
                    ranked.truncate(5);
                    return BatchItemResult {
                        path: track.path.clone(),
                        status: BatchMatchStatus::Matched,
                        candidates: ranked,
                        selected: Some(0),
                        error: None,
                    };
                }
                let better = match (ranked.first(), best_fallback.first()) {
                    (Some(a), Some(b)) => a.score > b.score,
                    (Some(_), None) => true,
                    _ => false,
                };
                if better {
                    ranked.truncate(5);
                    best_fallback = ranked;
                }
            }
            Err(err) => {
                last_err = Some(err);
            }
        }
    }

    if !any_ok {
        return BatchItemResult {
            path: track.path.clone(),
            status: BatchMatchStatus::Failed,
            candidates: Vec::new(),
            selected: None,
            error: last_err,
        };
    }
    classify(&track.path, best_fallback, config.strictness, last_err)
}

fn classify(
    path: &str,
    candidates: Vec<RankedCandidate>,
    strictness: zeo_types::config::MatchStrictness,
    error: Option<String>,
) -> BatchItemResult {
    if let Some(top) = candidates.first() {
        if passes_batch_threshold(top, strictness) {
            BatchItemResult {
                path: path.to_owned(),
                status: BatchMatchStatus::Matched,
                candidates,
                selected: Some(0),
                error: None,
            }
        } else {
            BatchItemResult {
                path: path.to_owned(),
                status: BatchMatchStatus::NeedsReview,
                candidates,
                selected: None,
                error,
            }
        }
    } else {
        BatchItemResult {
            path: path.to_owned(),
            status: BatchMatchStatus::NotFound,
            candidates,
            selected: None,
            error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MatchCandidate;
    use crate::provider::MetadataProvider;
    use zeo_types::config::MatchStrictness;

    struct MockProvider {
        source: MatchSource,
        items: Vec<MatchCandidate>,
    }

    impl MetadataProvider for MockProvider {
        fn source(&self) -> MatchSource {
            self.source
        }

        fn search(&self, _keyword: &str, _limit: usize) -> Result<Vec<MatchCandidate>, String> {
            Ok(self.items.clone())
        }

        fn fetch_lyric(&self, _candidate: &MatchCandidate) -> Result<Option<String>, String> {
            Ok(Some("[00:00.00]mock lyric".into()))
        }
    }

    #[test]
    fn waterfall_falls_back_to_second_source() {
        let p1 = Arc::new(MockProvider {
            source: MatchSource::QQMusic,
            items: Vec::new(),
        });
        let p2 = Arc::new(MockProvider {
            source: MatchSource::NetEase,
            items: vec![MatchCandidate {
                source: MatchSource::NetEase,
                source_id: "186016".into(),
                ext_id: None,
                title: "晴天".into(),
                artists: vec!["周杰伦".into()],
                album: Some("叶惠美".into()),
                album_artist: None,
                year: Some("2003".into()),
                track_number: Some(3),
                disc_number: Some(1),
                duration_ms: Some(269_000),
                cover_thumb_url: None,
                cover_original_url: None,
            }],
        });
        let svc = Arc::new(MatcherService::with_providers(vec![p1, p2]));
        let cancel = Arc::new(AtomicBool::new(false));
        let progress_count = Arc::new(AtomicUsize::new(0));
        let pc = progress_count.clone();
        let results = run_batch_match(
            svc,
            vec![BatchTrackInput {
                path: "/music/晴天.flac".into(),
                file_name: "晴天.flac".into(),
                title: "晴天".into(),
                artist: "周杰伦".into(),
                album: String::new(),
                duration_ms: Some(269_200),
            }],
            BatchConfig {
                smart_aggregate: false,
                sources: vec![MatchSource::QQMusic, MatchSource::NetEase],
                strictness: MatchStrictness::Standard,
                concurrency: 1,
                min_delay_ms: 0,
            },
            cancel,
            move |_| {
                pc.fetch_add(1, Ordering::Relaxed);
            },
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, BatchMatchStatus::Matched);
        assert_eq!(
            results[0].candidates[0].candidate.source,
            MatchSource::NetEase
        );
        assert_eq!(progress_count.load(Ordering::Relaxed), 1);
    }
}
