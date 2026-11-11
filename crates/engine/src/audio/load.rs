use super::*;

// --- 引擎线程 ----------------------------------------------------------------

pub(crate) struct Loaded {
    pub(crate) source: Source,
    pub(crate) resampler: Option<ResamplerState>,
    pub(crate) carry: Vec<f32>, // 通道转换后的输入暂存（交错、dev 声道数）
    pub(crate) out: Vec<f32>,   // 重采样产出暂存
    /// 缓冲放不下的已解码样本（下一轮先送这些）。
    pub(crate) backlog: Vec<f32>,
    pub(crate) src_channels: usize,
    /// 解码到了文件末尾且没预取到下一首：等缓冲里的尾巴播完再换。
    pub(crate) drain: Option<Drain>,
    /// 封面还没生成（缓冲先填起来再做，免得开头卡住）。
    pub(crate) cover_pending: bool,
    /// 本曲目的 ReplayGain 标签（设置变更时重算 `Shared::rg_factor`）。
    pub(crate) rg: zeo_metadata::ReplayGain,
    /// 本次装载已记过播放统计（每次装载最多记一次）。
    pub(crate) counted: bool,
    /// 本曲已解完、正在推 `next` 的样本（换曲边界还没播到）。
    pub(crate) eof: bool,
    /// 换曲边界在 `played` 计数里的下标：本曲最后一样本的输出位置。
    /// 播到它时 `next` 转正（无缝换曲——尾巴还没播完下一首的样本
    /// 就已经排进缓冲了）。
    pub(crate) boundary: Option<u64>,
    /// 已预取的下一首：本曲解码到尾时立刻打开，样本直接接着推进
    /// 同一环形缓冲——曲间不再有排空再装载的空档。
    pub(crate) next: Option<NextTrack>,
}

/// 预取的下一首：源、重采样、RG 标签与要发布的曲目快照都先备好，
/// 边界一到只需换指针。
pub(crate) struct NextTrack {
    pub(crate) source: Source,
    pub(crate) resampler: Option<ResamplerState>,
    pub(crate) src_channels: usize,
    pub(crate) rg: zeo_metadata::ReplayGain,
    pub(crate) public: TrackPublic,
}

/// 文件解码完、等缓冲播空的状态（`len` 最近的缓冲长度，`since` 它最后
/// 一次变短的时刻，用来发现输出停摆）。
pub(crate) struct Drain {
    pub(crate) len: usize,
    pub(crate) since: Instant,
}

impl Loaded {
    /// 作废暂存（seek / 停止后）：预取的下一首一并丢掉——位置变了
    /// 边界就不算了，下一首要按新位置重新预取。
    pub(crate) fn reset_buffers(&mut self) {
        self.carry.clear();
        self.backlog.clear();
        self.drain = None;
        self.next = None;
        self.eof = false;
        self.boundary = None;
        if let Some(r) = self.resampler.as_mut() {
            // 清空延迟线与暂存，跳转点之前的样本不再输出。
            r.reset();
        }
    }
}

/// 流式重采样：解码包大小不定，攒够重采样器一块的输入才处理，余下留到
/// 下一包。（以前每包的尾巴补零送入，等于每秒插入几十段静音：断续、爆音。）
pub(crate) struct ResamplerState {
    pub(crate) fft: rubato::Fft<f32>,
    pub(crate) channels: usize,
    pub(crate) pending: Vec<f32>,
    pub(crate) output_scratch: Vec<f32>,
}

impl ResamplerState {
    pub(crate) fn new(from: u32, to: u32, channels: usize) -> Result<Self, String> {
        let fft = rubato::Fft::<f32>::new(
            from as usize,
            to as usize,
            1024,
            channels,
            rubato::FixedSync::Input,
        )
        .map_err(|e| e.to_string())?;
        Ok(ResamplerState {
            fft,
            channels,
            pending: Vec::new(),
            output_scratch: Vec::new(),
        })
    }

    /// 清空延迟线与暂存（seek 后）。
    pub(crate) fn reset(&mut self) {
        self.fft.reset();
        self.pending.clear();
    }

