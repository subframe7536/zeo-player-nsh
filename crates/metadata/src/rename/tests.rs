use super::*;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// 独立临时目录（测试之间互不干扰）。
fn scratch_dir() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "zeo-rename-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn fields(pairs: &[(TagField, &str)]) -> HashMap<TagField, String> {
    pairs.iter().map(|(f, v)| (*f, (*v).to_owned())).collect()
}

/// 目录分隔符是硬规则（先查它，哪怕模板其余部分也合法）。
#[test]
fn template_rejects_separators() {
    for bad in ["a/b", "{artist}/{title}", "a\\b", "{artist}\\{title}"] {
        assert_eq!(
            Template::parse(bad),
            Err(SEPARATOR_ERROR.to_owned()),
            "{bad}"
        );
    }
}

/// 模板语法：空模板、未知占位符、未闭合 / 未配对的大括号。
#[test]
fn template_rejects_bad_syntax() {
    assert_eq!(Template::parse("   "), Err("模板不能为空".to_owned()));
    assert_eq!(
        Template::parse("{artis}"),
        Err("未知占位符 {artis}".to_owned())
    );
    assert_eq!(
        Template::parse("{title"),
        Err("模板里有未闭合的 {".to_owned())
    );
    assert_eq!(
        Template::parse("title}"),
        Err("模板里有未配对的 }".to_owned())
    );
}

/// `{name}` / `{name:N}` / `{name:0N}` / `{name:0Nd}` 与 `{{` `}}`。
#[test]
fn template_renders_fields_and_padding() {
    let t = Template::parse("{track:02d} - {title}").unwrap();
    assert_eq!(
        t.render(
            &fields(&[(TagField::Track, "1"), (TagField::Title, "夜曲")]),
            "x"
        ),
        "01 - 夜曲"
    );
    // 显式 d 与空格补位；非数字值不零补。
    let t = Template::parse("{track:03d},{title:6},{track}").unwrap();
    assert_eq!(
        t.render(
            &fields(&[(TagField::Track, "abc"), (TagField::Title, "歌")]),
            "x"
        ),
        "abc,     歌,abc"
    );
    // 字面大括号。
    let t = Template::parse("{{{title}}}").unwrap();
    assert_eq!(t.render(&fields(&[(TagField::Title, "歌")]), "x"), "{歌}");
}

/// 值里的 `/` `\` 换 `_`（不衍生目录），其余非法字符整串兜底清洗；
/// Windows 保留名加 `_` 前缀。
#[test]
fn template_sanitizes_values() {
    let t = Template::parse("{artist} - {title}").unwrap();
    assert_eq!(
        t.render(
            &fields(&[(TagField::Artist, "AC/DC"), (TagField::Title, "a\\b")]),
            "x"
        ),
        "AC_DC - a_b"
    );
    let t = Template::parse("{title}").unwrap();
    assert_eq!(
        t.render(&fields(&[(TagField::Title, "a<b>:\"c|?*d")]), "x"),
        "a_b___c___d"
    );
    assert_eq!(t.render(&fields(&[(TagField::Title, "CON")]), "x"), "_CON");
    assert_eq!(
        t.render(&fields(&[(TagField::Title, "com1")]), "x"),
        "_com1"
    );
    // 文件名去头尾空白与结尾的点。
    assert_eq!(
        t.render(&fields(&[(TagField::Title, "  name.  ")]), "x"),
        "name"
    );
}

/// `{filename}` 取原主干；渲染出原名 → Unchanged，大小写差异 → Ok。
#[test]
fn plan_unchanged_and_case_only() {
    let dir = scratch_dir();
    let src = dir.join("Song.mp3");
    fs::write(&src, b"x").unwrap();
    let t = Template::parse("{filename}").unwrap();
    let out = plan(&[(src.to_string_lossy().into_owned(), HashMap::new())], &t);
    assert_eq!(out[0].status, Status::Unchanged);

    let t = Template::parse("{title}").unwrap();
    let out = plan(
        &[(
            src.to_string_lossy().into_owned(),
            fields(&[(TagField::Title, "SONG")]),
        )],
        &t,
    );
    assert_eq!(out[0].status, Status::Ok, "仅大小写不同应放行");
    assert!(out[0].dest.ends_with("SONG.mp3"));
}

