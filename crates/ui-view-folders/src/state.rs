use super::*;

#[derive(Clone)]
pub(crate) struct FolderUi {
    pub(crate) here: Memo<Place>,
    pub(crate) all_rows: Memo<Rc<Vec<TreeRow>>>,
    pub(crate) files: Memo<Rc<Vec<usize>>>,
    pub(crate) select: Rc<dyn Fn(&str)>,
    pub(crate) toggle: Rc<dyn Fn(&str, bool)>,
    pub(crate) ops: Rc<Ops>,
    pub(crate) ctx: Rc<DragCtx>,
    pub(crate) list_ref: ElementRef,
    pub(crate) scroll: Signal<f32>,
}

pub(crate) fn create_folder_ui() -> FolderUi {
    let s = store();
    let select: Rc<dyn Fn(&str)> = Rc::new(move |path: &str| {
        s.folder_sel.set(Some(path.to_owned()));
        s.folder_expanded.update(|e| {
            e.insert(path.to_owned());
            expand_ancestors(e, path, SEP);
        });
        let nav = global::<zeo_ui_store::Nav>();
        if nav.get_untracked() != zeo_ui_store::Nav::Folders {
            nav.set(zeo_ui_store::Nav::Folders);
        }
    });
    let toggle: Rc<dyn Fn(&str, bool)> = Rc::new(move |path: &str, open: bool| {
        s.folder_expanded.update(|e| {
            if open {
                e.remove(path);
            } else {
                e.insert(path.to_owned());
            }
        });
    });
    let ops = Rc::new(Ops {
        op: s.folder_op,
        extra: s.folder_extra,
        cwd: s.folder_sel,
        expanded: s.folder_expanded,
        message: s.folder_message,
    });
    let tree = computed(move || {
        let roots = s.library.with(|l| l.config.roots.clone());
        let tracks = s.tracks.get();
        let mut extra: BTreeSet<String> = s.dirs.get().iter().cloned().collect();
        for d in s.folder_extra.get() {
            if Path::new(&d).is_dir() {
                extra.insert(d);
            }
        }
        let extra_vec: Vec<String> = extra.into_iter().collect();
        let t = build_tree(&roots, &tracks, &extra_vec);
        Same(Arc::new(t))
    });
    let here = computed(move || {
        let tree = tree.get();
        place(&tree, s.folder_sel.get().as_deref())
    });
    let all_rows = computed(move || {
        let r = flatten_with(&tree.get(), &s.folder_expanded.get());
        Rc::new(r)
    });
    let files = computed(move || here.with(|p| p.files.clone()));

    let active = signal(None::<DragState>);
    let over = signal(None::<String>);
    let anchor = signal((-10000.0, -10000.0));
    let scroll = signal(0.0_f32);
    let gx = animated_value(0.0);
    let gy = animated_value(0.0);
    let list_ref = element_ref();
    let ctx = Rc::new(DragCtx {
        active,
        over,
        anchor,
        extra: s.folder_extra,
        rows: all_rows,
        scroll,
        geo: Rc::new(Cell::new(None)),
        down: Rc::new(Cell::new(None)),
        gx,
        gy,
        list: list_ref,
        select: select.clone(),
        message: s.folder_message,
    });

    FolderUi {
        here,
        all_rows,
        files,
        select,
        toggle,
        ops,
        ctx,
        list_ref,
        scroll,
    }
}

thread_local! {
    static FOLDER_UI: RefCell<Option<FolderUi>> = const { RefCell::new(None) };
}

pub(crate) fn folder_ui() -> FolderUi {
    FOLDER_UI.with(|cell| {
        let mut b = cell.borrow_mut();
        if let Some(ui) = b.as_ref() {
            return ui.clone();
        }
        let ui = create_folder_ui();
        *b = Some(ui.clone());
        ui
    })
}
