use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use super::{TINY, artwork_uri, list_cover};

/// 已完成的（曲目路径, 实际尺寸, 缩略图路径），工作线程写、界面取走。
type Done = Arc<Mutex<Vec<(String, u32, Option<String>)>>>;

const WORKERS: usize = 3;

/// 工作任务：`Thumb` 只生成列表缩略图（第二元是需要的尺寸，已有
/// 更大的直接复用——每个音频文件最多一份缓存封面），`Warm` 生成
/// 全部尺寸（队尾预载——当前曲目的后三首，换歌时大封面 / 背景不再
/// 现算）。
#[derive(Clone, PartialEq, Eq, Hash)]
enum Job {
    Thumb(String, u32),
    Warm(String),
}

/// 工作队列：界面可见与队尾预载的请求从队首进（高优先级），扫描
/// 预热从队尾进（低优先级）；`queued` 按（类型, 路径）去重，已在
/// 队列里的重复请求直接丢掉。
struct Pool {
    q: Arc<(Mutex<VecDeque<Job>>, Condvar)>,
    queued: Arc<Mutex<HashSet<Job>>>,
    done: Done,
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        let q = Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        let queued = Arc::new(Mutex::new(HashSet::new()));
        let done = Arc::new(Mutex::new(Vec::new()));
        for i in 0..WORKERS {
            let q = q.clone();
            let queued = queued.clone();
            let done = done.clone();
            std::thread::Builder::new()
                .name(format!("zeo-player-cover-{i}"))
                .spawn(move || {
                    let (m, cv) = &*q;
                    loop {
                        let job = {
                            let mut q = m.lock().unwrap();
                            loop {
                                if let Some(j) = q.pop_front() {
                                    break j;
                                }
                                q = cv.wait(q).unwrap();
                            }
                        };
                        // 先出标记再干活：处理中的重请求会再入队（文件可
                        // 能变了，值得重算），排队中的重请求则丢掉。
                        queued.lock().unwrap().remove(&job);
                        match job {
                            Job::Thumb(path, side) => {
                                let got = list_cover(Path::new(&path), side);
                                done.lock().unwrap().push((
                                    path,
                                    got.as_ref().map(|(s, _)| *s).unwrap_or(side),
                                    got.map(|(_, p)| p.to_string_lossy().into_owned()),
                                ));
                            }
                            Job::Warm(path) => {
                                let p = Path::new(&path);
                                let got = list_cover(p, TINY);
                                artwork_uri(p);
                                done.lock().unwrap().push((
                                    path,
                                    got.as_ref().map(|(s, _)| *s).unwrap_or(TINY),
                                    got.map(|(_, p)| p.to_string_lossy().into_owned()),
                                ));
                            }
                        }
                    }
                })
                .expect("spawn cover worker");
        }
        Pool { q, queued, done }
    })
}

fn enqueue(job: Job, front: bool) {
    let pool = pool();
    if !pool.queued.lock().unwrap().insert(job.clone()) {
        return;
    }
    let (m, cv) = &*pool.q;
    let mut q = m.lock().unwrap();
    if front {
        q.push_front(job);
    } else {
        q.push_back(job);
    }
    cv.notify_one();
}

/// 请求 `path` 的列表封面（结果经 [`take_done`] 取回）：`side` 是
/// 需要的边长，已缓存的更大文件直接复用（每个音频文件最多一份缓存
/// 封面，升级后小文件由 [`prune_smaller`] 删掉）。工作线程顺手算封面
/// 主色调写库，色调结果经 [`take_dominant`] 取回。高优先级：界面
/// 可见的封面插到队首，不排在扫描预热后面。
pub fn request(path: &str, side: u32) {
    enqueue(Job::Thumb(path.to_owned(), side), true);
}

/// 队尾预载：`path` 的全部封面尺寸（缩略图 + 大封面 + 背景）都提前
/// 生成好；同样插队首，排在扫描预热前面。
pub fn request_warm(path: &str) {
    enqueue(Job::Warm(path.to_owned()), true);
}

/// 低优先级请求（扫描时给整库预热缩略图）：只出 [`TINY`]——行级插槽
/// 够用且生成快，需要大图的曲目由界面请求时升档重算。排到队尾。
pub fn request_low(path: &str) {
    enqueue(Job::Thumb(path.to_owned(), TINY), false);
}

/// 取走已完成的（曲目路径, 实际尺寸, 缩略图路径）。
pub fn take_done() -> Vec<(String, u32, Option<String>)> {
    std::mem::take(&mut *pool().done.lock().unwrap())
}
