use super::testutil::*;
use super::*;

/// 非法数字：直接报错，文件字节不变，目录里没有 `.zeo-tmp`。
#[test]
fn tag_write_rejects_bad_numbers_without_touching_file() {
    let Some(source) = corpus("sine.flac") else {
        return eprintln!("跳过：没有测试语料");
    };
    let path = scratch(&source);
    let before = fs::read(&path).unwrap();
    for bad in ["abc", "0", "-1", "3.5"] {
        let err = write_tags(&path, &edits(&[(TagField::Track, bad)]))
            .expect_err(&format!("{bad} 应当被拒绝"));
        assert!(err.contains("正整数"), "{bad}: {err}");
    }
    assert_eq!(fs::read(&path).unwrap(), before, "原文件不能动");
    assert!(temp_files(path.parent().unwrap()).is_empty());
}

/// 封面替换（字节与 MIME 原样写入）与移除。
#[test]
fn tag_write_cover_replace_and_remove() {
    let Some(source) = corpus("sine.flac") else {
        return eprintln!("跳过：没有测试语料");
    };
    let path = scratch(&source);
    // 8×8 红 PNG。
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
        8,
        8,
        image::Rgba([220, 40, 40, 255]),
    ))
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    let png = png.into_inner();

    let replace = TagEdits {
        fields: Vec::new(),
        cover: Some(CoverEdit::Replace {
            data: png.clone(),
            mime: "image/png".to_owned(),
        }),
        cover_file: None,
        lrc: None,
    };
    write_tags(&path, &replace).unwrap();
    assert!(crate::read_meta(&path).unwrap().has_art);
    let (data, mime) = crate::read_artwork(&path).unwrap();
    assert_eq!(data, png, "封面字节应原样存进去");
    assert_eq!(mime.as_deref(), Some("image/png"));

    let remove = TagEdits {
        fields: Vec::new(),
        cover: Some(CoverEdit::Remove),
        cover_file: None,
        lrc: None,
    };
    write_tags(&path, &remove).unwrap();
    assert!(!crate::read_meta(&path).unwrap().has_art);
    assert!(decoded_frames(&path) > 0);
}

/// 同名图片侧车：Replace 新建 / 更新 `<曲名>.<后缀>`（mime 定后缀，
/// bmp 转 PNG），写完删掉其它后缀的旧侧车；Remove 删除文件。音频
/// 本身不动。
#[test]
fn tag_write_cover_sidecar() {
    let Some(source) = corpus("sine.flac") else {
        return eprintln!("跳过：没有测试语料");
    };
    let png8 = |px: [u8; 4]| {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(8, 8, image::Rgba(px)))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        png.into_inner()
    };
    let replace = |data, mime: &str| TagEdits {
        fields: Vec::new(),
        cover: None,
        cover_file: Some(CoverEdit::Replace {
            data,
            mime: mime.to_owned(),
        }),
        lrc: None,
    };

    // 新建 .png，音频字节不动。
    let path = scratch(&source);
    let png_path = path.with_extension("png");
    let before = fs::read(&path).unwrap();
    let png = png8([220, 40, 40, 255]);
    write_tags(&path, &replace(png.clone(), "image/png")).unwrap();
    assert_eq!(fs::read(&png_path).unwrap(), png);
    assert_eq!(fs::read(&path).unwrap(), before, "侧车编辑不能动音频文件");

    // 再写 .jpg：旧 .png 删掉（jpg 读取序在前，留着会遮住新图）。
    let jpg_path = path.with_extension("jpg");
    write_tags(&path, &replace(b"fake-jpeg".to_vec(), "image/jpeg")).unwrap();
    assert_eq!(fs::read(&jpg_path).unwrap(), b"fake-jpeg");
    assert!(!png_path.exists(), "旧后缀的侧车应删掉");

    // bmp 转 PNG 写 .png，.jpg 同样被清掉。
    let mut bmp = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
        8,
        8,
        image::Rgba([40, 40, 220, 255]),
    ))
    .write_to(&mut bmp, image::ImageFormat::Bmp)
    .unwrap();
    write_tags(&path, &replace(bmp.into_inner(), "image/bmp")).unwrap();
    assert!(png_path.exists(), "bmp 应转码写 .png");
    assert!(!jpg_path.exists());
    assert_eq!(
        image::open(&png_path).unwrap().to_rgba8().get_pixel(0, 0).0,
        [40, 40, 220, 255],
        "转码应保留原图内容"
    );

    // Remove 删掉同名图片文件。
    let remove = TagEdits {
        fields: Vec::new(),
        cover: None,
        cover_file: Some(CoverEdit::Remove),
        lrc: None,
    };
    write_tags(&path, &remove).unwrap();
    assert!(!png_path.exists(), "Remove 应删掉同名图片");
    assert!(
        temp_files(path.parent().unwrap()).is_empty(),
        "失败不能留 .zeo-tmp"
    );
}

/// 失败路径：只读文件 → `ensure_writable` 在复制前就报错，原文件
/// 不变、目录里没有 `.zeo-tmp`。
#[cfg(unix)]
#[test]
fn tag_write_leaves_no_temp_on_failure() {
    use std::os::unix::fs::PermissionsExt;
    let Some(source) = corpus("sine.flac") else {
        return eprintln!("跳过：没有测试语料");
    };
    let path = scratch(&source);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    let before = fs::read(&path).unwrap();
    let err = write_tags(&path, &edits(&[(TagField::Title, "新标题")]))
        .expect_err("只读文件应当写不进去");
    assert!(err.contains("不可写"), "{err}");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(fs::read(&path).unwrap(), before, "原文件不能动");
    assert!(
        temp_files(path.parent().unwrap()).is_empty(),
        "失败不能留 .zeo-tmp"
    );
}

