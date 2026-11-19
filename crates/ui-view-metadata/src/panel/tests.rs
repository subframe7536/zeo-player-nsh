use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// 三态展开；全空跳过。
    #[test]
    pub(crate) fn batch_edits_builds_per_file() {
        let paths: Vec<String> = (1..=3).map(|i| format!("/m/{i}.mp3")).collect();
        let mut modes = HashMap::new();
        modes.insert(TagField::Artist, Tri::Set);
        modes.insert(TagField::Album, Tri::Clear);
        let mut values = HashMap::new();
        values.insert(TagField::Artist, "歌手".to_owned());
        let items = batch_edits(&paths, &modes, &values, false, &CoverMode::Keep, |_, _| {
            String::new()
        });
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
        // 全 Keep 的跳过。
        let empty = batch_edits(
            &paths,
            &HashMap::new(),
            &HashMap::new(),
            false,
            &CoverMode::Keep,
            |_, _| String::new(),
        );
        assert!(empty.is_empty());
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

    /// 乱码修复：Latin-1 乱码按识别出的编码还原，三态已定值的字段不动；
    /// 预览显示 `乱码 → 修复后`。
    #[test]
    pub(crate) fn encoding_fix_respects_tri_state_and_previews() {
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
            batch_edits(&path, modes, values, true, &CoverMode::Keep, field_of)
        };
        let items = run(&HashMap::new(), &HashMap::new());
        assert_eq!(
            get(&items, TagField::Artist),
            Some(Edit::Set("周杰伦".to_owned()))
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

        let lines = encoding_preview(&sample, &HashMap::new(), &HashMap::new());
        assert_eq!(lines[0].text, format!("歌手：{garbled} → 周杰伦"));
    }

    /// 重命名预览：模板按当前标签值渲染；坏模板标红；空模板无预览。
    #[test]
    pub(crate) fn rename_preview_renders_and_flags_bad_templates() {
        let dir = std::env::temp_dir().join(format!("zeo-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = |name: &str| dir.join(name).to_string_lossy().replace('\\', "/");
        let sample = vec![
            entry(&path("a.flac"), "My Song", "me", 1),
            entry(&path("b.flac"), "Other", "Me", 2),
        ];
        let rows = HashMap::new();
        assert!(rename_preview("", &sample, &rows).is_empty());

        let p = rename_preview("{track:02d} - {title}", &sample, &rows);
        assert!(p[0].text.ends_with("01 - My Song.flac"), "{:?}", p);
        assert!(p[1].text.ends_with("02 - Other.flac"), "{:?}", p);
        assert!(p.iter().all(|l| !l.bad));

        let p = rename_preview("{nope}", &sample, &rows);
        assert!(p[0].bad, "{:?}", p);
        assert!(template_error("{nope}").is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 面板最窄宽度（`PANEL_MIN_W`）下的批量面板。
    #[component]
    pub(crate) fn NarrowBatchPanelTest() -> impl IntoElement {
        zeo_ui_store::install();
        let paths = Rc::new(vec!["a.flac".to_owned(), "b.flac".to_owned()]);
        let displayed = computed(|| Rc::new(Vec::<TrackEntry>::new()));
        let rows = signal(HashMap::new());
        view()
            .w(px(240.0))
            .h(px(800.0))
            .test_id("host")
            .child(batch_panel(paths, displayed, rows, Rc::new(|| {})))
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
        app.wait_for("text:字段").unwrap();
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

    /// 工具栏批量工具测试组件：两颗入口钮 + 各自的页面级弹窗（与页面同款
    /// `when` 挂载），选中集固定两首。
    #[component]
    pub(crate) fn ToolDialogsTest() -> impl IntoElement {
        zeo_ui_store::install();
        let shown = vec![entry("a.flac", "t0", "a", 1), entry("b.flac", "t1", "a", 2)];
        let displayed = computed(move || Rc::new(shown.clone()));
        let rows = signal(HashMap::new());
        let selected = signal(BTreeSet::from(["a.flac".to_owned(), "b.flac".to_owned()]));
        let encoding_open = signal(false);
        let rename_open = signal(false);
        let can = computed(move || !selected.get().is_empty());
        view()
            .flex_row()
            .child(encoding_button(encoding_open, can))
            .child(rename_button(rename_open, can))
            .child(when(
                move || encoding_open.get() && can.get(),
                move || {
                    let (order, entries, sample) = tool_context(displayed, selected);
                    encoding_dialog(order, entries, sample, rows, encoding_open)
                },
            ))
            .child(when(
                move || rename_open.get() && can.get(),
                move || {
                    let (order, entries, sample) = tool_context(displayed, selected);
                    rename_dialog(order, entries, sample, rows, selected, rename_open)
                },
            ))
    }

    /// 工具栏「批量重命名」点开模板弹窗：有占位符速查；点预设整个替换模板，
    /// 点占位符追加；空模板时「重命名 N 首」不可按；右上角 ✕ 收弹窗。
    /// 「乱码修复」点开预览弹窗，「取消」收掉。
    #[test]
    pub(crate) fn tool_buttons_open_rename_and_encoding_dialogs() {
        use nsh_core::geometry::{px, size};
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;
        use std::time::Duration;
        let theme =
            nsh_base::Theme::from_json(include_str!("../../../../theme.json")).expect("theme.json");
        let mut config =
            EngineConfig::new(Root::of::<ToolDialogsTest>().tokens(theme.into_tokens()));
        config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
        config.viewport = size(px(600.0), px(800.0));
        let mut app = Headless::start(config).expect("start");
        app.wait_for("label:批量重命名").unwrap();
        app.press("label:批量重命名").unwrap();
        app.wait_for("text:{filename}").unwrap();
        // 空模板：应用钮不可按。
        assert_eq!(app.screen().enabled("label:重命名 2 首"), Ok(false));
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
        assert_eq!(app.screen().enabled("label:重命名 2 首"), Ok(true));
        app.press("label:关闭重命名模板").unwrap();
        app.wait_for_gone("label:模板", Duration::from_secs(5))
            .unwrap();

        app.press("label:乱码修复").unwrap();
        app.wait_for("text:乱码修复").unwrap();
        app.wait_for("text:修复 2 首").unwrap();
        app.press("取消").unwrap();
        app.wait_for_gone("text:修复 2 首", Duration::from_secs(5))
            .unwrap();
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
        app.wait_for("text:字段").unwrap();

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

        // 批量面板没有标签历史（入口在页面工具栏）。
        assert!(!app.shows("标签历史…"));
    }

    /// `ApplyButtonTest` 的所选路径（真实文件副本，`paths` 要在组件挂载前填好）。
    static APPLY_PATHS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    #[component]
    pub(crate) fn ApplyButtonTest() -> impl IntoElement {
        zeo_ui_store::install();
        let paths = Rc::new(APPLY_PATHS.lock().unwrap().clone());
        let shown: Vec<TrackEntry> = paths
            .iter()
            .enumerate()
            .map(|(i, p)| entry(p, &format!("t{i}"), "a", i as u32 + 1))
            .collect();
        let displayed = computed(move || Rc::new(shown.clone()));
        let rows = signal(HashMap::new());
        view()
            .w(px(480.0))
            .h(px(800.0))
            .child(batch_panel(paths, displayed, rows, Rc::new(|| {})))
    }

    /// 「应用到 N 首」：草稿没动时不可按；有改动时可按，应用成功后文字换
    /// ✓（无障碍名「应用成功」），三秒还原；下方不再出「已保存」类消息行。
    #[test]
    pub(crate) fn apply_button_gating_and_success_check() {
        use nsh_core::geometry::{px, size};
        use nsh_engine::{EngineConfig, Root};
        use nsh_testing::Headless;
        use std::time::Duration;

        let corpus = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.nsh/tmp/zeo-player-test-music/sine.flac"
        );
        let src = std::path::Path::new(corpus);
        if !src.is_file() {
            return eprintln!("跳过：没有 {corpus}");
        }
        let dir = std::env::temp_dir().join(format!("zeo-apply-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut paths = Vec::new();
        for name in ["x1.flac", "x2.flac"] {
            let p = dir.join(name);
            std::fs::copy(src, &p).unwrap();
            paths.push(p.to_string_lossy().into_owned());
        }
        *APPLY_PATHS.lock().unwrap() = paths;

        let theme =
            nsh_base::Theme::from_json(include_str!("../../../../theme.json")).expect("theme.json");
        let mut config =
            EngineConfig::new(Root::of::<ApplyButtonTest>().tokens(theme.into_tokens()));
        config.ui_sets = vec![nsh_base::registry(), zeo_player::registry()];
        config.viewport = size(px(500.0), px(900.0));
        let mut app = Headless::start(config).expect("start");
        app.wait_for("text:批量修改").unwrap();
        // 草稿里没有任何改动：不可按。
        assert_eq!(app.screen().enabled("label:应用批量修改"), Ok(false));
        // 封面选「移除」（一处改动）：可按。
        app.press("text:移除").unwrap();
        assert_eq!(app.screen().enabled("label:应用批量修改"), Ok(true));
        app.press("label:应用批量修改").unwrap();
        // 成功：按钮文字换 ✓，消息行不出现。
        app.wait_for("label:应用成功").unwrap();
        assert!(!app.shows("标签没有需要修改的内容"));
        // 三秒后还原。
        app.wait_for_gone("label:应用成功", Duration::from_secs(10))
            .unwrap();
        app.wait_for("label:应用批量修改").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
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

    /// 页面级标签历史弹窗（入口在工具栏图标钮）。
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
        // 标签历史入口在页面工具栏：单选面板里没有这颗按钮。
        assert!(!app.shows("标签历史…"));
    }
}