/// 磁盘撞名与批次内撞名都是 Conflict。
#[test]
fn plan_conflicts() {
    let dir = scratch_dir();
    let taken = dir.join("taken.mp3");
    let other = dir.join("other.mp3");
    fs::write(&taken, b"x").unwrap();
    fs::write(&other, b"x").unwrap();
    let t = Template::parse("{title}").unwrap();

    // 磁盘已有同名文件。
    let out = plan(
        &[(
            other.to_string_lossy().into_owned(),
            fields(&[(TagField::Title, "taken")]),
        )],
        &t,
    );
    assert_eq!(
        out[0].status,
        Status::Conflict("目录里已有同名文件".to_owned())
    );

    // 批次内两条计划渲出同名。
    let a = dir.join("a.mp3");
    let b = dir.join("b.mp3");
    fs::write(&a, b"x").unwrap();
    fs::write(&b, b"x").unwrap();
    let out = plan(
        &[
            (
                a.to_string_lossy().into_owned(),
                fields(&[(TagField::Title, "same")]),
            ),
            (
                b.to_string_lossy().into_owned(),
                fields(&[(TagField::Title, "SAME")]),
            ),
        ],
        &t,
    );
    for p in &out {
        assert_eq!(
            p.status,
            Status::Conflict("与批次内其他文件重名".to_owned()),
            "{p:?}"
        );
    }
}

/// 空渲染结果 → Invalid；侧车目标被占 → Conflict。
#[test]
fn plan_invalid_and_sidecar_conflict() {
    let dir = scratch_dir();
    let src = dir.join("a.mp3");
    fs::write(&src, b"x").unwrap();
    let t = Template::parse("{title}").unwrap();
    let out = plan(&[(src.to_string_lossy().into_owned(), HashMap::new())], &t);
    assert_eq!(out[0].status, Status::Invalid("文件名为空".to_owned()));

    // a.lrc 跟着改成 b.lrc，但 b.lrc 已存在。
    fs::write(dir.join("a.lrc"), b"[00:00]x").unwrap();
    fs::write(dir.join("b.lrc"), b"occupied").unwrap();
    let out = plan(
        &[(
            src.to_string_lossy().into_owned(),
            fields(&[(TagField::Title, "b")]),
        )],
        &t,
    );
    assert_eq!(
        out[0].status,
        Status::Conflict("同名歌词 / 封面文件已存在".to_owned())
    );
}

/// apply：音频 + `.lrc` + `.jpg` 侧车一起换主干，全部留在原目录。
#[test]
fn apply_renames_audio_and_sidecars() {
    let dir = scratch_dir();
    let src = dir.join("old.flac");
    fs::write(&src, b"audio").unwrap();
    fs::write(dir.join("old.lrc"), b"[00:00]x").unwrap();
    fs::write(dir.join("old.jpg"), b"img").unwrap();
    let t = Template::parse("{track:02d} - {title}").unwrap();
    let out = plan(
        &[(
            src.to_string_lossy().into_owned(),
            fields(&[(TagField::Track, "7"), (TagField::Title, "新歌")]),
        )],
        &t,
    );
    assert_eq!(out[0].status, Status::Ok);
    let (moves, error) = apply(&out[0]);
    assert_eq!(error, None);
    let dest = dir.join("07 - 新歌.flac");
    assert_eq!(
        moves,
        vec![
            (
                src.to_string_lossy().into_owned(),
                dest.to_string_lossy().into_owned()
            ),
            (
                dir.join("old.lrc").to_string_lossy().into_owned(),
                dir.join("07 - 新歌.lrc").to_string_lossy().into_owned()
            ),
            (
                dir.join("old.jpg").to_string_lossy().into_owned(),
                dir.join("07 - 新歌.jpg").to_string_lossy().into_owned()
            ),
        ]
    );
    // 目录里只有改完的三个文件，没有新建目录、没有残件。
    let names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        {
            let mut n = names;
            n.sort();
            n
        },
        ["07 - 新歌.flac", "07 - 新歌.jpg", "07 - 新歌.lrc"]
    );
    assert_eq!(fs::read(dir.join("07 - 新歌.flac")).unwrap(), b"audio");
}

/// apply 只跑 Ok：Unchanged 空移动，Conflict / Invalid 带错返回。
#[test]
fn apply_gates_on_status() {
    let unchanged = Planned {
        src: "a.mp3".to_owned(),
        dest: "a.mp3".to_owned(),
        status: Status::Unchanged,
    };
    assert_eq!(apply(&unchanged), (Vec::new(), None));
    let conflict = Planned {
        src: "a.mp3".to_owned(),
        dest: "b.mp3".to_owned(),
        status: Status::Conflict("撞名".to_owned()),
    };
    assert_eq!(apply(&conflict), (Vec::new(), Some("撞名".to_owned())));
}

