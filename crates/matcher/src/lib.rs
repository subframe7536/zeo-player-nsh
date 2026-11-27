//! `zeo-matcher`：在线元数据自动匹配引擎。
//!
//! 隔离所有外部 HTTP 请求、多平台协议适配、候选打分排序、封面临时下载与批量调度，
//! 不依赖任何 UI 或音频引擎 crate，遵循工作区自底向上分层约束。

pub mod batch;
pub mod model;
pub mod provider;
pub mod rank;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use zeo_types::config::MatchSource;

pub use batch::run_batch_match;
pub use model::*;
pub use provider::{MetadataProvider, detect_image_mime, mime_extension};
pub use rank::{
    build_query, normalize_text, parse_filename_fallback, passes_batch_threshold, rank_candidates,
};

static SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

/// 统一的元数据匹配服务（内置全部 P0/P1 数据源，也支持注入 mock provider 供测试）。
pub struct MatcherService {
    agent: ureq::Agent,
    providers: HashMap<MatchSource, Arc<dyn MetadataProvider>>,
}

impl MatcherService {
    /// 构造默认服务，注册全部内置音乐平台。
    pub fn new() -> Self {
        let agent = provider::build_agent();
        let list: Vec<Arc<dyn MetadataProvider>> = vec![
            Arc::new(provider::qqmusic::QQMusicProvider::new(agent.clone())),
            Arc::new(provider::netease::NetEaseProvider::new(agent.clone())),
            Arc::new(provider::kugou::KugouProvider::new(agent.clone())),
            Arc::new(provider::kuwo::KuwoProvider::new(agent.clone())),
            Arc::new(provider::musicbrainz::MusicBrainzProvider::new(
                agent.clone(),
            )),
        ];
        Self {
            agent,
            providers: list.into_iter().map(|p| (p.source(), p)).collect(),
        }
    }

    /// 测试或定制场景：注入自定义提供方列表。
    pub fn with_providers(list: Vec<Arc<dyn MetadataProvider>>) -> Self {
        Self {
            agent: provider::build_agent(),
            providers: list.into_iter().map(|p| (p.source(), p)).collect(),
        }
    }

    /// 按指定平台搜索并打分排序；`MatchSource::Smart` 会并发聚合默认国内四源。
    pub fn search(
        &self,
        source: MatchSource,
        query: &MatchQuery,
        limit: usize,
    ) -> Result<Vec<RankedCandidate>, String> {
        if query.keyword.trim().is_empty() {
            return Ok(Vec::new());
        }
        if source == MatchSource::Smart {
            return self.search_multi(
                &[
                    MatchSource::QQMusic,
                    MatchSource::NetEase,
                    MatchSource::Kugou,
                    MatchSource::Kuwo,
                ],
                query,
                limit,
            );
        }
        let Some(p) = self.providers.get(&source) else {
            return Err(format!("不支持的数据源: {}", source.label()));
        };
        let raw = p.search(query.keyword.trim(), limit)?;
        Ok(rank_candidates(raw, query))
    }

    /// 并发查询多个平台，合并候选并统一打分排序（取前 15 条）。
    pub fn search_multi(
        &self,
        sources: &[MatchSource],
        query: &MatchQuery,
        per_source_limit: usize,
    ) -> Result<Vec<RankedCandidate>, String> {
        let active: Vec<Arc<dyn MetadataProvider>> = sources
            .iter()
            .filter_map(|s| self.providers.get(s).cloned())
            .collect();
        if active.is_empty() {
            return Ok(Vec::new());
        }
        let keyword = query.keyword.trim().to_owned();
        let results: Vec<Result<Vec<MatchCandidate>, String>> = std::thread::scope(|scope| {
            let handles: Vec<_> = active
                .into_iter()
                .map(|p| {
                    let kw = keyword.clone();
                    scope.spawn(move || p.search(&kw, per_source_limit))
                })
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .unwrap_or_else(|_| Err("搜索线程异常退出".to_owned()))
                })
                .collect()
        });
        let mut merged = Vec::new();
        let mut any_ok = false;
        let mut last_err = None;
        for res in results {
            match res {
                Ok(items) => {
                    any_ok = true;
                    merged.extend(items);
                }
                Err(e) => last_err = Some(e),
            }
        }
        if !any_ok {
            return Err(last_err.unwrap_or_else(|| "全部数据源搜索失败".to_owned()));
        }
        let mut ranked = rank_candidates(merged, query);
        ranked.truncate(15);
        Ok(ranked)
    }

    /// 拉取指定候选的 LRC 歌词。
    pub fn fetch_lyric(&self, candidate: &MatchCandidate) -> Result<Option<String>, String> {
        let Some(p) = self.providers.get(&candidate.source) else {
            return Ok(None);
        };
        p.fetch_lyric(candidate)
    }

    /// 下载在线封面原字节与 MIME 类型。
    pub fn download_cover(&self, url: &str) -> Result<(Vec<u8>, String), String> {
        provider::download_cover_bytes(&self.agent, url)
    }

    /// 下载在线封面并落盘到临时目录，返回 `(本地路径, 原始字节, MIME)`。
    pub fn download_cover_to_file(
        &self,
        url: &str,
        dir: &Path,
        stem: &str,
    ) -> Result<(PathBuf, Vec<u8>, String), String> {
        let (bytes, mime) = self.download_cover(url)?;
        let _ = std::fs::create_dir_all(dir);
        let ext = mime_extension(&mime);
        let safe_stem: String = stem
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .take(40)
            .collect();
        let seq = SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("{safe_stem}-{seq}.{ext}"));
        std::fs::write(&path, &bytes).map_err(|e| format!("保存临时封面失败: {e}"))?;
        Ok((path, bytes, mime))
    }
}

impl Default for MatcherService {
    fn default() -> Self {
        Self::new()
    }
}

/// 全局共享默认匹配服务实例。
pub fn default_service() -> Arc<MatcherService> {
    static SVC: OnceLock<Arc<MatcherService>> = OnceLock::new();
    SVC.get_or_init(|| Arc::new(MatcherService::new())).clone()
}

/// 为单次弹窗或批量会话分配独立的临时封面目录（关闭时调用 [`cleanup_temp_dir`] 删除）。
pub fn new_session_temp_dir() -> PathBuf {
    let seq = SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("zeo-match-{pid}-{seq}"))
}

/// 清理会话临时目录（忽略不存在等错误）。
pub fn cleanup_temp_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}
