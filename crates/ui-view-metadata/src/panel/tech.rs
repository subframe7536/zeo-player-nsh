use super::*;

/// 「音频参数」节：常开，每行尾部一颗复制钮；「文件夹」行收尾（文件
/// 名在顶栏改，这里不再放）。编码信息读 `MetaCache`（曲库条目都已在
/// 缓存里），标签类型与入库时间随 `full` 落地即准。
pub(crate) fn tech_section(
    path: &str,
    rows: Signal<HashMap<String, Arc<MetaRow>>>,
    full: Signal<Option<FullRow>>,
) -> View {
    let entry = untracked(|| {
        zeo_metadata::MetaCache::global()
            .entry(Path::new(path))
            .ok()
    });
    // 标签类型：`full` 的权威值优先，未落地回落行里的（MetaRow 由同
    // 一条 read_row 得来）。
    let tag_type = {
        let path = path.to_owned();
        move || {
            full.get()
                .and_then(|(_, t, _)| t)
                .or_else(|| rows.with(|r| r.get(&path).and_then(|row| row.tag_type)))
                .unwrap_or("无")
                .to_owned()
        }
    };
    let mut lines = Vec::new();
    if let Some(entry) = &entry {
        let m = &*entry.value;
        lines.push(tech_line("品质", quality_label(m)));
        lines.push(tech_line("编码", m.codec.clone().unwrap_or_default()));
        lines.push(tech_line_dyn("标签类型", tag_type));
        lines.push(tech_line(
            "采样率",
            m.sample_rate
                .map(|r| format!("{:.1} kHz", r as f64 / 1000.0))
                .unwrap_or_default(),
        ));
        lines.push(tech_line(
            "位深",
            m.bit_depth.map(|d| format!("{d} bit")).unwrap_or_default(),
        ));
        lines.push(tech_line(
            "比特率",
            m.bit_rate.map(|b| format!("{b} kbps")).unwrap_or_default(),
        ));
        lines.push(tech_line(
            "声道",
            m.channels.map(|c| c.to_string()).unwrap_or_default(),
        ));
        lines.push(tech_line("时长", store::format_time(m.duration)));
        lines.push(tech_line(
            "大小",
            format!("{:.1} MB", entry.size as f64 / 1_048_576.0),
        ));
        lines.push(tech_line(
            "修改时间",
            if entry.modified == 0 {
                String::new()
            } else {
                zeo_types::config::format_local_datetime(entry.modified)
            },
        ));
    }
    // 入库时间随 `full` 落地（后台同趟读 `db::added_at`）；没记过显示 —。
    lines.push(tech_line_dyn("入库时间", move || {
        full.get()
            .and_then(|(_, _, added)| added)
            .map(zeo_types::config::format_local_datetime)
            .unwrap_or_else(|| "—".to_owned())
    }));
    lines.push(tech_line(
        "文件夹",
        Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    ));
    section("音频参数", lines)
}

/// 技术参数行：72pt 标签 + 值 + 复制钮（`set_clipboard`，标签拼进
/// 无障碍名便于测试断言）。
pub(crate) fn tech_line_dyn(label: &'static str, value: impl Fn() -> String + 'static) -> View {
    let value = Rc::new(value);
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text(label)
                .w(px(72.0))
                .text_size(px(12.0))
                .line_height(px(12.0))
                .text_color(muted_fg()),
        )
        .child(
            text({
                let value = value.clone();
                move || value()
            })
            .flex_1()
            .min_w(px(0.0))
            .text_size(px(12.0))
            .line_height(px(16.0))
            .text_color(fg()),
        )
        .child(
            shell(Variant::Ghost, Size::IconXs)
                .accessibility_label(SharedString::from(format!("复制{label}")))
                .on_press(move |_: &()| set_clipboard(value()))
                .child(icon(Icon::Copy, 11.0, muted_fg)),
        )
}

/// 静态值版 [`tech_line_dyn`]。
pub(crate) fn tech_line(label: &'static str, value: String) -> View {
    tech_line_dyn(label, move || value.clone())
}

/// 品质徽章：Lossless + 高采样 → Hi-Res；Lossless → SQ；Lossy ≥320 → HQ；
/// 其余回落编码名。
pub(crate) fn quality_label(m: &zeo_metadata::MetaInfo) -> String {
    match m.quality.as_deref() {
        Some("Lossless")
            if m.bit_depth.is_some_and(|d| d > 16) || m.sample_rate.is_some_and(|r| r > 48000) =>
        {
            "Hi-Res".to_owned()
        }
        Some("Lossless") => "SQ".to_owned(),
        Some("Lossy") if m.bit_rate.is_some_and(|b| b >= 320) => "HQ".to_owned(),
        _ => m
            .codec
            .clone()
            .or_else(|| m.quality.clone())
            .unwrap_or_default(),
    }
}
