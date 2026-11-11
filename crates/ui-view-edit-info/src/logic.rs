use super::*;

/// 草稿 → 写回改动：与原值相同的字段 Keep；改空的 Clear。歌词按
/// `lrc_target` 只走一条道——写 `.lrc` 时内嵌标签原样留着（另一栏的
/// 草稿不生效）。
pub(crate) fn build_edits(d: &TagDraft) -> TagEdits {
    let fields = TagField::ALL
        .iter()
        .filter_map(|f| {
            if d.lrc_target && *f == TagField::Lyrics {
                return None;
            }
            let old = d.original.get(f).map(String::as_str).unwrap_or_default();
            let new = d.values.get(f).map(|s| s.trim()).unwrap_or_default();
            if new == old {
                None
            } else if new.is_empty() {
                Some((*f, Edit::Clear))
            } else {
                Some((*f, Edit::Set(new.to_owned())))
            }
        })
        .collect();
    let lrc = if !d.lrc_target {
        None
    } else {
        let new = d.lrc_text.trim();
        if new == d.lrc_original.trim() {
            None
        } else if new.is_empty() {
            Some(Edit::Clear)
        } else {
            Some(Edit::Set(new.to_owned()))
        }
    };
    TagEdits {
        fields,
        // 只写选中的目标；另一头的草稿留着（切回去内容还在），不生效。
        cover: if d.cover_target {
            None
        } else {
            d.cover.clone()
        },
        cover_file: if d.cover_target {
            d.cover_file.clone()
        } else {
            None
        },
        lrc,
    }
}

/// 数字字段本地校验（与 `write::validate` 同规）：音轨 / 碟号必须正整数，
/// 年份 `u16` 整数；为空合法（Clear）。
pub(crate) fn valid(d: &TagDraft) -> bool {
    let numeric = [
        TagField::Year,
        TagField::Track,
        TagField::TrackTotal,
        TagField::Disc,
        TagField::DiscTotal,
    ];
    numeric.iter().all(|f| {
        let Some(v) = d.values.get(f) else {
            return true;
        };
        let v = v.trim();
        if v.is_empty() {
            return true;
        }
        if *f == TagField::Year {
            v.parse::<u16>().is_ok()
        } else {
            v.parse::<u32>().is_ok_and(|n| n > 0)
        }
    })
}

pub(crate) fn save(draft: Signal<TagDraft>) {
    let s = store();
    let d = draft.get_untracked();
    draft.update(|d| {
        d.saving = true;
        d.error = None;
    });
    store::save_tags(
        d.path.clone(),
        build_edits(&d),
        move |result| match result {
            Ok(()) => s.edit_info.set(None),
            Err(error) => draft.update(|d| {
                d.saving = false;
                d.error = Some(error);
            }),
        },
    );
}

// --- 基础元数据 ----------------------------------------------------------------