    /// 送入交错样本，把产出追加到 `out`。
    pub(crate) fn push(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        let ch = self.channels;
        self.pending.extend_from_slice(input);
        let mut start = 0;
        loop {
            let needed = self.fft.input_frames_next().max(1);
            if self.pending.len() - start < needed * ch {
                break;
            }
            let out_max = self.fft.output_frames_max();
            self.output_scratch.resize(out_max * ch, 0.0);
            let chunk = &self.pending[start..start + needed * ch];
            let input =
                rubato::audioadapter_buffers::direct::InterleavedSlice::new(chunk, ch, needed)
                    .map_err(|e| format!("重采样输入错误: {e}"))?;
            let mut output = rubato::audioadapter_buffers::direct::InterleavedSlice::new_mut(
                &mut self.output_scratch,
                ch,
                out_max,
            )
            .map_err(|e| format!("重采样输出错误: {e}"))?;
            let (_, written) = self
                .fft
                .process_into_buffer(&input, &mut output, None)
                .map_err(|e| format!("重采样错误: {e}"))?;
            out.extend_from_slice(&self.output_scratch[..written * ch]);
            start += needed * ch;
        }
        self.pending.drain(..start);
        Ok(())
    }
}

/// 装载一个曲目（引擎线程内）：读元数据、封面、打开解码器。
// 参数都是引擎线程循环里的局部状态，打包成结构体只会多一层转手。
#[allow(clippy::too_many_arguments)]
pub(crate) fn load_track(
    shared: &Arc<Shared>,
    media: &MediaSession,
    loaded: &mut Option<Loaded>,
    path: PathBuf,
    queue: Arc<Vec<String>>,
    queue_index_hint: i64,
    dev_rate: u32,
    dev_channels: usize,
) {
    shared.loading.store(true, Ordering::Relaxed);
    reset_ring(shared, 0.0, dev_rate);
    // 播放统计按「本次装载」归零（单曲循环重载即新的计次机会）。
    shared.listened.store(0, Ordering::Relaxed);

    let meta = match MetaCache::global().get(&path) {
        Ok(meta) => meta,
        Err(err) => {
            set_error(shared, &err);
            shared.loading.store(false, Ordering::Relaxed);
            return;
        }
    };

    match Source::open(&path) {
        Ok(source) => {
            let src_rate = source.sample_rate;
            let src_channels = source.channels;
            let resampler = if src_rate != dev_rate {
                match ResamplerState::new(src_rate, dev_rate, dev_channels) {
                    Ok(r) => Some(r),
                    Err(err) => {
                        set_error(shared, &format!("重采样器创建失败: {err}"));
                        shared.loading.store(false, Ordering::Relaxed);
                        return;
                    }
                }
            } else {
                None
            };
            // RG 标签随扫描入缓存 / 库；迁移前的老行四项全空时现场补读。
            let rg = if meta.replaygain.is_empty() {
                zeo_metadata::read_replaygain(&path)
            } else {
                meta.replaygain
            };
            let cfg = *shared.rg_config.lock().unwrap_or_else(|e| e.into_inner());
            shared
                .rg_factor
                .store(replaygain_factor(&cfg, &rg).to_bits(), Ordering::Relaxed);
            *loaded = Some(Loaded {
                source,
                resampler,
                carry: Vec::new(),
                out: Vec::new(),
                backlog: Vec::new(),
                src_channels,
                drain: None,
                cover_pending: true,
                rg,
                counted: false,
                eof: false,
                boundary: None,
                next: None,
            });

            let actual_index = if queue_index_hint >= 0 {
                queue_index_hint
            } else {
                queue
                    .iter()
                    .position(|p| p == &path.to_string_lossy())
                    .map(|i| i as i64)
                    .unwrap_or(0)
            };
            // 封面已在内存（队尾预载 / 播过的大图 URI）就直接进快照：
            // 同一帧里曲目信息与封面一起换，界面不会闪占位符；后台
            // `load_covers` 照旧跑（色调补算、媒体会话、冷缓存生成）。
            *shared.track.lock().unwrap() =
                Arc::new(track_public(&path, &meta, queue, actual_index));
            shared.ready.store(true, Ordering::Relaxed);
            // 封面由引擎循环在缓冲填起来之后生成（`load_covers`）；媒体
            // 会话的封面在 `publish` 里现查缓存，不用单独喂。
            publish_media(shared, media, true);
        }
        Err(err) => set_error(shared, &err),
    }
    shared.loading.store(false, Ordering::Relaxed);
}

