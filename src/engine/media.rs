//! playwire 系统媒体会话：Now Playing / 媒体键。
//! 所有 playwire 调用收拢在专属媒体线程上串行执行（其后端按单线程拥有
//! ObjC 对象的模型设计）；OS 事件经通道回灌引擎命令。

use std::sync::mpsc::Sender;
use std::time::Duration;

use playwire::{Capabilities, Event, MediaControls, PlaybackState, PlayerConfig, Repeat, Track};

use crate::engine::audio::{EngineCmd, Snapshot};

/// 指示媒体线程操作的内部消息。
enum Op {
    Publish(Box<PlaybackState>),
    Artwork(Option<String>),
}

#[derive(Clone)]
pub struct MediaSession {
    tx: Sender<Op>,
}

impl MediaSession {
    /// 启动媒体会话与它的串行线程；事件转成引擎命令。
    pub fn start(cmd_tx: Sender<EngineCmd>) -> MediaSession {
        let (tx, rx) = std::sync::mpsc::channel::<Op>();
        std::thread::Builder::new()
            .name("zeo-player-media".into())
            .spawn(move || {
                #[allow(unused_mut)]
                let mut config = PlayerConfig::new("Zeo Player")
                    .desktop_entry("dev.subf.zeo-player")
                    .track_id_prefix("/dev/subf/zeo_player/track");
                // Windows 的系统媒体控件要挂在窗口上：等 window.rs 找到窗口。
                #[cfg(target_os = "windows")]
                {
                    let mut waited = 0;
                    loop {
                        if let Some(hwnd) = crate::window::hwnd() {
                            config = config.hwnd(hwnd).app_media_id("dev.subf.zeo-player");
                            break;
                        }
                        if waited > 300 {
                            eprintln!("zeo-player: 没找到窗口，系统媒体控件不可用");
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                        waited += 1;
                    }
                }
                let mut controls = match MediaControls::new(config, move |event: Event| {
                    // OS 回调线程：只做转发，不阻塞。
                    let cmd = match event {
                        Event::Play => EngineCmd::Play,
                        Event::Pause => EngineCmd::Pause,
                        Event::PlayPause => EngineCmd::Toggle,
                        Event::Stop => EngineCmd::Stop,
                        Event::Next => EngineCmd::Next,
                        Event::Previous => EngineCmd::Previous,
                        Event::SeekTo(pos) => EngineCmd::Seek(pos.as_secs_f64()),
                        Event::SeekBy(delta) => EngineCmd::SeekBy(delta),
                        Event::SetVolume(v) => EngineCmd::SetVolume(v),
                        _ => return,
                    };
                    let _ = cmd_tx.send(cmd);
                }) {
                    Ok(controls) => controls,
                    Err(err) => {
                        eprintln!("zeo-player: 系统媒体会话不可用: {err}");
                        return;
                    }
                };
                let mut artwork_file: Option<String> = None;
                let mut last_track_id: Option<String> = None;
                while let Ok(op) = rx.recv() {
                    match op {
                        Op::Artwork(url) => artwork_file = url,
                        Op::Publish(state) => {
                            let mut state = *state;
                            // 换曲时带上封面（后端也会 diff，重复发布无害）。
                            let changed = state
                                .track
                                .as_ref()
                                .map(|track| last_track_id.as_deref() != Some(track.id.as_str()))
                                .unwrap_or(false);
                            if changed {
                                let id = state.track.as_ref().unwrap().id.clone();
                                if let Some(track) = state.track.as_mut() {
                                    track.artwork_url = artwork_file.clone().unwrap_or_default();
                                }
                                last_track_id = Some(id);
                            }
                            if let Err(err) = controls.set_state(&state) {
                                eprintln!("zeo-player: 媒体状态发布失败: {err}");
                            }
                        }
                    }
                }
            })
            .ok();
        MediaSession { tx }
    }

    /// 更新媒体会话封面文件（file://）。
    pub fn set_artwork_file(&self, url: Option<String>) {
        let _ = self.tx.send(Op::Artwork(url));
    }

    /// 发布快照到系统媒体面。
    pub fn publish(&self, snapshot: &Snapshot) {
        let Some(path) = snapshot.path.as_ref() else {
            return;
        };
        // 循环模式都在端点回绕（单曲是重播自己）：有多于一首就能切。
        let can_switch = snapshot.queue.len() > 1;
        let state = PlaybackState {
            track: Some(Track {
                id: path.clone(),
                title: snapshot.title.clone(),
                artists: vec![snapshot.artist.clone()],
                album: snapshot.album.clone(),
                artwork_url: String::new(),
                url: String::new(),
            }),
            playing: snapshot.is_playing,
            position: Duration::from_secs_f64(snapshot.current_time.max(0.0)),
            duration: if snapshot.duration > 0.0 {
                Some(Duration::from_secs_f64(snapshot.duration))
            } else {
                None
            },
            volume: snapshot.volume,
            repeat: match snapshot.loop_mode {
                crate::engine::audio::LoopMode::Single => Repeat::One,
                _ => Repeat::All,
            },
            shuffle: snapshot.loop_mode == crate::engine::audio::LoopMode::Random,
            capabilities: Capabilities {
                can_go_next: can_switch,
                can_go_previous: can_switch,
                can_seek: true,
            },
        };
        let _ = self.tx.send(Op::Publish(Box::new(state)));
    }
}
