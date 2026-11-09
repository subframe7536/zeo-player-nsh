use super::*;

pub use zeo_ui_store::FileOp;

/// 新建 / 重命名的目标名：非空、不含路径分隔符、不是 `.` / `..`。
pub(crate) fn valid_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("名字不能为空".into());
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err("名字里不能含 / 或 \\".into());
    }
    Ok(name)
}

/// `path` 的父目录（去掉尾部分隔符再取前缀）；根路径没有父目录。
pub(crate) fn parent_dir(path: &str, sep: char) -> Option<String> {
    let trimmed = path.trim_end_matches(sep);
    trimmed.rfind(sep).map(|i| {
        let parent = &trimmed[..i];
        if parent.is_empty() {
            // `/a` 的父目录是 `/`。
            sep.to_string()
        } else if parent.len() == 2 && parent.ends_with(':') {
            // `C:\a` 的父目录是 `C:\`。
            format!("{parent}{sep}")
        } else {
            parent.to_owned()
        }
    })
}

/// 重命名输入框的初值：文件夹是整个名字，文件只给主名（扩展名保留，
/// 不进输入框）。
pub(crate) fn rename_default(path: &str, dir: bool) -> String {
    let file = last_segment(path);
    if dir {
        return file.to_owned();
    }
    Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_owned())
}

/// 重命名的目标路径：文件夹整个名字换掉；文件只改主名、扩展名沿用
/// （输入已带相同扩展名时不重复拼）。
pub(crate) fn rename_dest(path: &str, dir: bool, name: &str, sep: char) -> Result<String, String> {
    let name = valid_name(name)?;
    let Some(parent) = parent_dir(path, sep) else {
        return Err("磁盘根目录不能重命名".into());
    };
    let file_name = if dir {
        name.to_owned()
    } else {
        let ext = Path::new(path.trim_end_matches(sep))
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        if ext.is_empty()
            || name
                .to_lowercase()
                .ends_with(&format!(".{}", ext.to_lowercase()))
        {
            name.to_owned()
        } else {
            format!("{name}.{ext}")
        }
    };
    Ok(format!("{}{sep}{file_name}", parent.trim_end_matches(sep)))
}

/// `src` 移 / 改名到 `dest`：曲库根、曲目索引与标签缓存 / 数据库
/// （`move_path`）、手动歌单与会话里的空文件夹记录一起换前缀（等于
/// `src` 或以 `src/` 为前缀的都算）——内容没变，不重读标签也不重扫。
pub(crate) fn move_refs(src: &str, dest: &str, extra: &Signal<BTreeSet<String>>) {
    let (src, dest) = (
        src.trim_end_matches(SEP).to_owned(),
        dest.trim_end_matches(SEP).to_owned(),
    );
    let under = format!("{src}{SEP}");
    library_store().move_path(&src, &dest);
    store::move_playlist_paths(&src, &dest);
    extra.update(|e| {
        let moved: Vec<String> = e
            .iter()
            .filter(|d| *d == &src || d.starts_with(&under))
            .cloned()
            .collect();
        for d in moved {
            e.remove(&d);
            e.insert(format!("{dest}{}", &d[src.len()..]));
        }
    });
}

/// `path` 下（含自身）的曲库根摘掉、索引与元数据记录清掉、歌单条目
/// 移除（文件已没，留着是死条目）。
pub(crate) fn remove_refs_under(path: &str) {
    let trimmed = path.trim_end_matches(SEP);
    let under = format!("{trimmed}{SEP}");
    let lib = library_store();
    for root in lib.roots() {
        let r = root.trim_end_matches(SEP);
        if r == trimmed || r.starts_with(&under) {
            lib.remove_root(&root);
        }
    }
    store::remove_playlist_paths_under(trimmed);
    lib.remove_under(trimmed);
}

/// 正在播的曲目在 `path` 里（含自身）就先停掉：文件马上要被移走或删
/// 掉，Windows 上解码器还握着文件会移不动。后台线程跑（`store()` 只
/// 在组件上下文里），用 `player()` 的快照看当前曲。
pub fn stop_if_playing_under(path: &str) {
    let trimmed = path.trim_end_matches(SEP);
    let under = format!("{trimmed}{SEP}");
    if player()
        .state()
        .track
        .path
        .as_deref()
        .is_some_and(|p| p == trimmed || p.starts_with(&under))
    {
        player().stop();
    }
}

/// 增删改的执行者与弹窗状态：文件操作在后台线程做（正在播放的文件在
/// Windows 上可能被解码器握着，先停掉再多试几次），完成后回来修曲库
/// 引用、重扫并定位。
pub(crate) struct Ops {
    /// `Some` 时对应弹窗挂着。
    pub(crate) op: Signal<Option<FileOp>>,
    /// 本次会话建出来还没曲目的空文件夹：靠它挂进树（树由曲目路径推
    /// 导，磁盘上已没了的在构建时滤掉）。
    pub(crate) extra: Signal<BTreeSet<String>>,
    /// 定位用：选中的文件夹。
    pub(crate) cwd: Signal<Option<String>>,
    /// 折叠集合（定位时把目标从里面摘掉让它展开）。
    pub(crate) expanded: Signal<BTreeSet<String>>,
    /// 结果提示（页头副标题短暂显示）。
    pub(crate) message: Signal<String>,
}

