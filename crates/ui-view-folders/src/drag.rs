use super::*;

// --- 应用内拖动 ----------------------------------------------------------------

/// 拖过这段距离（pt）才接管手势：更小的位移仍是点击。
pub(crate) const DRAG_SLOP: f32 = 6.0;

/// `source`（文件或文件夹路径）能不能落到 `target` 文件夹：不能落到自
/// 身、原来的父目录（等于没动），文件夹也不能进自己或自己的子树。
pub(crate) fn drop_ok(source: &str, dir: bool, target: &str, sep: char) -> bool {
    let src = source.trim_end_matches(sep);
    let dst = target.trim_end_matches(sep);
    if dst.is_empty() || src == dst {
        return false;
    }
    let parent = src.rfind(sep).map(|i| &src[..i]).unwrap_or("");
    if parent == dst {
        return false;
    }
    if dir && dst.starts_with(&format!("{src}{sep}")) {
        return false;
    }
    true
}

/// 把 `source` 移进 `target` 目录，返回新路径；目标里已有同名项、或文件
/// 系统拒绝（如跨磁盘 rename）时报错，由调用方提示。
pub(crate) fn move_into(source: &str, target: &str, sep: char) -> Result<String, String> {
    let name = last_segment(source);
    let dest = format!("{}{sep}{name}", target.trim_end_matches(sep));
    if std::path::Path::new(&dest).exists() {
        return Err(format!("{target} 里已有 {name}"));
    }
    std::fs::rename(source, &dest).map_err(|e| format!("移动 {name} 失败：{e}"))?;
    Ok(dest)
}

/// 一次拖动中的源条目。
#[derive(Clone, PartialEq)]
pub(crate) struct DragState {
    /// 被拖的文件 / 文件夹的绝对路径集合。
    pub(crate) sources: Vec<String>,
    /// 拖的是文件夹（移动后要修根目录、校验子树）。
    pub(crate) dir: bool,
    /// 拖动浮层上的名字。
    pub(crate) name: String,
}

/// 文件行与树行共享的拖动会话：手势接管、ghost 跟随、命中树行、落下移动。
pub(crate) struct DragCtx {
    /// `Some` 表示正在拖。
    pub(crate) active: Signal<Option<DragState>>,
    /// 命中且合法的目标树行路径（树行据此高亮）。
    pub(crate) over: Signal<Option<String>>,
    /// ghost 的锚点（拖拽起点在本页根 view 里的位置）。
    pub(crate) anchor: Signal<(f32, f32)>,
    /// 本次会话建出来还没曲目的空文件夹（移动时要跟着换前缀）。
    pub(crate) extra: Signal<BTreeSet<String>>,
    /// 当前可见树行（命中判定用）。
    pub(crate) rows: Memo<Rc<Vec<TreeRow>>>,
    /// 树列表的滚动 y 偏移。
    pub(crate) scroll: Signal<f32>,
    /// 树列表在 surface 里的位置（x, y, w），开始拖时量。
    pub(crate) geo: Rc<Cell<Option<(f32, f32, f32)>>>,
    /// 接管时记下的按下点（surface 坐标）：原生 pan 的位移基准点，
    /// ghost 锚在这里再加 nudge 才贴着指针。
    pub(crate) down: Rc<Cell<Option<Point<Points>>>>,
    /// ghost 的 translate：行的 `pan_values` 绑到这两个值，拖动的位移
    /// 由平台原生写入，每帧零引擎开销（别用 `set` 逐帧推它）。
    pub(crate) gx: AnimatedValue,
    pub(crate) gy: AnimatedValue,
    /// 测量用：树列表。
    pub(crate) list: ElementRef,
    /// 移动成功后定位（新路径或目标文件夹）。
    pub(crate) select: Rc<dyn Fn(&str)>,
    /// 结果提示（页头副标题短暂显示）。
    pub(crate) message: Signal<String>,
}

impl DragCtx {
    /// 行的拖动手势：超过 `DRAG_SLOP` 就接管；桌面端鼠标拖动不滚列表，
    /// 不会和滚动争。`block_native(false)`：行的 `pan_values` 也要跟着
    /// 这个拖走原生位移（接管默认会杀掉它）。
    pub(crate) fn claim(&self) -> TouchClaim {
        let down = self.down.clone();
        TouchClaim::on_move()
            .block_native(false)
            .ask(move |req: &ClaimRequest| {
                let d = req.event.gesture.delta;
                if d.x.0.hypot(d.y.0) > DRAG_SLOP {
                    down.set(Some(req.event.gesture.origin));
                    req.claim();
                }
            })
    }

