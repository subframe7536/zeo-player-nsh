use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// 编号按传入顺序递增、带不带总数；大小写覆盖同名字段；三态展开；
    /// 全空跳过。
    #[test]
    pub(crate) fn batch_edits_builds_per_file() {
        let paths: Vec<String> = (1..=3).map(|i| format!("/m/{i}.mp3")).collect();
        let mut modes = HashMap::new();
        modes.insert(TagField::Artist, Tri::Set);
        modes.insert(TagField::Album, Tri::Clear);
        let mut values = HashMap::new();
        values.insert(TagField::Artist, "歌手".to_owned());
        // 编号起始 3 带总数 + 首字母大写（作用于现有标题）。
        let field_of = |_: &str, f: TagField| match f {
            TagField::Title => "my song".to_owned(),
            _ => String::new(),
        };
        let items = batch_edits(
            &paths,
            &modes,
            &values,
            Some((3, true)),
            Some(CaseOp::Title),
            &CoverMode::Keep,
            field_of,
        );
        assert_eq!(items.len(), 3);
        let get = |e: &TagEdits, f: TagField| {
            e.fields
                .iter()
                .find(|(k, _)| *k == f)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            get(&items[0].1, TagField::Track),
            Some(Edit::Set("3".to_owned()))
        );
        assert_eq!(
            get(&items[2].1, TagField::Track),
            Some(Edit::Set("5".to_owned()))
        );
        assert_eq!(
            get(&items[0].1, TagField::TrackTotal),
            Some(Edit::Set("3".to_owned()))
        );
        assert_eq!(
            get(&items[0].1, TagField::Artist),
            Some(Edit::Set("歌手".to_owned()))
        );
        assert_eq!(get(&items[0].1, TagField::Album), Some(Edit::Clear));
        // 大小写覆盖 Title（Artist 的 Set 不受影响——Artist 不在大小写范围）。
        assert_eq!(
            get(&items[0].1, TagField::Title),
            Some(Edit::Set("My Song".to_owned()))
        );
        // 全 Keep 的跳过。
        let empty = batch_edits(
            &paths,
            &HashMap::new(),
            &HashMap::new(),
            None,
            None,
            &CoverMode::Keep,
            |_, _| String::new(),
        );
        assert!(empty.is_empty());
    }

    /// 大小写：词首大写（ASCII 字母词），小写 / 大写整串转；非 ASCII
    /// 与数字原样。
    #[test]
    pub(crate) fn case_convert_rules() {
        assert_eq!(case_convert("my song", CaseOp::Title), "My Song");
        assert_eq!(case_convert("MY SONG", CaseOp::Title), "My Song");
        assert_eq!(case_convert("abc123def", CaseOp::Title), "Abc123def");
        assert_eq!(case_convert("中文abc", CaseOp::Title), "中文Abc");
        assert_eq!(case_convert("A/B", CaseOp::Lower), "a/b");
        assert_eq!(case_convert("a b", CaseOp::Upper), "A B");
    }

    #[component]
    pub(crate) fn MultiRowOverflowTest() -> impl IntoElement {
        zeo_ui_store::install();
        let p = "mock.flac".to_owned();
        let rows = signal(HashMap::new());
        let full = signal(Some((
            HashMap::from([
                (TagField::Lyrics, "line\n".repeat(100)),
                (TagField::Comment, "comment\n".repeat(50)),
            ]),
            Some("FLAC"),
            None,
        )));
        let draft = signal(SingleDraft {
            path: p.clone(),
            original: HashMap::new(),
            values: HashMap::new(),
            cover: None,
            cover_src: None,
            saving: false,
            saved: false,
            error: None,
        });
        scroll().flex_1().min_h(px(0.0)).child(
            view()
                .overflow_hidden()
                .child(single_form(p, draft, rows, full)),
        )
    }

    #[test]
    pub(crate) fn multiline_overflow_does_not_expand_scroll_content() {
        use nsh_core::geometry::{px, size};
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;
        let theme =
            nsh_base::Theme::from_json(include_str!("../../../../theme.json")).expect("theme.json");
        let mut config =
            EngineConfig::new(Root::of::<MultiRowOverflowTest>().tokens(theme.into_tokens()));
        config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
        config.viewport = size(px(400.0), px(800.0));
        let mut app = Headless::start(config).expect("start");
        app.wait_for("基本信息").unwrap();
        // Since lyrics and comment have overflow_hidden, their 100 lines of text
        // are clipped within max_h (260pt and 120pt) and do not expand the scroll content.
        let err = app.scroll("text:基本信息", px(-1.0)).unwrap_err();
        assert!(err.contains("cannot scroll by -1"));
    }

    #[component]
    pub(crate) fn SinglePanelInstantSeedTest() -> impl IntoElement {
        zeo_ui_store::install();
        let p = "mock.flac".to_owned();
        let meta = Arc::new(zeo_metadata::MetaInfo {
            title: "Mock Title".to_owned(),
            artist: "Mock Artist".to_owned(),
            lyrics: Some("[00:01.00]测试歌词".to_owned()),
            comment: Some("测试注释".to_owned()),
            ..Default::default()
        });
        let entry = TrackEntry(Arc::new(zeo_types::config::TrackShared {
            provider: zeo_types::config::ProviderId::Local,
            path: p.clone(),
            file_name: "mock.flac".to_owned(),
            album_artist: "Mock Artist".to_owned(),
            genres: vec!["二次元".to_owned(), "JPOP".to_owned()],
            size_bytes: 1024,
            modified: 0,
            stamp: 1,
            meta,
        }));
        let displayed = computed(move || Rc::new(vec![entry.clone()]));
        let rows = signal(HashMap::new());
        let selected = signal(BTreeSet::from([p.clone()]));
        single_panel(p, displayed, rows, selected, Rc::new(|| {}))
    }

    #[test]
    pub(crate) fn single_panel_prefills_lyrics_instantly_from_seed() {
        use nsh_core::geometry::{px, size};
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;
        let theme =
            nsh_base::Theme::from_json(include_str!("../../../../theme.json")).expect("theme.json");
        let mut config =
            EngineConfig::new(Root::of::<SinglePanelInstantSeedTest>().tokens(theme.into_tokens()));
        config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
        config.viewport = size(px(400.0), px(800.0));
        let mut app = Headless::start(config).expect("start");
        app.wait_for("label:歌词").unwrap();
        assert_eq!(
            app.screen().input_text("label:歌词").unwrap(),
            Some("[00:01.00]测试歌词")
        );
        assert_eq!(
            app.screen().input_text("label:注释").unwrap(),
            Some("测试注释")
        );
        assert_eq!(
            app.screen().input_text("label:标题").unwrap(),
            Some("Mock Title")
        );
        app.wait_for("text:二次元").unwrap();
        app.wait_for("text:JPOP").unwrap();
        app.wait_for("text:标签").unwrap();
        app.wait_for("text:流派：二次元; JPOP").unwrap();
    }
}
