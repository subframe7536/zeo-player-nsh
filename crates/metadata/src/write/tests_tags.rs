use super::testutil::*;
use super::*;
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;

/// 八个容器往返：全字段写入 → `read_meta` / `read_fields` 读回一致；
/// 写完的文件 `Source::open` 仍能解码。任一容器失败立即报出容器名。
#[test]
fn tag_write_roundtrip() {
    for name in [
        "sine.flac",
        "quiet.mp3",
        "sine.wav",
        "aac.m4a",
        "alac.m4a",
        "vorbis.ogg",
        "pcm.aiff",
    ] {
        let Some(source) = corpus(name) else {
            eprintln!("跳过 {name}：没有测试语料");
            continue;
        };
        let path = scratch(&source);
        let set = edits(&[
            (TagField::Title, "新标题"),
            (TagField::Artist, "歌手甲"),
            (TagField::Album, "专辑乙"),
            (TagField::AlbumArtist, "专辑歌手丙"),
            (TagField::Genre, "Rock"),
            (TagField::Year, "2024"),
            (TagField::Track, "3"),
            (TagField::TrackTotal, "12"),
            (TagField::Disc, "1"),
            (TagField::DiscTotal, "2"),
            (TagField::Composer, "作曲丁"),
            (TagField::Comment, "注释戊"),
        ]);
        write_tags(&path, &set).unwrap_or_else(|e| panic!("{name} 写标签失败: {e}"));

        let meta =
            crate::read_meta(&path).unwrap_or_else(|e| panic!("{name} 写后读元数据失败: {e}"));
        assert_eq!(meta.title, "新标题", "{name}");
        assert_eq!(meta.artist, "歌手甲", "{name}");
        assert_eq!(meta.album, "专辑乙", "{name}");
        assert_eq!(meta.album_artist, "专辑歌手丙", "{name}");
        assert_eq!(meta.genres, ["Rock"], "{name}");
        assert_eq!(meta.year, Some(2024), "{name}");
        assert_eq!(meta.track, Some(3), "{name}");
        assert_eq!(meta.track_total, Some(12), "{name}");
        assert_eq!(meta.disk, Some(1), "{name}");
        assert_eq!(meta.disk_total, Some(2), "{name}");
        assert_eq!(meta.composers, ["作曲丁"], "{name}");

        let fields = read_fields(&path).unwrap_or_else(|e| panic!("{name} 写后读字段失败: {e}"));
        assert_eq!(fields[&TagField::Comment], "注释戊", "{name}");

        assert!(decoded_frames(&path) > 0, "{name} 写完解不出声音");
        assert!(
            temp_files(path.parent().unwrap()).is_empty(),
            "{name} 留下 .zeo-tmp"
        );
    }
}

/// Set 写进字段后 Clear 删掉（`Set("")` 与 `Clear` 等价）。
#[test]
fn tag_write_clear_removes_key() {
    let Some(source) = corpus("sine.flac") else {
        return eprintln!("跳过：没有测试语料");
    };
    let path = scratch(&source);
    let set = edits(&[(TagField::Album, "临时专辑")]);
    write_tags(&path, &set).unwrap();
    assert_eq!(crate::read_meta(&path).unwrap().album, "临时专辑");
    let clear = TagEdits {
        fields: vec![(TagField::Album, Edit::Clear)],
        cover: None,
        cover_file: None,
        lrc: None,
    };
    write_tags(&path, &clear).unwrap();
    assert_eq!(crate::read_meta(&path).unwrap().album, "");
    let blank = edits(&[(TagField::Title, "")]);
    write_tags(&path, &blank).unwrap();
    // 空串 = Clear：标题不再回退到文件名，保持空串。
    assert_eq!(crate::read_meta(&path).unwrap().title, "");
}