    /// 指针（surface 坐标）下的树行路径。
    pub(crate) fn target_at(&self, page: Point<Points>) -> Option<String> {
        let (ox, oy, w) = self.geo.get()?;
        if !(ox..=ox + w).contains(&page.x.0) {
            return None;
        }
        let row_h = TREE_ROW;
        let index = ((page.y.0 - oy + self.scroll.get_untracked()) / row_h).floor();
        if index < 0.0 {
            return None;
        }
        self.rows
            .with_untracked(|r| r.get(index as usize).map(|r| r.path.clone()))
    }

    pub(crate) fn begin(&self, sources: Vec<String>, dir: bool, name: String) {
        self.active.set(Some(DragState { sources, dir, name }));
        self.over.set(None);
        // 量出来之前把 ghost 停在窗外，免得沿上次的锚点闪一下。
        self.anchor.set((-10000.0, -10000.0));
        let (down, geo, anchor) = (self.down.clone(), self.geo.clone(), self.anchor);
        let list = self.list;
        spawn(async move {
            if let Some(d) = down.get() {
                // ghost 锚在按下点（窗口坐标）+ nudge；pan
                // 值是「当前位置 - 按下点」，加起来正好贴着指针。
                anchor.set((d.x.0 + 10.0, d.y.0 + 12.0));
            }
            if let Some(m) = list.measure().await {
                geo.set(Some((m.page.x.0, m.page.y.0, m.frame.size.width.0)));
            }
        });
    }

    pub(crate) fn move_to(&self, page: Point<Points>) {
        let over = self.target_at(page).filter(|target| {
            self.active.with_untracked(|a| {
                a.as_ref()
                    .is_some_and(|d| d.sources.iter().all(|s| drop_ok(s, d.dir, target, SEP)))
            })
        });
        self.over.set(over);
    }

    /// 落下：按松手位置重算目标（`over` 只是拖动中的高亮），命中就移动；
    /// 失败留一行提示。
    pub(crate) fn finish(&self, page: Point<Points>) {
        self.move_to(page);
        let (Some(d), Some(target)) = (self.active.get_untracked(), self.over.get_untracked())
        else {
            self.cancel();
            return;
        };
        self.cancel();
        let mut moved_count = 0;
        let mut last_error = None;
        let mut last_dest = None;
        for src in &d.sources {
            // 正在播的条目先停掉：Windows 上解码器还握着文件会移不动。
            stop_if_playing_under(src);
            match move_into(src, &target, SEP) {
                Ok(dest) => {
                    // 被移走的是曲库根目录、装着根目录或在歌单 / 空文件夹
                    // 记录里：引用与索引记录跟着改路径（不重扫不重读）。
                    move_refs(src, &dest, &self.extra);
                    last_dest = Some(dest);
                    moved_count += 1;
                }
                Err(error) => {
                    last_error = Some(error);
                }
            }
        }
        if moved_count > 0 {
            if d.dir {
                if let Some(dest) = last_dest {
                    (self.select)(&dest);
                }
            } else {
                (self.select)(&target);
            }
            if let Some(error) = last_error {
                show_message(
                    self.message,
                    format!("已移动 {moved_count} 项，部分失败：{error}"),
                );
            } else if d.sources.len() == 1 {
                show_message(self.message, format!("已把 {} 移到 {target}", d.name));
            } else {
                show_message(
                    self.message,
                    format!("已把 {} 个文件移到 {target}", moved_count),
                );
            }
        } else if let Some(error) = last_error {
            show_message(self.message, error);
        }
    }

    pub(crate) fn cancel(&self) {
        self.active.set(None);
        self.over.set(None);
        // pan 值回到 0，下次 `pan_began` 从 0 起算。release 先到引擎
        // 再到平台的最后一次写入，这个 set 落地在最后。
        self.gx.set(0.0);
        self.gy.set(0.0);
        self.down.set(None);
    }
}

/// 拖放结果 / 操作提示：副标题显示几秒后自动清掉（期间被新提示顶掉则
/// 不动）。
pub(crate) fn show_message(message: Signal<String>, text: String) {
    message.set(text.clone());
    spawn(async move {
        timer(Duration::from_secs(5)).await;
        message.update(|m| {
            if *m == text {
                m.clear();
            }
        });
    });
}
