use super::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
pub(crate) fn corpus(name: &str) -> Option<PathBuf> {
    for dir in ["zeo-player-test-music", "zeo-player-test-formats"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(".nsh/tmp")
            .join(dir)
            .join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// 复制到独立临时目录（语料是共享文件，绝不能改写）。
pub(crate) fn scratch(source: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "zeo-tag-write-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let dest = dir.join(source.file_name().unwrap());
    fs::copy(source, &dest).unwrap();
    dest
}

/// 目录里的 `.zeo-tmp` 文件（测试断言失败路径不留残局）。
pub(crate) fn temp_files(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "zeo-tmp"))
        .collect()
}

/// `Source::open` 解码前 1 秒的帧数（写完的文件必须仍可读）。
pub(crate) fn decoded_frames(path: &Path) -> usize {
    let mut source = crate::codec::Source::open(path)
        .unwrap_or_else(|e| panic!("写完的 {} 解码失败: {e}", path.display()));
    let mut frames = 0;
    while frames < source.sample_rate as usize {
        match source.next_block() {
            Ok(Some(block)) => frames += block.len() / source.channels.max(1),
            Ok(None) => break,
            Err(e) => panic!("写完的 {} 解码出错: {e}", path.display()),
        }
    }
    frames
}

pub(crate) fn edits(pairs: &[(TagField, &str)]) -> TagEdits {
    TagEdits {
        fields: pairs
            .iter()
            .map(|(f, v)| (*f, Edit::Set((*v).to_owned())))
            .collect(),
        cover: None,
        cover_file: None,
        lrc: None,
    }
}