/// 曲目的公开快照（装载与无缝换曲共用）：封面现查大图缓存——在内存
/// （队尾预载 / 预热 / 播过）就直接带上，没有由 cover 线程后补；
/// 大图是按需读元数据生成的 data URI，不落盘。
pub(crate) fn track_public(
    path: &std::path::Path,
    meta: &zeo_metadata::MetaInfo,
    queue: Arc<Vec<String>>,
    queue_index: i64,
) -> TrackPublic {
    let artwork = zeo_metadata::covers::cached_artwork(path);
    let title = if !meta.title.is_empty() {
        meta.title.clone()
    } else {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown".to_owned())
    };
    let artist = if !meta.artist.is_empty() {
        meta.artist.clone()
    } else {
        "Unknown Artist".to_owned()
    };
    let album = if !meta.album.is_empty() {
        meta.album.clone()
    } else {
        "Unknown Album".to_owned()
    };
    TrackPublic {
        path: Some(path.to_string_lossy().into_owned()),
        title,
        artist,
        album,
        covers_ready: artwork.is_some(),
        artwork,
        duration: meta.duration,
        queue,
        queue_index,
        error: None,
    }
}

/// 预取下一首（引擎线程，本曲尾巴还在播时调）：按循环模式定目标，
/// 读元数据、打开解码器、备好重采样器与曲目快照；封面另起线程预热，
/// 边界转正时 `cached_artwork` 多半已经命中。`None` = 没法定或打
/// 不开（走 `drain` 老路，`load_track` 会原样报错）。
pub(crate) fn prefetch_next(
    shared: &Arc<Shared>,
    dev_rate: u32,
    dev_channels: usize,
) -> Option<NextTrack> {
    let (queue, index) = {
        let t = shared.track.lock().unwrap();
        (t.queue.clone(), t.queue_index)
    };
    let next_index = advance(index, queue.len(), shared.loop_mode(), true)?;
    let path = PathBuf::from(queue.get(next_index as usize)?.clone());
    let meta = MetaCache::global().get(&path).ok()?;
    let source = Source::open(&path).ok()?;
    let resampler = if source.sample_rate != dev_rate {
        match ResamplerState::new(source.sample_rate, dev_rate, dev_channels) {
            Ok(r) => Some(r),
            Err(_) => return None,
        }
    } else {
        None
    };
    let rg = if meta.replaygain.is_empty() {
        zeo_metadata::read_replaygain(&path)
    } else {
        meta.replaygain
    };
    warm_covers(&path);
    let src_channels = source.channels;
    Some(NextTrack {
        source,
        resampler,
        src_channels,
        rg,
        public: track_public(&path, &meta, queue, next_index),
    })
}

/// 给下一首预热封面：大图 URI 与列表封面文件都生成好（纯生成、不碰
/// 共享状态）——边界转正时快照就能带上封面，少一帧占位符。
pub(crate) fn warm_covers(path: &std::path::Path) {
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let _ = zeo_metadata::covers::artwork_uri(&path);
        let _ = zeo_metadata::covers::list_cover_pub(&path);
    });
}