/// TRACKNUMBER="3/12" 合并写法：动号 / 动总数 / 清号 / 清总数都
/// 不能把另一边弄丢或留下读得回来的旧值。
#[test]
fn tag_write_pair_format_total_survives() {
    let Some(source) = corpus("sine.flac") else {
        return eprintln!("跳过：没有测试语料");
    };
    // 造一个 TRACKNUMBER="3/12" 的文件（合并写法）。
    let pair_file = |path: &Path| {
        let mut tagged = Probe::open(path)
            .unwrap()
            .guess_file_type()
            .unwrap()
            .read()
            .unwrap();
        let tag = tagged.primary_tag_mut().unwrap();
        tag.insert_text(ItemKey::TrackNumber, "3/12".to_owned());
        tag.remove_key(ItemKey::TrackTotal);
        tagged.save_to_path(path, WriteOptions::default()).unwrap();
    };
    let meta = |path: &Path| crate::read_meta(path).unwrap();

    let path = scratch(&source);
    pair_file(&path);
    let clear_track = TagEdits {
        fields: vec![(TagField::Track, Edit::Clear)],
        cover: None,
        cover_file: None,
        lrc: None,
    };
    write_tags(&path, &clear_track).unwrap();
    // 清了号，总数还在（拆到独立键）。
    assert_eq!(
        (meta(&path).track, meta(&path).track_total),
        (None, Some(12))
    );

    let path = scratch(&source);
    pair_file(&path);
    write_tags(&path, &edits(&[(TagField::Track, "5")])).unwrap();
    assert_eq!(
        (meta(&path).track, meta(&path).track_total),
        (Some(5), Some(12))
    );

    let path = scratch(&source);
    pair_file(&path);
    write_tags(&path, &edits(&[(TagField::TrackTotal, "15")])).unwrap();
    assert_eq!(
        (meta(&path).track, meta(&path).track_total),
        (Some(3), Some(15))
    );

    let path = scratch(&source);
    pair_file(&path);
    let clear_total = TagEdits {
        fields: vec![(TagField::TrackTotal, Edit::Clear)],
        cover: None,
        cover_file: None,
        lrc: None,
    };
    write_tags(&path, &clear_total).unwrap();
    assert_eq!(
        (meta(&path).track, meta(&path).track_total),
        (Some(3), None)
    );
}

/// 歌词：写入 → `read_lyrics` 读回；清空后 `read_lyrics` 为 `None`。
/// LYRICS / UNSYNCEDLYRICS 两个键都不许留旧值。
#[test]
fn tag_write_lyrics_roundtrip() {
    for name in ["sine.flac", "quiet.mp3", "vorbis.ogg", "aac.m4a"] {
        let Some(source) = corpus(name) else {
            eprintln!("跳过 {name}：没有测试语料");
            continue;
        };
        let path = scratch(&source);
        let text = "[00:01.00]第一行\n[00:02.00]第二行";
        let set = TagEdits {
            fields: vec![(TagField::Lyrics, Edit::Set(text.to_owned()))],
            cover: None,
            cover_file: None,
            lrc: None,
        };
        match write_tags(&path, &set) {
            Ok(()) => assert_eq!(
                crate::read_lyrics(&path).as_deref(),
                Some(text),
                "{name} 歌词应原样读回"
            ),
            Err(e) => {
                eprintln!("{name} 不支持写歌词键: {e}");
                continue;
            }
        }
        let clear = TagEdits {
            fields: vec![(TagField::Lyrics, Edit::Clear)],
            cover: None,
            cover_file: None,
            lrc: None,
        };
        write_tags(&path, &clear).unwrap();
        assert_eq!(crate::read_lyrics(&path), None, "{name}");
        assert!(decoded_frames(&path) > 0, "{name} 写完解不出声音");
    }
}

/// `.lrc` 侧车：Set 新建 / 更新同名歌词文件（不碰音频），Clear 删除
/// 文件；失败不留 `.zeo-tmp`。大写 `.LRC` 已有时更新的是它。
#[test]
fn tag_write_lrc_sidecar() {
    let Some(source) = corpus("sine.flac") else {
        return eprintln!("跳过：没有测试语料");
    };
    let path = scratch(&source);
    let lrc_path = path.with_extension("lrc");
    let before = fs::read(&path).unwrap();
    let set = TagEdits {
        fields: Vec::new(),
        cover: None,
        cover_file: None,
        lrc: Some(Edit::Set("[00:01.00]侧车歌词".to_owned())),
    };
    write_tags(&path, &set).unwrap();
    assert_eq!(fs::read_to_string(&lrc_path).unwrap(), "[00:01.00]侧车歌词");
    assert_eq!(fs::read(&path).unwrap(), before, "侧车编辑不能动音频文件");
    assert!(decoded_frames(&path) > 0);

    let clear = TagEdits {
        fields: Vec::new(),
        cover: None,
        cover_file: None,
        lrc: Some(Edit::Clear),
    };
    write_tags(&path, &clear).unwrap();
    assert!(!lrc_path.exists(), "Clear 应删掉 .lrc");
    assert!(
        temp_files(path.parent().unwrap()).is_empty(),
        "失败不能留 .zeo-tmp"
    );
}
