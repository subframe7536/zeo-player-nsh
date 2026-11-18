use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// 大小写覆盖同名字段；三态展开；全空跳过。
    #[test]
    pub(crate) fn batch_edits_builds_per_file() {
        let paths: Vec<String> = (1..=3).map(|i| format!("/m/{i}.mp3")).collect();
        let mut modes = HashMap::new();
        modes.insert(TagField::Artist, Tri::Set);
        modes.insert(TagField::Album, Tri::Clear);
        let mut values = HashMap::new();
        values.insert(TagField::Artist, "歌手".to_owned());
        // 首字母大写（作用于现有标题）。
        let field_of = |_: &str, f: TagField| match f {
            TagField::Title => "my song".to_owned(),
            _ => String::new(),
        };
        let items = batch_edits(
            &paths,
            &modes,
            &values,
            Some(CaseOp::Title),
            false,
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
            false,
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

    /// 乱码修复：Latin-1 乱码按所选编码还原，三态已定值的字段不动，大小写
    /// 作用在修复之后的值上；预览显示 `乱码 → 修复后`。
    #[test]
    pub(crate) fn encoding_fix_runs_before_case_and_respects_tri_state() {
        // 「周杰伦」的 GBK 字节被当 Latin-1 读出来。
        let garbled = "\u{d6}\u{dc}\u{bd}\u{dc}\u{c2}\u{d7}";
        let sample = vec![entry("/m/a.mp3", "my song", garbled, 1)];
        let path = vec!["/m/a.mp3".to_owned()];
        let get = |items: &[(String, TagEdits)], f: TagField| {
            items[0]
                .1
                .fields
                .iter()
                .find(|(k, _)| *k == f)
                .map(|(_, e)| e.clone())
        };
        let field_of = |_: &str, f: TagField| match f {
            TagField::Title => "my song".to_owned(),
            TagField::Artist => garbled.to_owned(),
            _ => String::new(),
        };
        let run = |modes: &HashMap<TagField, Tri>, values: &HashMap<TagField, String>| {
            batch_edits(
                &path,
                modes,
                values,
                Some(CaseOp::Title),
                true,
                &CoverMode::Keep,
                field_of,
            )
        };
        let items = run(&HashMap::new(), &HashMap::new());
        assert_eq!(
            get(&items, TagField::Artist),
            Some(Edit::Set("周杰伦".to_owned()))
        );
        assert_eq!(
            get(&items, TagField::Title),
            Some(Edit::Set("My Song".to_owned()))
        );
        // 歌词与注释的完整文本在文件里：留给写回时解析（Redecode）。
        assert_eq!(get(&items, TagField::Lyrics), Some(Edit::Redecode));
        assert_eq!(get(&items, TagField::Comment), Some(Edit::Redecode));
        // 艺术家、注释被三态定了值：乱码修复不碰它们。
        let modes = HashMap::from([
            (TagField::Artist, Tri::Set),
            (TagField::Comment, Tri::Clear),
        ]);
        let values = HashMap::from([(TagField::Artist, "X".to_owned())]);
        let items = run(&modes, &values);
        assert_eq!(
            get(&items, TagField::Artist),
            Some(Edit::Set("X".to_owned()))
        );
        assert_eq!(get(&items, TagField::Comment), Some(Edit::Clear));

        let mut d = BatchDraft::default();
        d.fix_encoding = true;
        let p = build_preview(&d, &sample, 1, &HashMap::new(), &HashMap::new());
        assert_eq!(p.encoding[0].text, format!("歌手：{garbled} → 周杰伦"));
        d.fix_encoding = false;
        assert!(
            build_preview(&d, &sample, 1, &HashMap::new(), &HashMap::new())
                .encoding
                .is_empty()
        );
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
        assert_eq!(
            build_preview(&d, &sample, 2, &rows, &HashMap::new()),
            Preview::default()
        );

        d.case = Some(CaseOp::Title);
        d.rename_tpl = "{track:02d} - {title}".to_owned();
        let p = build_preview(&d, &sample, 2, &rows, &HashMap::new());
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
        let p = build_preview(&d, &sample, 2, &rows, &HashMap::new());
        assert!(p.rename[0].bad, "{:?}", p.rename);
        assert!(template_error(&d.rename_tpl).is_some());

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
        app.wait_for("text:大小写").unwrap();
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

    /// 「文件」节：导出歌词 / 封面、标签历史各开自己的弹窗；导出弹窗的选项可改、
    /// 另存模式才出现文件夹与文件名模板。
    #[test]
    pub(crate) fn export_and_history_dialogs_open() {
        use nsh_core::geometry::{px, size};
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;
        use std::time::Duration;
        let theme =
            nsh_base::Theme::from_json(include_str!("../../../../theme.json")).expect("theme.json");
        let mut config =
            EngineConfig::new(Root::of::<NarrowBatchPanelTest>().tokens(theme.into_tokens()));
        config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
        config.viewport = size(px(600.0), px(900.0));
        let mut app = Headless::start(config).expect("start");
        app.wait_for("text:乱码修复").unwrap();

        app.press("导出歌词…").unwrap();
        app.wait_for("text:把内嵌歌词写成外部文本文件（UTF-8，无 BOM），不改动标签。")
            .unwrap();
        // 同目录模式没有文件名模板；切到另存才出现。
        assert!(!app.shows("label:导出文件名模板"));
        app.press("label:去除时间戳").unwrap();
        app.press("label:导出位置").unwrap();
        app.press("role:option[另存到文件夹]").unwrap();
        app.wait_for("label:导出文件名模板").unwrap();
        app.press("完成").unwrap();
        app.wait_for_gone("label:导出文件名模板", Duration::from_secs(5))
            .unwrap();

        app.press("导出封面…").unwrap();
        app.wait_for("text:把内嵌封面保存成图片文件，不改动标签。")
            .unwrap();
        app.press("完成").unwrap();

        // 批量面板没有标签历史（只在单选面板里）。
        assert!(!app.shows("标签历史…"));
    }

    #[component]
    pub(crate) fn LyricsRemountTest() -> impl IntoElement {
        zeo_ui_store::install();
        let full = signal(Some((
            HashMap::from([(TagField::Lyrics, "old".to_owned())]),
            Some("FLAC"),
            None,
        )));
        let draft = signal(SingleDraft {
            path: "mock.flac".to_owned(),
            original: HashMap::new(),
            values: HashMap::from([(TagField::Lyrics, "old".to_owned())]),
            cover: None,
            cover_src: None,
            saving: false,
            saved: false,
            error: None,
        });
        let remount = signal(0u32);
        view()
            .child(multi_row(
                "歌词",
                TagField::Lyrics,
                draft,
                full,
                40.0,
                120.0,
                Some(remount),
            ))
            .child(text_button(
                Variant::Outline,
                Size::Sm,
                None,
                "换歌词",
                move || {
                    draft.update(|d| {
                        d.values.insert(TagField::Lyrics, "new".to_owned());
                    });
                    remount.update(|r| *r += 1);
                },
            ))
    }

    /// 导入歌词靠「改草稿 + 递增 remount」让非受控的歌词输入框重新挂载出新文本。
    #[test]
    pub(crate) fn lyrics_input_remounts_with_new_text() {
        use nsh_core::geometry::{px, size};
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;
        let theme =
            nsh_base::Theme::from_json(include_str!("../../../../theme.json")).expect("theme.json");
        let mut config =
            EngineConfig::new(Root::of::<LyricsRemountTest>().tokens(theme.into_tokens()));
        config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
        config.viewport = size(px(500.0), px(400.0));
        let mut app = Headless::start(config).expect("start");
        app.wait_for("label:歌词").unwrap();
        assert_eq!(app.screen().input_text("label:歌词").unwrap(), Some("old"));
        app.press("换歌词").unwrap();
        app.wait_until(
            "歌词输入框换成新文本",
            std::time::Duration::from_secs(5),
            |s| s.input_text("label:歌词") == Ok(Some("new")),
        )
        .unwrap();
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
        single_panel(p, displayed, rows, selected, Rc::new(|| {}), false)
    }

    /// 页面级标签历史弹窗（入口在「显示列」下拉末尾）。
    #[component]
    pub(crate) fn HistoryDialogTest() -> impl IntoElement {
        zeo_ui_store::install();
        let open = signal(true);
        // 和页面（lib.rs）一样用 `when(open)` 挂载：「完成」关掉信号才会卸载弹窗。
        view().child(when(
            move || open.get(),
            move || {
                history_dialog(
                    "mock.flac".to_owned(),
                    signal(HashMap::new()),
                    open,
                    signal(0u32),
                )
            },
        ))
    }

    /// 没有历史记录时弹窗给提示；「完成」收起弹窗。
    #[test]
    pub(crate) fn history_dialog_shows_empty_state_and_closes() {
        use nsh_core::geometry::{px, size};
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;
        let theme =
            nsh_base::Theme::from_json(include_str!("../../../../theme.json")).expect("theme.json");
        let mut config =
            EngineConfig::new(Root::of::<HistoryDialogTest>().tokens(theme.into_tokens()));
        config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
        config.viewport = size(px(600.0), px(600.0));
        let mut app = Headless::start(config).expect("start");
        app.wait_for("text:这首曲目还没有标签历史。保存写回前会自动记下旧值。")
            .unwrap();
        app.press("完成").unwrap();
        app.wait_for_gone(
            "text:这首曲目还没有标签历史。保存写回前会自动记下旧值。",
            std::time::Duration::from_secs(5),
        )
        .unwrap();
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
        // 标签历史入口已挪到「显示列」下拉末尾：单选面板里不再有这颗按钮。
        assert!(!app.shows("标签历史…"));
    }
}
