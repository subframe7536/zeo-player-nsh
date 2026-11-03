//! playwire 系统媒体会话：Now Playing / 媒体键。
//! 所有 playwire 调用收拢在专属媒体线程上串行执行（其后端按单线程拥有
//! ObjC 对象的模型设计）；OS 事件经通道回灌引擎命令。

use std::sync::mpsc::Sender;
use std::time::Duration;

use playwire::{Capabilities, Event, MediaControls, PlaybackState, PlayerConfig, Repeat, Track};

use crate::engine::audio::{EngineCmd, Snapshot};

#[derive(Clone)]
pub struct MediaSession {
    tx: Sender<Box<PlaybackState>>,
}

impl MediaSession {
    /// 启动媒体会话与它的串行线程；事件转成引擎命令。
    pub fn start(cmd_tx: Sender<EngineCmd>) -> MediaSession {
        let (tx, rx) = std::sync::mpsc::channel::<Box<PlaybackState>>();
        std::thread::Builder::new()
            .name("zeo-player-media".into())
            .spawn(move || {
                #[allow(unused_mut)]
                let mut config = PlayerConfig::new("Zeo Player")
                    .desktop_entry("dev.subf.zeo-player")
                    .track_id_prefix("/dev/subf/zeo_player/track");
                // Windows 的系统媒体控件要挂在窗口上：等 window.rs 找到窗口。
                #[cfg(target_os = "windows")]
                let mut thumbnail = {
                    let mut waited = 0;
                    let hwnd = loop {
                        if let Some(hwnd) = crate::window::hwnd() {
                            break hwnd;
                        }
                        if waited > 300 {
                            eprintln!("zeo-player: 没找到窗口，系统媒体控件不可用");
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                        waited += 1;
                    };
                    config = config.hwnd(hwnd).app_media_id("dev.subf.zeo-player");
                    smtc_thumbnail::Thumbnail::new(hwnd)
                };
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
                // 后端按 Track 整体 diff（含 artwork_url），重发同曲目的
                // 封面更新会触发新一轮元数据，不需要单独的成功通知。
                while let Ok(state) = rx.recv() {
                    if let Err(err) = controls.set_state(&state) {
                        eprintln!("zeo-player: 媒体状态发布失败: {err}");
                    }
                    #[cfg(target_os = "windows")]
                    if let Some(thumbnail) = thumbnail.as_mut() {
                        // 与 artwork_url 同源：现查缓存，升档 / 冷生成都能跟上。
                        let path = state.track.as_ref().and_then(|t| {
                            crate::metadata::covers::cached_list(std::path::Path::new(&t.id))
                                .map(|(_, f)| f)
                        });
                        thumbnail.publish(path.as_deref());
                    }
                }
            })
            .ok();
        MediaSession { tx }
    }

    /// 发布快照到系统媒体面。
    pub fn publish(&self, snapshot: &Snapshot) {
        let track = &snapshot.track;
        let Some(path) = track.path.as_ref() else {
            return;
        };
        // 循环模式都在端点回绕（单曲是重播自己）：有多于一首就能切。
        let can_switch = track.queue.len() > 1;
        // 封面现查最大的那份列表缓存（升档 96→400 后 96 文件会被删，
        // 冷缓存则晚于换曲才生成）：随每次发布带上，没有就清空缩略图。
        let artwork_url = crate::metadata::covers::cached_list(std::path::Path::new(path))
            .map(|(_, f)| file_url(&f.to_string_lossy()))
            .unwrap_or_default();
        let state = PlaybackState {
            track: Some(Track {
                id: path.clone(),
                title: track.title.clone(),
                artists: vec![track.artist.clone()],
                album: track.album.clone(),
                artwork_url,
                url: String::new(),
            }),
            playing: snapshot.is_playing,
            position: Duration::from_secs_f64(snapshot.current_time.max(0.0)),
            duration: if track.duration > 0.0 {
                Some(Duration::from_secs_f64(track.duration))
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
        let _ = self.tx.send(Box::new(state));
    }
}

/// 本地路径 → `file://` URI：分隔 `/` 与盘符 `:` 保留原样，其余非
/// unreserved 字节按 UTF-8 百分号编码——WinRT `Uri::CreateUri` 与
/// `NSURL` 不接受裸空格 / 非 ASCII（数据目录的用户名段可能含这些）。
fn file_url(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let path = path.replace('\\', "/");
    let mut out = String::with_capacity(path.len() + 8);
    out.push_str(if path.starts_with('/') { "file://" } else { "file:///" });
    for &b in path.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                out.push(b as char)
            }
            _ => {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0x0f) as usize] as char);
            }
        }
    }
    out
}

