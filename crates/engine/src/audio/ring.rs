use super::*;

/// 解码线程 → 输出回调的样本环：单生产者（引擎线程）单消费者（cpal 回调），
/// 无锁。
///
/// 以前是 `Mutex<VecDeque>`：CPU 忙时引擎线程常在持锁期间被抢占，实时回调
/// 只能等它，于是输出断续。现在两边只读写原子量：`write` / `read` 是累计
/// 样本数，`discard` 之前的样本作废（seek / 换曲时由引擎设成当前 `write`，
/// 回调下次取数时跳过）。
pub struct Ring {
    pub(crate) slots: OnceLock<Box<[AtomicU32]>>,
    pub(crate) write: AtomicU64,
    pub(crate) read: AtomicU64,
    pub(crate) discard: AtomicU64,
}

impl Ring {
    pub fn new() -> Ring {
        Ring {
            slots: OnceLock::new(),
            write: AtomicU64::new(0),
            read: AtomicU64::new(0),
            discard: AtomicU64::new(0),
        }
    }

    /// 分配容量（样本数）；只在第一次调用时生效。
    pub(crate) fn init(&self, capacity: usize) {
        let _ = self
            .slots
            .set((0..capacity.max(1)).map(|_| AtomicU32::new(0)).collect());
    }

    /// 分配的容量（样本数；未分配为 0）。
    pub(crate) fn capacity(&self) -> usize {
        self.slots.get().map_or(0, |s| s.len())
    }

    /// 待播的样本数。
    pub fn len(&self) -> usize {
        let w = self.write.load(Ordering::Acquire);
        let r = self
            .read
            .load(Ordering::Acquire)
            .max(self.discard.load(Ordering::Acquire));
        w.saturating_sub(r) as usize
    }

    /// 生产者：写入放得下的整帧，返回写入的样本数。
    pub(crate) fn push(&self, data: &[f32], channels: usize) -> usize {
        let Some(slots) = self.slots.get() else {
            return 0;
        };
        let cap = slots.len() as u64;
        let free = (cap as usize).saturating_sub(self.len());
        let n = data.len().min(free);
        let n = n - n % channels.max(1);
        let w = self.write.load(Ordering::Relaxed);
        for (i, &sample) in data[..n].iter().enumerate() {
            slots[((w + i as u64) % cap) as usize].store(sample.to_bits(), Ordering::Relaxed);
        }
        self.write.store(w + n as u64, Ordering::Release);
        n
    }

    /// 生产者：作废还没播放的样本。
    pub(crate) fn clear(&self) {
        self.discard
            .store(self.write.load(Ordering::Relaxed), Ordering::Release);
    }

    /// 消费者：按顺序取最多 `max` 个样本交给 `sink(序号, 样本)`，返回取出的数量。
    pub(crate) fn pop(&self, max: usize, mut sink: impl FnMut(usize, f32)) -> usize {
        let Some(slots) = self.slots.get() else {
            return 0;
        };
        let cap = slots.len() as u64;
        let w = self.write.load(Ordering::Acquire);
        let r = self
            .read
            .load(Ordering::Relaxed)
            .max(self.discard.load(Ordering::Acquire));
        let n = (w.saturating_sub(r) as usize).min(max);
        for i in 0..n {
            sink(
                i,
                f32::from_bits(slots[((r + i as u64) % cap) as usize].load(Ordering::Relaxed)),
            );
        }
        self.read.store(r + n as u64, Ordering::Release);
        n
    }
}

impl Default for Ring {
    fn default() -> Ring {
        Ring::new()
    }
}

/// 缓冲填充目标（样本数）。
pub(crate) fn ring_target(dev_rate: u32, dev_channels: usize) -> usize {
    dev_rate as usize * RING_TARGET_SECS * dev_channels
}