/// 无缝换曲的边界到了：预取的 `next` 转正。`played` 是「本曲播出位置」
/// 的累计帧计数——原子减去边界，回调在边界后推过的帧数正好续成新曲
/// 位置；`listened` 同理归零（多播出的尾巴帧直接继承成新曲收听量）。
pub(crate) fn complete_switch(l: &mut Loaded, shared: &Arc<Shared>, media: &MediaSession) {
    let Some(next) = l.next.take() else {
        l.eof = false;
        l.boundary = None;
        return;
    };
    // 输出停摆时立即转正的边界可能还没播到：按当前位置钳一下再减。
    let boundary = l
        .boundary
        .take()
        .unwrap_or(0)
        .min(shared.played.load(Ordering::Relaxed));
    let prev = shared.played.fetch_sub(boundary, Ordering::Relaxed);
    shared
        .listened
        .store(prev.saturating_sub(boundary), Ordering::Relaxed);
    l.source = next.source;
    l.resampler = next.resampler;
    l.src_channels = next.src_channels;
    l.rg = next.rg;
    let cfg = *shared.rg_config.lock().unwrap_or_else(|e| e.into_inner());
    shared
        .rg_factor
        .store(replaygain_factor(&cfg, &l.rg).to_bits(), Ordering::Relaxed);
    l.eof = false;
    l.drain = None;
    l.counted = false;
    l.cover_pending = true;
    let mut public = next.public;
    // 预热多半已把大图做进缓存：转正时再查一次，少一帧占位符。
    if public.artwork.is_none()
        && let Some(path) = public.path.as_deref().map(PathBuf::from)
    {
        public.artwork = zeo_metadata::covers::cached_artwork(&path);
        public.covers_ready = public.artwork.is_some();
    }
    *shared.track.lock().unwrap() = Arc::new(public);
    publish_media(shared, media, true);
}

/// 给当前曲目生成封面：720 px 的 JPEG `data:` URI（界面显示用，实时读
/// 元数据不落盘；全屏模糊背景复用它）；列表封面文件也顺手生成一份，
/// 系统媒体会话在 `publish` 里现查缓存拿它。读标签 + 解码缩放在冷缓存
/// 时要几百毫秒：挪出引擎线程（不然饿死音频回调、也排不进正在预热的
/// 缩略图池），线程随换曲自生自灭，`cover_gen` 让晚到的旧结果失效。
pub(crate) fn load_covers(shared: &Arc<Shared>, media: &MediaSession) {
    let Some(path) = shared.track.lock().unwrap().path.clone() else {
        return;
    };
    let gen_id = shared.cover_gen.fetch_add(1, Ordering::Relaxed) + 1;
    let shared = shared.clone();
    let media = media.clone();
    std::thread::spawn(move || {
        if shared.cover_gen.load(Ordering::Relaxed) != gen_id {
            return;
        }
        let file = std::path::Path::new(&path);
        let cover = zeo_metadata::covers::artwork_uri(file);
        // 系统媒体会话的封面在 publish 里现查列表缓存：这里顺手把文件
        // 生成出来（冷缓存 / 还没显示过的曲目才缺）。
        let _ = zeo_metadata::covers::list_cover_pub(file);
        {
            let mut slot = shared.track.lock().unwrap();
            if shared.cover_gen.load(Ordering::Relaxed) != gen_id
                || slot.path.as_deref() != Some(path.as_str())
            {
                return;
            }
            let mut t = (**slot).clone();
            t.artwork = cover;
            t.covers_ready = true;
            *slot = Arc::new(t);
        }
        publish_media(&shared, &media, true);
    });
}

/// 作废缓冲里还没播的样本，并把位置基线设为 `seconds`。
pub(crate) fn reset_ring(shared: &Arc<Shared>, seconds: f64, dev_rate: u32) {
    shared.ring.clear();
    shared.played.store(
        (seconds * dev_rate as f64).round() as u64,
        Ordering::Relaxed,
    );
}

pub(crate) fn set_error(shared: &Arc<Shared>, message: &str) {
    eprintln!("zeo-player: {message}");
    let mut slot = shared.track.lock().unwrap();
    // 同一条错误不换快照（不然解码失败的循环每秒都刷一遍界面）。
    if slot.error.as_deref() == Some(message) {
        return;
    }
    let mut t = (**slot).clone();
    t.error = Some(message.to_owned());
    *slot = Arc::new(t);
}
