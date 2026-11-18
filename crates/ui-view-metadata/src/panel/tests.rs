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
            paths.len(),
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
            paths.len(),
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

    fn entry(path: &str, title: &str, artist: &str, track: u32) -> TrackEntry {
        let meta = Arc::new(zeo_metadata::MetaInfo {
            title: title.to_owned(),
            artist: artist.to_owned(),
            track: Some(track),
            ..Default::default()
        });
        TrackEntry(Arc::new(zeo_types::config::TrackShared {
            provider: zeo_types::config::ProviderId::Local,
            path: path.to_owned(),
            file_name: path.rsplit('/').next().unwrap().to_owned(),
            album_artist: String::new(),
            genres: Vec::new(),
            artists: Vec::new(),
            size_bytes: 1,
            modified: 0,
            stamp: 1,
            meta,
        }))
    }

    /// 效果预览：大小写显示 `旧 → 新`；重命名按应用后的标签渲染（模板里
    /// 的 `{title}` 取大小写格式化后的值）；坏模板标红；没启用的工具无预览。
    #[test]
    pub(crate) fn preview_shows_case_and_rename_effects() {
        let dir = std::env::temp_dir().join(format!("zeo-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = |name: &str| dir.join(name).to_string_lossy().replace('\\', "/");
        let sample = vec![
            entry(&path("a.flac"), "my song", "me", 1),
            entry(&path("b.flac"), "Other", "Me", 2),
        ];
        let rows = HashMap::new();
        let mut d = BatchDraft::default();
        assert_eq!(build_preview(&d, &sample, 2, &rows), Preview::default());

        d.case = Some(CaseOp::Title);
        d.rename_tpl = "{track:02d} - {title}".to_owned();
        let p = build_preview(&d, &sample, 2, &rows);
        assert_eq!(p.case[0].text, "标题：my song → My Song");
        // 第二首标题已是首字母大写：只剩没变的提示。
        assert!(p.case[1].text.ends_with("（无变化）"), "{:?}", p.case);
        assert!(
            p.rename[0].text.ends_with("01 - My Song.flac"),
            "{:?}",
            p.rename
        );
        assert!(
            p.rename[1].text.ends_with("02 - Other.flac"),
            "{:?}",
            p.rename
        );
        assert!(p.rename.iter().all(|l| !l.bad));

        d.rename_tpl = "{nope}".to_owned();
        let p = build_preview(&d, &sample, 2, &rows);
        assert!(p.rename[0].bad, "{:?}", p.rename);
        assert!(template_error(&d.rename_tpl).is_some());

        d.num_start = "x".to_owned();
        assert!(build_preview(&d, &sample, 2, &rows).number[0].bad);
        d.num_start = "3".to_owned();
        let p = build_preview(&d, &sample, 5, &rows);
        assert_eq!(p.number[0].text, "音轨号 3 – 7");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 面板最窄宽度（`PANEL_MIN_W`）下的批量面板。
    #[component]
    pub(crate) fn NarrowBatchPanelTest() -> impl IntoElement {
        zeo_ui_store::install();
        let paths = Rc::new(vec!["a.flac".to_owned(), "b.flac".to_owned()]);
        let displayed = computed(|| Rc::new(Vec::<TrackEntry>::new()));
        let rows = signal(HashMap::new());
        let selected = signal(BTreeSet::new());
        view()
            .w(px(240.0))
            .h(px(800.0))
            .test_id("host")
            .child(batch_panel(
                paths,
                displayed,
                rows,
                selected,
                Rc::new(|| {}),
            ))
    }

    /// 窄面板里批量表单的控件折行而不是溢出：任何可见节点都不越过面板右缘。
    #[test]
    pub(crate) fn batch_panel_wraps_controls_instead_of_overflowing() {
        use nsh_core::geometry::{px, size};
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;
        let theme =
            nsh_base::Theme::from_json(include_str!("../../../../theme.json")).expect("theme.json");
        let mut config =
            EngineConfig::new(Root::of::<NarrowBatchPanelTest>().tokens(theme.into_tokens()));
        config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
        config.viewport = size(px(400.0), px(800.0));
        let mut app = Headless::start(config).expect("start");
        app.wait_for("text:音轨编号").unwrap();
        let nodes = app.screen().layout_nodes();
        let host = nodes
            .iter()
            .find(|n| n.test_id.as_deref() == Some("host"))
            .expect("host")
            .bounds;
        let right = host.origin.x.0 + host.size.width.0;
        // 行首标签与同行的矮控件（按钮）在同一条水平中线上；「封面」不再有
        // 独立的节标题（整个面板只有行首这一个）。
        let center_y = |text: &str| {
            let hits: Vec<_> = nodes
                .iter()
                .filter(|n| n.text.as_ref().is_some_and(|t| t.text.text == text))
                .collect();
            assert_eq!(hits.len(), 1, "{text}: {} 处", hits.len());
            hits[0].bounds.origin.y.0 + hits[0].bounds.size.height.0 / 2.0
        };
        let (label, button) = (center_y("封面"), center_y("替换…"));
        assert!(
            (label - button).abs() <= 1.0,
            "封面标签与按钮未对齐：{label} vs {button}"
        );
        for n in nodes.iter().filter(|n| !n.hidden) {
            let edge = n.bounds.origin.x.0 + n.bounds.size.width.0;
            assert!(
                edge <= right + 0.5,
                "{:?} {:?} overflows the panel: {edge} > {right}",
                n.kind,
                n.text.as_ref().map(|t| &t.text)
            );
        }
    }

    /// 重命名行是一颗按钮，点开模板弹窗：有占位符速查；点预设整个替换模板，
    /// 点占位符追加；「完成」收起后按钮上显示当前模板。
    #[test]
    pub(crate) fn rename_button_opens_dialog_with_presets_and_cheatsheet() {
        use nsh_core::geometry::{px, size};
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;
        let theme =
            nsh_base::Theme::from_json(include_str!("../../../../theme.json")).expect("theme.json");
        let mut config =
            EngineConfig::new(Root::of::<NarrowBatchPanelTest>().tokens(theme.into_tokens()));
        config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
        config.viewport = size(px(600.0), px(800.0));
        let mut app = Headless::start(config).expect("start");
        app.wait_for("text:设置重命名模板…").unwrap();
        app.press("label:重命名模板").unwrap();
        app.wait_for("text:{filename}").unwrap();
        app.press("text:{artist} - {title}").unwrap();
        assert_eq!(
            app.screen().input_text("label:模板").unwrap(),
            Some("{artist} - {title}")
        );
        app.press("label:插入 {year}").unwrap();
        assert_eq!(
            app.screen().input_text("label:模板").unwrap(),
            Some("{artist} - {title}{year}")
        );
        app.press("完成").unwrap();
        app.wait_for_gone("label:模板", std::time::Duration::from_secs(5))
            .unwrap();
        app.wait_for("text:{artist} - {title}{year}").unwrap();
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
            artists: Vec::new(),
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