/// raw.aac（ADTS 裸流）：没有标签容器，写标签应明确失败而不是悄悄
/// 丢掉；失败也不能动文件。
#[test]
fn tag_write_reports_unsupported_container() {
    let Some(source) = corpus("raw.aac") else {
        return eprintln!("跳过：没有测试语料");
    };
    let path = scratch(&source);
    let before = fs::read(&path).unwrap();
    match write_tags(&path, &edits(&[(TagField::Title, "新标题")])) {
        Ok(()) => {
            // lofty 若能给 ADTS 套上 ID3v2，文件必须仍可解码。
            assert!(decoded_frames(&path) > 0, "写完的 ADTS 解不出声音");
        }
        Err(e) => {
            eprintln!("raw.aac 写标签失败（预期）: {e}");
            assert_eq!(fs::read(&path).unwrap(), before, "失败不能动原文件");
        }
    }
    assert!(temp_files(path.parent().unwrap()).is_empty());
}

/// 元数据视图新增字段：指挥 / 作词 / 发行商 / 版权 / 评分 /
/// ReplayGain 四项写入 → `read_row` 读回（评分、增益、峰值读回
/// 规范形态）；逐个 Clear 后全空。
#[test]
fn tag_write_extended_fields_roundtrip() {
    let extended = [
        TagField::Conductor,
        TagField::Lyricist,
        TagField::Publisher,
        TagField::Copyright,
        TagField::Rating,
        TagField::TrackGain,
        TagField::TrackPeak,
        TagField::AlbumGain,
        TagField::AlbumPeak,
    ];
    let mut covered = 0;
    for (name, tag_type) in [("sine.flac", "Vorbis Comments"), ("quiet.mp3", "ID3v2")] {
        let Some(source) = corpus(name) else {
            eprintln!("跳过 {name}：没有测试语料");
            continue;
        };
        covered += 1;
        let path = scratch(&source);
        let set = edits(&[
            (TagField::Conductor, "指挥甲"),
            (TagField::Lyricist, "作词乙"),
            (TagField::Publisher, "发行商丙"),
            (TagField::Copyright, "版权丁"),
            (TagField::Rating, "4"),
            (TagField::TrackGain, "-6.5"),
            (TagField::TrackPeak, "0.98"),
            (TagField::AlbumGain, "-3.25"),
            (TagField::AlbumPeak, "1.05"),
        ]);
        write_tags(&path, &set).unwrap_or_else(|e| panic!("{name} 写标签失败: {e}"));

        let (fields, read_type) =
            read_row(&path).unwrap_or_else(|e| panic!("{name} 写后读字段失败: {e}"));
        assert_eq!(read_type, Some(tag_type), "{name} 标签类型名");
        assert_eq!(fields[&TagField::Conductor], "指挥甲", "{name}");
        assert_eq!(fields[&TagField::Lyricist], "作词乙", "{name}");
        assert_eq!(fields[&TagField::Publisher], "发行商丙", "{name}");
        assert_eq!(fields[&TagField::Copyright], "版权丁", "{name}");
        assert_eq!(fields[&TagField::Rating], "4", "{name}");
        assert_eq!(fields[&TagField::TrackGain], "-6.50", "{name}");
        assert_eq!(fields[&TagField::TrackPeak], "0.980000", "{name}");
        assert_eq!(fields[&TagField::AlbumGain], "-3.25", "{name}");
        assert_eq!(fields[&TagField::AlbumPeak], "1.050000", "{name}");

        let clear = TagEdits {
            fields: extended.iter().map(|f| (*f, Edit::Clear)).collect(),
            cover: None,
            cover_file: None,
            lrc: None,
        };
        write_tags(&path, &clear).unwrap_or_else(|e| panic!("{name} 清字段失败: {e}"));
        let fields = read_fields(&path).unwrap();
        for field in extended {
            assert!(
                fields[&field].is_empty(),
                "{name} {:?} 清空后应读不回值",
                field
            );
        }
        assert!(decoded_frames(&path) > 0, "{name} 写完解不出声音");
        assert!(
            temp_files(path.parent().unwrap()).is_empty(),
            "{name} 留下 .zeo-tmp"
        );
    }
    assert!(covered > 0, "测试语料缺失，什么都没跑到");
}

/// 评分与 ReplayGain 的校验：非法值在复制文件前就报错，文件不动。
#[test]
fn tag_write_rejects_bad_rating_and_gain() {
    let Some(source) = corpus("sine.flac") else {
        return eprintln!("跳过：没有测试语料");
    };
    let path = scratch(&source);
    let before = fs::read(&path).unwrap();
    for bad in ["0", "6", "x", "3.5"] {
        let err = write_tags(&path, &edits(&[(TagField::Rating, bad)]))
            .expect_err(&format!("评分 {bad} 应当被拒绝"));
        assert!(err.contains("1 到 5"), "{bad}: {err}");
    }
    for bad in ["abc", "100", "-61"] {
        for field in [TagField::TrackGain, TagField::AlbumGain] {
            let err = write_tags(&path, &edits(&[(field, bad)]))
                .expect_err(&format!("增益 {bad} 应当被拒绝"));
            assert!(err.contains("dB"), "{bad}: {err}");
        }
    }
    for bad in ["abc", "0", "-1"] {
        for field in [TagField::TrackPeak, TagField::AlbumPeak] {
            let err = write_tags(&path, &edits(&[(field, bad)]))
                .expect_err(&format!("峰值 {bad} 应当被拒绝"));
            assert!(err.contains("正数"), "{bad}: {err}");
        }
    }
    assert_eq!(fs::read(&path).unwrap(), before, "原文件不能动");
    assert!(temp_files(path.parent().unwrap()).is_empty());
}