/// `plan_name`：目录分隔符一律 Invalid（硬规则，先查），dest 保持
/// 源路径不变。
#[test]
fn plan_name_rejects_separators() {
    let dir = scratch_dir();
    let src = dir.join("song.flac");
    fs::write(&src, b"x").unwrap();
    let src = src.to_string_lossy().into_owned();
    for bad in ["a/b", "a\\b", "x / y"] {
        let p = plan_name(&src, bad);
        assert_eq!(
            p.status,
            Status::Invalid(SEPARATOR_ERROR.to_owned()),
            "{bad}"
        );
        assert_eq!(p.dest, src, "{bad}");
    }
}

/// `plan_name`：顺手带上的「.原扩展名」剥掉（不分大小写），剥完
/// 才清洗；剥的不是原扩展名就当普通字符留下。
#[test]
fn plan_name_strips_original_ext() {
    let dir = scratch_dir();
    let src = dir.join("song.flac");
    fs::write(&src, b"x").unwrap();
    let src = src.to_string_lossy().into_owned();
    for typed in ["new name.flac", "new name.FLAC", "new name"] {
        let p = plan_name(&src, typed);
        assert_eq!(p.status, Status::Ok, "{typed}: {p:?}");
        assert!(p.dest.ends_with("new name.flac"), "{typed}: {}", p.dest);
    }
    // `.mp3` 不是原扩展名：留在主干里，扩展名仍是 `.flac`。
    let p = plan_name(&src, "new.mp3");
    assert_eq!(p.status, Status::Ok);
    assert!(p.dest.ends_with("new.mp3.flac"), "{}", p.dest);
    // 只敲了扩展名：剥完剩空名 → Invalid。
    assert_eq!(
        plan_name(&src, ".flac").status,
        Status::Invalid("文件名为空".to_owned())
    );
}

/// `plan_name`：非法字符 / Windows 保留名走 `sanitize` 同款清洗。
#[test]
fn plan_name_sanitizes() {
    let dir = scratch_dir();
    let src = dir.join("song.flac");
    fs::write(&src, b"x").unwrap();
    let src = src.to_string_lossy().into_owned();
    let p = plan_name(&src, "a:b*c");
    assert_eq!(p.status, Status::Ok);
    assert!(p.dest.ends_with("a_b_c.flac"), "{}", p.dest);
    let p = plan_name(&src, "con");
    assert_eq!(p.status, Status::Ok);
    assert!(p.dest.ends_with("_con.flac"), "{}", p.dest);
    // 首尾空白与结尾的点去掉。
    let p = plan_name(&src, "  done.  ");
    assert_eq!(p.status, Status::Ok);
    assert!(p.dest.ends_with("done.flac"), "{}", p.dest);
}

/// `plan_name`：原名 → Unchanged；磁盘撞名 → Conflict；空名 →
/// Invalid。批次内撞名不参与（单条计划）。
#[test]
fn plan_name_statuses() {
    let dir = scratch_dir();
    let src = dir.join("song.flac");
    fs::write(&src, b"x").unwrap();
    let src = src.to_string_lossy().into_owned();
    assert_eq!(plan_name(&src, "song").status, Status::Unchanged);
    // 大小写差异的原地改名 → Ok。
    let p = plan_name(&src, "SONG");
    assert_eq!(p.status, Status::Ok);
    assert!(p.dest.ends_with("SONG.flac"), "{}", p.dest);
    // 磁盘已有同名文件 → Conflict。
    fs::write(dir.join("taken.flac"), b"x").unwrap();
    assert_eq!(
        plan_name(&src, "taken").status,
        Status::Conflict("目录里已有同名文件".to_owned())
    );
    // 同名 `.lrc` 侧车目标被占 → Conflict（与模板计划同规）。
    fs::write(dir.join("song.lrc"), b"[00:00]x").unwrap();
    fs::write(dir.join("b.lrc"), b"occupied").unwrap();
    assert_eq!(
        plan_name(&src, "b").status,
        Status::Conflict("同名歌词 / 封面文件已存在".to_owned())
    );
}
