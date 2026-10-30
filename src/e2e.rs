//! 端到端测试（真实文件、真实解码与输出设备）：
//! 扫描 → 标签/流派拆分 → LRC → 波形 → 播放/暂停/seek/切曲。
//! 依赖 /tmp/zeo-test-music 测试语料（scripts/make-test-music.sh 生成）。

#![cfg(test)]

use std::path::Path;
use std::time::{Duration, Instant};

use crate::engine::{Library, Meta, Player};
use crate::library::library_store;
use crate::lyrics::{parse_lyric, LyricOptions};
use crate::metadata::MetaCache;
use crate::waveform::waveform;

fn corpus() -> Option<std::path::PathBuf> {
    let dir = Path::new("/tmp/zeo-test-music");
    dir.is_dir().then(|| dir.to_path_buf())
}

fn wait_for(timeout_ms: u64, check: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    false
}

#[test]
fn end_to_end_playback() {
    let Some(root) = corpus() else {
        eprintln!("跳过：没有 /tmp/zeo-test-music 测试语料");
        return;
    };
    // SAFETY(测试): 其它测试线程不读写环境变量；此处进程内仅本测试使用库存储。
    unsafe { std::env::set_var("ZEO_DATA_DIR", "/tmp/zeo-test-data-e2e") };
    let _ = std::fs::remove_dir_all("/tmp/zeo-test-data-e2e");

    // 1. 曲库：多根目录 + 扫描 + 流派拆分
    let library = Library::new();
    let roots = library.add_root(root.to_string_lossy().into_owned()).expect("add_root");
    assert_eq!(roots.len(), 1);
    let count = library_store().scan();
    assert_eq!(count, 6, "应扫到 6 个音频文件（mp3/flac/wav 含子目录）");
    let tracks = library.tracks();
    let flac = tracks.iter().find(|t| t.path.ends_with("sine.flac")).expect("sine.flac");
    assert_eq!(flac.title, "测试歌曲");
    assert_eq!(flac.artist, "测试歌手 A; 测试歌手 B");
    assert_eq!(flac.album, "测试专辑");
    assert_eq!(flac.genres, vec!["Rock", "Indie", "Dream Pop"], "genre 按 ; 拆分并 trim");
    assert_eq!(flac.track, Some(1));
    assert!(flac.duration > 1.9 && flac.duration < 2.1, "flac 时长 ≈2s，实际 {}", flac.duration);

    // 2. 元数据缓存
    let meta = MetaCache::global().get(Path::new(&flac.path)).expect("meta");
    assert!(meta.sample_rate == Some(44_100));
    assert_eq!(meta.codec.as_deref(), Some("FLAC"));

    // 3. 外置 LRC 优先 + 解析（元数据头合并、翻译行、空行策略、卡拉OK清理）
    let lrc_path = Meta::external_lrc(flac.path.clone()).expect("同名 .lrc 存在");
    let text = std::fs::read_to_string(lrc_path).unwrap();
    let out = parse_lyric(&text, LyricOptions::default());
    assert!(out.timed);
    // 元数据头(3条同time合并) + 第一句(带翻译) + 长空行(5→12s ≥3s 保留) + 间奏句 + 末句
    assert_eq!(out.lines.len(), 5, "行数：{}，应为 5", out.lines.len());
    assert!(out.lines[0].metadata);
    assert_eq!(out.lines[0].raw_content, "作词：甲\n作曲：乙\n编曲：丙");
    assert_eq!(out.lines[1].raw_content, "第一句歌词");
    assert_eq!(out.lines[1].trans_content.as_deref(), Some("First line"));
    assert_eq!(out.lines[2].raw_content, "");
    assert_eq!(out.lines[3].raw_content, "间奏后的句子");
    assert_eq!(out.lines[4].raw_content, "最后一句 呀", "卡拉 OK 尖括号碎片被清理");

    // 4. 波形：56 根、归一化
    let bars = waveform(&root.join("sine.wav")).expect("waveform");
    assert_eq!(bars.len(), crate::waveform::BARS);
    // 音量恒定的正弦波：波形平、且偏高。2 秒只有约 2000 块、每块约 44 个
    // 样本，不到 440 Hz 的一个周期，块 RMS 随相位起伏；95 分位取到偏高的块，
    // 每根再平均约 40 块，于是稳定在 0.8 多一点而不是 0.9（参考实现同样如此）。
    assert!(bars.iter().all(|b| (0.1..=0.9 + 1e-9).contains(b)), "{bars:?}");
    let body = &bars[..bars.len() - 1]; // 最后一根可能落在不满的块上
    let (lo, hi) = body.iter().fold((1.0_f64, 0.0_f64), |(lo, hi), b| (lo.min(*b), hi.max(*b)));
    assert!(lo > 0.75 && hi - lo < 0.02, "平且高：{lo:.3}..{hi:.3}");

    // 5. 播放通路：mp3 打开 → 进度推进 → 暂停 → seek → 队列切曲 → flac seek
    let mp3 = root.join("quiet.mp3");
    let wav = root.join("quiet.wav");
    let queue = vec![
        mp3.to_string_lossy().into_owned(),
        wav.to_string_lossy().into_owned(),
        flac.path.clone(),
    ];
    let player = Player::new();
    player.open(mp3.to_string_lossy().into_owned(), queue);
    assert!(
        wait_for(3000, || player.state().is_ready && player.state().is_playing),
        "mp3 装载并开始播放: {:?}",
        player.state().error
    );
    assert!(
        wait_for(2500, || player.state().current_time > 0.25),
        "播放进度推进，current_time={}",
        player.state().current_time
    );
    player.pause();
    assert!(wait_for(1000, || !player.state().is_playing), "暂停生效");

    player.seek(1.0);
    assert!(
        wait_for(1500, || (player.state().current_time - 1.0).abs() < 0.35),
        "seek 到 1.0s，实际 {}",
        player.state().current_time
    );

    player.next();
    assert!(
        wait_for(3000, || player.state().path.as_deref() == Some(wav.to_str().unwrap())),
        "下一首切到 wav: {:?}",
        player.state().path
    );
    assert!(player.state().is_playing, "切曲后自动播放");

    player.next(); // 到 flac（44.1k → 设备重采样路径）
    assert!(
        wait_for(3000, || player.state().path.as_deref() == Some(flac.path.as_str())),
        "下一首切到 flac: {:?}",
        player.state().path
    );
    // flac 只有 ~2s：先暂停再操作，不然自然播完按顺序循环回绕到第一首。
    player.pause();
    assert!(wait_for(1000, || !player.state().is_playing), "暂停生效");
    player.seek(1.2);
    assert!(
        wait_for(1500, || (player.state().current_time - 1.2).abs() < 0.4),
        "flac seek，实际 {}",
        player.state().current_time
    );
    player.previous();
    assert!(
        wait_for(3000, || player.state().path.as_deref() == Some(wav.to_str().unwrap())),
        "上一首回到 wav: {:?}",
        player.state().path
    );
    player.stop();

    // 6. 移除根目录后重扫为空
    library.remove_root(roots[0].clone());
    let count = library_store().scan();
    assert_eq!(count, 0);
}
