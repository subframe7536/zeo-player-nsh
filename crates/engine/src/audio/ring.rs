use super::*;

/// 解码线程 → 输出回调的样本环：单生产者（引擎线程）单消费者（cpal 回调），
/// 无锁。
///
/// 以前是 `Mutex<VecDeque>`：CPU 忙时引擎线程常在持锁期间被抢占，实时回调
/// 只能等它，于是输出断续。现在两边只读写原子量：`write` / `read` 是累计
/// 样本数，`discard` 之前的样本作废（seek / 换曲时由引擎设成当前 `write`，
/// 回调下次取数时跳过）。
#[repr(align(64))]
struct CachePadded<T>(T);

pub struct Ring {
    pub(crate) slots: OnceLock<Box<[AtomicU32]>>,
    write: CachePadded<AtomicU64>,
    read: CachePadded<AtomicU64>,
    discard: CachePadded<AtomicU64>,
}

impl Ring {
    pub fn new() -> Ring {
        Ring {
            slots: OnceLock::new(),
            write: CachePadded(AtomicU64::new(0)),
            read: CachePadded(AtomicU64::new(0)),
            discard: CachePadded(AtomicU64::new(0)),
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
        let w = self.write.0.load(Ordering::Acquire);
        let r = self
            .read
            .0
            .load(Ordering::Acquire)
            .max(self.discard.0.load(Ordering::Acquire));
        w.saturating_sub(r) as usize
    }

    /// 生产者：写入放得下的整帧，返回写入的样本数（两段连续切片消除逐样本取模）。
    pub(crate) fn push(&self, data: &[f32], channels: usize) -> usize {
        let Some(slots) = self.slots.get() else {
            return 0;
        };
        let cap = slots.len();
        let free = cap.saturating_sub(self.len());
        let n = data.len().min(free);
        let n = n - n % channels.max(1);
        if n == 0 {
            return 0;
        }
        let w = self.write.0.load(Ordering::Relaxed);
        let pos = (w % cap as u64) as usize;
        let first = n.min(cap - pos);
        for (slot, &sample) in slots[pos..pos + first].iter().zip(&data[..first]) {
            slot.store(sample.to_bits(), Ordering::Relaxed);
        }
        if n > first {
            for (slot, &sample) in slots[..n - first].iter().zip(&data[first..n]) {
                slot.store(sample.to_bits(), Ordering::Relaxed);
            }
        }
        self.write.0.store(w + n as u64, Ordering::Release);
        n
    }

    /// 生产者：作废还没播放的样本。
    pub(crate) fn clear(&self) {
        self.discard
            .0
            .store(self.write.0.load(Ordering::Relaxed), Ordering::Release);
    }

    /// 消费者：按顺序取最多 `max` 个样本交给 `sink(序号, 样本)`，返回取出的数量。
    pub(crate) fn pop(&self, max: usize, mut sink: impl FnMut(usize, f32)) -> usize {
        let Some(slots) = self.slots.get() else {
            return 0;
        };
        let cap = slots.len();
        let w = self.write.0.load(Ordering::Acquire);
        let r = self
            .read
            .0
            .load(Ordering::Relaxed)
            .max(self.discard.0.load(Ordering::Acquire));
        let n = (w.saturating_sub(r) as usize).min(max);
        if n == 0 {
            return 0;
        }
        let pos = (r % cap as u64) as usize;
        let first = n.min(cap - pos);
        for (i, slot) in slots[pos..pos + first].iter().enumerate() {
            sink(i, f32::from_bits(slot.load(Ordering::Relaxed)));
        }
        if n > first {
            for (i, slot) in slots[..n - first].iter().enumerate() {
                sink(first + i, f32::from_bits(slot.load(Ordering::Relaxed)));
            }
        }
        self.read.0.store(r + n as u64, Ordering::Release);
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
