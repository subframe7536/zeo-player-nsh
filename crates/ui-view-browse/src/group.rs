use super::*;

/// 专辑详情：头部已有封面与专辑名；序号列显示标签曲目号。
pub(crate) const ALBUM_COLS: Columns = Columns {
    number: true,
    index: false,
    cover: false,
    album: false,
    size: false,
    format: false,
};

/// 艺术家 / 专辑网格共用的列数：内容两侧各让 `PAGE_X - CARD_PAD`
///（封面与页标题同一条内容线），卡宽节奏 184、卡间按 `CARD_PAD` 计。
pub(crate) fn grid_columns() -> u32 {
    let width = content_break() - 2.0 * (PAGE_X - CARD_PAD);
    ((width + CARD_PAD) / (184.0 + CARD_PAD))
        .floor()
        .clamp(2.0, 10.0) as u32
}

// --- 艺术家 ---------------------------------------------------------------------

#[derive(Clone, PartialEq)]
pub(crate) struct Group {
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) year: Option<u32>,
    pub(crate) album_count: usize,
    pub(crate) tracks: Rc<Vec<TrackEntry>>,
    pub(crate) cover: Option<String>,
}

impl Group {
    pub(crate) fn albums(&self) -> usize {
        self.album_count
    }
}

pub(crate) fn group_by(
    tracks: &[TrackEntry],
    key: impl Fn(&TrackEntry) -> Vec<(String, String, String)>,
) -> Vec<Group> {
    let mut map: HashMap<String, (String, String, Vec<TrackEntry>)> = HashMap::new();
    for track in tracks {
        for (k, title, subtitle) in key(track) {
            map.entry(k)
                .or_insert_with(|| (title, subtitle, Vec::new()))
                .2
                .push(track.clone());
        }
    }
    let mut keyed: Vec<(String, String, Group)> = map
        .into_iter()
        .map(|(key, (title, subtitle, mut list))| {
            // 预计算小写专辑名键，避免 O(N log N) 排序比较中反复 to_lowercase() 堆分配。
            let mut decorated: Vec<(String, TrackEntry)> = list
                .drain(..)
                .map(|t| (t.album.to_lowercase(), t))
                .collect();
            decorated.sort_by(|(ka, a), (kb, b)| ka.cmp(kb).then_with(|| by_disc_track(a, b)));
            let list: Vec<TrackEntry> = decorated.into_iter().map(|(_, t)| t).collect();
            let cover = list.iter().find(|t| t.has_art).map(|t| t.path.clone());
            let year = list.iter().filter_map(|t| t.year).max();
            let album_count = list
                .iter()
                .map(|t| t.display_album())
                .collect::<HashSet<_>>()
                .len();
            let k_title = title.to_lowercase();
            let k_sub = subtitle.to_lowercase();
            (
                k_title,
                k_sub,
                Group {
                    key,
                    title,
                    subtitle,
                    year,
                    album_count,
                    tracks: Rc::new(list),
                    cover,
                },
            )
        })
        .collect();
    keyed.sort_by(|(ta, sa, _), (tb, sb, _)| ta.cmp(tb).then_with(|| sa.cmp(sb)));
    keyed.into_iter().map(|(_, _, g)| g).collect()
}

/// 圆形头像：封面或首字。`size` 为 `None` 时铺满宽度、正方形。
pub fn avatar(
    cover: impl Fn() -> Option<String> + Copy + 'static,
    name: impl Fn() -> String + 'static,
    size: Option<f32>,
) -> View {
    let frame = view()
        .rounded_full()
        .overflow_hidden()
        .bg(muted())
        .items_center()
        .justify_center();
    let frame = match size {
        Some(size) => frame.size(px(size)),
        None => frame.w_full().aspect_square(),
    };
    let glyph = size.map_or(64.0, |s| s * 0.4);
    frame
        .child(
            text(move || name().chars().next().map(String::from).unwrap_or_default())
                .text_size(px(glyph))
                .line_height(px(glyph))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted_fg()),
        )
        .child({
            let resolved = computed(move || {
                cover().and_then(|p| cover_for(&p, size.unwrap_or(168.0)))
            });
            when(
                move || resolved.with(Option::is_some),
                move || {
                    let last = stored(None::<SharedString>);
                    let trail = computed(move || {
                        let now = resolved.get().unwrap_or_default();
                        let prev = last.replace(Some(now.clone()));
                        prev.filter(|p| !p.is_empty()).unwrap_or(now)
                    });
                    image(move || resolved.get().unwrap_or_default())
                        .placeholder(move || trail.get())
                        .absolute()
                        .inset(px(0.0))
                        .object_fit(ResizeMode::Cover)
                },
            )
        })
}