/// 操作完成后定位到某个文件夹：选中并保证它与祖先展开。
pub(crate) fn select_folder(
    cwd: Signal<Option<String>>,
    expanded: Signal<BTreeSet<String>>,
    path: &str,
) {
    cwd.set(Some(path.to_owned()));
    expanded.update(|e| {
        e.insert(path.to_owned());
        expand_ancestors(e, path, SEP);
    });
}

impl Ops {
    pub(crate) fn close(&self) {
        self.op.set(None);
    }

    pub(crate) fn select(&self, path: &str) {
        select_folder(self.cwd, self.expanded, path);
    }

    /// 「新建文件夹」/「重命名」的确定：校验名字并算出目标路径（不合
    /// 法 / 已存在时把错误文案留在弹窗里），合法就关掉弹窗开工。
    pub(crate) fn submit_name(&self, name: &str) -> Result<(), String> {
        let Some(op) = self.op.get_untracked() else {
            return Ok(());
        };
        match op {
            FileOp::NewFolder { parent } => {
                let name = valid_name(name)?.to_owned();
                let path = format!("{}{SEP}{name}", parent.trim_end_matches(SEP));
                if Path::new(&path).exists() {
                    return Err("已存在同名的文件或文件夹".into());
                }
                std::fs::create_dir(&path).map_err(|e| format!("创建失败：{e}"))?;
                self.extra.update(|e| {
                    e.insert(path.clone());
                });
                self.close();
                self.select(&path);
                show_message(self.message, format!("已创建文件夹 {name}"));
                Ok(())
            }
            FileOp::Rename { path, dir } => {
                let dest = rename_dest(&path, dir, name, SEP)?;
                if dest == path.trim_end_matches(SEP) {
                    // 名字没变：直接关，什么都不做。
                    self.close();
                    return Ok(());
                }
                if Path::new(&dest).exists() {
                    return Err("已存在同名的文件或文件夹".into());
                }
                self.close();
                self.rename(path, dest, dir);
                Ok(())
            }
            FileOp::DeleteFolder { .. } => Ok(()),
        }
    }

    /// 后台改名：文件可能被播放器握着（Windows），停掉后多试几次；成
    /// 功再修曲库引用、重扫并定位。
    pub(crate) fn rename(&self, src: String, dest: String, dir: bool) {
        let (cwd, expanded, message, extra) = (self.cwd, self.expanded, self.message, self.extra);
        let ui = store::poster();
        std::thread::spawn(move || {
            stop_if_playing_under(&src);
            if !Path::new(&src).exists() {
                return ui.post(move || {
                    show_message(message, "原路径已不存在".into());
                });
            }
            let mut done = false;
            for _ in 0..10 {
                if std::fs::rename(&src, &dest).is_ok() {
                    done = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            let name = last_segment(&dest).to_owned();
            let target = if dir {
                dest.clone()
            } else {
                parent_dir(&dest, SEP).unwrap_or_else(|| dest.clone())
            };
            ui.post(move || {
                if done {
                    move_refs(&src, &dest, &extra);
                    select_folder(cwd, expanded, &target);
                    show_message(message, format!("已重命名为 {name}"));
                } else {
                    show_message(message, format!("重命名 {name} 失败"));
                }
            });
        });
    }

    /// 「删除文件夹」确认：后台删（回收站 / 直接删按设置），成功后清
    /// 掉其中的曲库根、曲目记录与歌单条目，重扫并定位到父目录。
    pub(crate) fn run_delete(&self, path: String) {
        self.close();
        let permanent = library_store().delete_permanently();
        let (cwd, expanded, message, extra) = (self.cwd, self.expanded, self.message, self.extra);
        let ui = store::poster();
        std::thread::spawn(move || {
            stop_if_playing_under(&path);
            let dir = Path::new(&path);
            let mut gone = !dir.exists();
            for _ in 0..10 {
                if gone {
                    break;
                }
                if zeo_library::provider::local::delete_dir(dir, permanent).is_ok() {
                    gone = true;
                } else {
                    std::thread::sleep(Duration::from_millis(150));
                }
            }
            let name = last_segment(&path).to_owned();
            let parent = parent_dir(&path, SEP).unwrap_or_else(|| path.clone());
            ui.post(move || {
                if gone {
                    remove_refs_under(&path);
                    let under = format!("{}{SEP}", path.trim_end_matches(SEP));
                    extra.update(|e| {
                        e.retain(|d| !(d == &path || d.starts_with(&under)));
                    });
                    select_folder(cwd, expanded, &parent);
                    show_message(message, format!("已删除 {name}"));
                } else {
                    show_message(message, format!("删除 {name} 失败"));
                }
            });
        });
    }
}
