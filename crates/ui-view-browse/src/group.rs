use super::*;

/// 专辑详情：头部已有封面与专辑名；序号列显示标签曲目号。
pub(crate) const ALBUM_COLS: Columns = Columns {
    number: true,
    cover: false,
    album: false,
    size: false,
    format: false,
};

// --- 艺术家 ---------------------------------------------------------------------

#[derive(Clone, PartialEq)]
pub(crate) struct Group {
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) year: Option<u32>,
    pub(crate) tracks: Rc<Vec<TrackEntry>>,
    pub(crate) cover: Option<String>,
}

impl Group {
    pub(crate) fn albums(&self) -> usize {
        self.tracks
            .iter()
            .map(|t| t.display_album())
            .collect::<HashSet<_>>()
            .len()
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
    let mut out: Vec<Group> = map
        .into_iter()
        .map(|(key, (title, subtitle, mut list))| {
            list.sort_by(|a, b| compare(&a.album, &b.album).then_with(|| by_disc_track(a, b)));
            let cover = list.iter().find(|t| t.has_art).map(|t| t.path.clone());
            let year = list.iter().filter_map(|t| t.year).max();
            Group {
                key,
                title,
                subtitle,
                year,
                tracks: Rc::new(list),
                cover,
            }
        })
        .collect();
    out.sort_by(|a, b| compare(&a.title, &b.title).then_with(|| compare(&a.subtitle, &b.subtitle)));
    out
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
        .child(when(
            move || {
                cover()
                    .and_then(|p| cover_for(&p, size.unwrap_or(168.0)))
                    .is_some()
            },
            move || {
                image(move || {
                    cover()
                        .and_then(|p| cover_for(&p, size.unwrap_or(168.0)))
                        .unwrap_or_default()
                })
                .absolute()
                .inset(px(0.0))
                .object_fit(ResizeMode::Cover)
            },
        ))
}