/// Windows SMTC 缩略图：playwire 2.0 只走
/// `RandomAccessStreamReference::CreateFromUri`，而 WinRT 只收
/// http(s) / ms-appx / ms-appdata，`file://` 进去缩略图恒空——更糟的是
/// 这会让 `apply_metadata` 在 `Update()` 之前抛错，标题 / 艺术家也不提交。
/// 这里用 `GetForWindow(hwnd)` 拿到同一个 SMTC 单例的 DisplayUpdater，
/// 改用 `StorageFile` + `CreateFromFile` 直投本地封面并替它 `Update()`。
#[cfg(target_os = "windows")]
mod smtc_thumbnail {
    use std::path::{Path, PathBuf};

    use windows::Media::{
        SystemMediaTransportControls, SystemMediaTransportControlsDisplayUpdater,
    };
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::RandomAccessStreamReference;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
    use windows::core::{HSTRING, factory};

    pub struct Thumbnail {
        updater: SystemMediaTransportControlsDisplayUpdater,
        /// 上次成功发布的封面文件（None = 无封面）：相同就跳过，免得每次
        /// 进度发布都重开 StorageFile；失败不记，下次发布会重试。
        last: Option<PathBuf>,
    }

    impl Thumbnail {
        pub fn new(hwnd: u64) -> Option<Thumbnail> {
            let interop = factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()
                .map_err(|err| eprintln!("zeo-player: SMTC interop: {err}"))
                .ok()?;
            let controls: SystemMediaTransportControls =
                unsafe { interop.GetForWindow(HWND(hwnd as usize as *mut core::ffi::c_void)) }
                    .map_err(|err| eprintln!("zeo-player: SMTC GetForWindow: {err}"))
                    .ok()?;
            let updater = controls
                .DisplayUpdater()
                .map_err(|err| eprintln!("zeo-player: SMTC DisplayUpdater: {err}"))
                .ok()?;
            Some(Thumbnail { updater, last: None })
        }

        /// `set_state` 之后调：封面文件变了才重投，None 则清空缩略图。
        /// 投新封面顺带替 playwire 提交未落盘的 MusicProperties。
        pub fn publish(&mut self, artwork: Option<&Path>) {
            let new = artwork.map(Path::to_path_buf);
            if new == self.last {
                return;
            }
            let set = match &new {
                Some(path) => StorageFile::GetFileFromPathAsync(&HSTRING::from(
                    &*path.to_string_lossy(),
                ))
                .and_then(|op| op.join())
                .and_then(|file| RandomAccessStreamReference::CreateFromFile(&file))
                .and_then(|stream| self.updater.SetThumbnail(&stream)),
                None => self.updater.SetThumbnail(None),
            };
            match set.and_then(|()| self.updater.Update()) {
                Ok(()) => self.last = new,
                Err(err) => eprintln!("zeo-player: SMTC 封面发布失败: {err}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `file://` URI：盘符与分隔符保留，空格 / 非 ASCII / 特殊字符编码。
    #[test]
    fn file_url_escapes_non_unreserved() {
        assert_eq!(
            file_url("C:\\Users\\John Doe\\covers\\ab-96.jpg"),
            "file:///C:/Users/John%20Doe/covers/ab-96.jpg"
        );
        assert_eq!(
            file_url("C:/Users/张三/covers/ab-96.jpg"),
            "file:///C:/Users/%E5%BC%A0%E4%B8%89/covers/ab-96.jpg"
        );
        assert_eq!(
            file_url("/Users/me/Music/a#b%100.jpg"),
            "file:///Users/me/Music/a%23b%25100.jpg"
        );
    }
}
