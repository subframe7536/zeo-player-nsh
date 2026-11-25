# AGENTS.md

ZeoPlayer is a local-first desktop music player written in pure Rust with NSH (macOS AppKit / Windows Win32 + Direct2D). `README.md` is user-facing; keep implementation details here.

## Rules

- **Ask before every `git commit` and `git push`.** Never commit or push on your own.
- **NSH pin**: Track the NSH stable channel only via `sh scripts/update-nsh.sh`. Keep `nsh-core`, `nsh-base`, `nsh-engine`, `nsh-testing`, and `nsh-updater` in `[workspace.dependencies]` synced (<!-- nsh-pin -->currently `0.7.0 (3f8ecd8 stable)`, version `0.7.0`<!-- /nsh-pin -->).
- **Cross-platform**: Keep both macOS and Windows building. Gate OS-specific code with `cfg(target_os = ...)`; don't add a macOS-only path without a Windows counterpart or a documented gap.
- **Docs & workarounds**: Update `README.md` when behavior changes. Record upstream NSH limitations in `upstream.md` and work around them in-app—never patch NSH crates locally.
- **Toolchains**: Resolve `$CARGO_HOME` / `$RUSTUP_HOME` first, falling back to `~/.cargo` / `~/.rustup` only when unset.
- **Formatting**: Run `cargo fmt` before ending every turn.
- **Language & commits**: `README.md` and code comments are in Chinese (match surrounding style). Commit messages are in English: short summary line + bullet list per area.

## Architecture

### Workspace (`crates/zeo-*`)

Bottom-up dependency graph; lower crates must never depend on upper ones (use injected callbacks/mirrors like `set_window_apply_hook`, engine `set_hwnd`, `zeo_types::prefs`):

- **Core**: `zeo-types` (data models + `prefs` mirror) → `zeo-db` (SQLite) → `zeo-metadata` (lofty tags/covers/lyrics/waveforms + symphonia `codec::Source`) → `zeo-library` (`LibraryStore` + `provider`) → `zeo-playlists`, `zeo-engine`
- **Infra**: `zeo-icons` (`nsh-core` only: `glyph`, `render`, `logo`, `morph`), `zeo-keybinds` (no deps; signals/dispatch in `zeo-ui-store`), `zeo-shell` (window / single-instance / tray / reveal), `zeo-updater` (`nsh-updater` + `ureq`; `supported()` is false in dev)
- **UI**: `zeo-ui-store` (signals/theme/fonts) → `zeo-ui-components` → `zeo-ui-views-base` (shared widgets + track menu) → `zeo-ui-view-*` (`home`, `browse`, `folders`, `playlists`, `metadata`, `edit-info`, `settings`, `command-palette`, `welcome`) → `zeo-ui-views` (facade + `library()`) → `zeo-ui-now-playing` → `zeo-ui-frame`
  - Allowed peer edges: `folders`→`playlists::condition_editor`, `home`→`browse::avatar`, `metadata`→`folders::stop_if_playing_under`, `welcome`→`settings::accent_section` (one-way).
- **Root (`zeo-player`)**: `src/lib.rs` (`#[native_module]` + `App`), `e2e.rs` (`#[cfg(test)]`), `tests/`.

### Conventions

- **State & I/O**: UI state/commands live in `zeo-ui-store` (`crates/ui-store/src/lib.rs`). Keep engine/I/O out of `zeo-ui-*` crates; go through `zeo_engine`. Keep module internals `pub(super)` and use `pub` only for cross-crate consumers.
- **Persistence**: `library.json`, `playlists.json`, and settings live in the `dirs` data dir; keep formats backward-compatible or migrate them.
- **CI scope**: `scripts/ci-scope.sh <base>` maps diffs to `crates/*` and runs `cargo test -p` on changed crates + `cargo check --all-targets` on transitive reverse-dependents (`Cargo.toml`/`.lock`/`.github/`/`scripts/` trigger a full run).

## UI & NSH Patterns

- **Layout**: Page headers (`components::page_header`, `views_base::hero`, home greeting) have no top padding, `leading-none` (`theme::PAGE_TITLE`), aligned to `top_inset()`; use `theme::PAGE_X` and `theme::PAGE_HEADER_PB`. Single-line `text()` uses `.line_height(px(size))` (wrapping prose keeps default line height).
- **Signals & threading**:
  - `store()`, `store::later`, and `Owner` are thread-local to the root UI thread. Background threads post back via `store::poster()` (`Post::post`, per-instance channel). Read current track via `player().state()`, not `store().track`.
  - `zeo-shell` window events are instance-scoped (`bind_instance()` + per-instance `Notify` via `listener()`).
  - Thread-local cached signals/memos shared across views (e.g. `folder_ui()`) must be created under `zeo_ui_store::in_root(...)` so tray-hide disposal does not leave dead signals.
  - `dynamic` tracks all signal reads during build: wrap pure construction in `untracked`, and sync external config changes to input drafts via `computed` (not raw `s.library`).
- **Controls (`nsh_base`)**:
  - `components::slider` wraps `nsh_base::slider` (0..1; `step = 0` disables pointer snapping; EQ bipolar & waveform overlay transparent sliders on custom visuals). EQ/volume drags use `apply_eq_live` / `set_volume_live` and only broadcast/publish on release.
  - Dropdowns use `nsh_base::select` (uncontrolled `.open`, `SelectValue::Controlled`, `role:option`); filterable dropdowns use `nsh_base::combobox`; numeric inputs keep `input_field`.
- **Welcome page (`zeo-ui-view-welcome`)**:
  - Full-window `absolute().inset(0).z_index(60)` overlay (not `dialog`), computed synchronously in `install` (`!welcome_done && roots.is_empty()`).
  - Finish/Skip/Esc calls `finish_welcome()` (`welcome_done = true`); reopen via `open_welcome()` without modifying the flag. Disables other root `keys()` while open. UI tests must set `"welcome_done": true` in `library.json`.

## Covers, Lyrics & Track Lists

- **Virtualization & memory**: Keep large lists virtualized. No data URIs or full-size art in global caches/DB (`MetaCache` stores `has_art` only; re-read via `metadata::read_artwork`).
- **List covers**: One cached file per track — 96 px for ≤48 pt slots, lazily upgraded to 400 px for larger slots (`covers::list_cover` + `prune_smaller`). `cover_frame` sets the previous source as `placeholder` to prevent flashing during upgrade. Don't wrap cover-holding pages in a `dynamic` reading `s.tracks` wholesale (gate on `computed`, e.g. `is_empty`).
- **`show_covers` gate**: When off, `cover_for` returns `None` and cover slots are omitted (`when(store::covers_on, …)`). Player bar uses `cover_for_always` + `cover()`. When on without art, `cover_initial` draws a letter tile.
- **Fullscreen & media session**: Fullscreen art and blurred background reuse an 8-entry LRU of 720 px JPEG `data:` URIs (`covers::artwork_uri`; blur is `blur_radius` on macOS, `st.blur()` on Windows). `MediaSession::publish` re-queries `covers::cached_list` (`file://` percent-encoded); Windows additionally pushes thumbnails via `smtc_thumbnail` (`GetForWindow` + `StorageFile`).
- **Lyrics & waveforms**: Loaded on demand (`metadata::read_lyrics`); `engine::Meta::lyrics` caches near playhead + next 3 queue tracks (alongside covers and SQLite-persisted `waveforms`). Cover/lyrics cache keys fold in sidecar timestamps (`jpg/jpeg/png/webp`, `.lrc`) and `prefer_local_*` flags.
- **Track tables**: 音乐库 / 艺术家详情 / 歌单 / 标签详情 show `#` (hover → play/pause); 音乐库 also shows 大小; 专辑详情 shows tag track `#`; others show neither. 音乐库 / 歌单 / 标签 share `views_base::pinned_track_list`. Track rows lead the artist line with `quality_badge` (`MetaInfo::quality_tag()` + `theme::quality_color`).

## Metadata View (`zeo-ui-view-metadata`)

- **Table & data**: Backed by library index + SQLite (0-frame open, no background disk scan; missing DB fields stay empty, fallback only in `TrackShared::display_*`). Uses `nsh_base::data_table` virtual window (`~120` rows), dynamic `visible_cols` (24 cols, default 15), no `render_cell` (native `text(cell_text)` for fast horizontal scroll), and no `classes.fixed_edge` (only `file_name` sets `border-r border-border`).
- **Resize & selection**: `resizable_columns(false)` + custom `render_header_cell` dragging an overlay `guide_line_x` (commits `col_widths` on mouse-up, min 50 pt, double-click resets). Multi-select uses `BTreeSet<PathBuf>` (table `selection` stays empty; clicking a selected row without modifiers deselects it).
- **Panels**: Single-select opens at frame 0 via `track_seed` + 32-entry `FullRow` LRU (async fetch for tag type/`added_at`), uncontrolled inputs (`default_value`), 4 sections, lyrics import (`export::decode_text_file` + `remount`), inline filename rename (`rename::plan_name`). Multi-select uses `wrap_row` (`flex_1().min_w(N)`).
- **Tools & history**: Toolbar buttons open page-level dialogs: Encoding Fix (`export::fix_mojibake` + `Edit::Redecode` resolved on save thread), Batch Rename (`batch_rename.rs`, validates all via `plan_renames` before `fs::rename` + `LibraryStore::move_path` + `move_playlist_paths`), Export Lyrics/Covers (`batch_export.rs`), and Tag History (single-track only, SQLite schema v10 `tag_batches`/`tag_snapshots`, `history_reload` rebuilds panel with `fresh = true`).

## Windowing, Single Instance & File Ops

- **Single instance**: Windows relies on `nsh.json` `"singleInstance": true` (`nsh\app.json` at runtime; portable updates must replace the whole dir). macOS uses `zeo-shell/src/single_instance.rs` (`flock` on `<bundle id>.lock` + `activate` + distributed notification `dev.subf.zeo-player.show`, called in `native()` and `App()`; skipped in debug/`cfg(test)`).
- **Flicker-free startup**: Desktop root declares `app_window().id("main")` (`window.defaultVisible` is unused in NSH v2). When `remember_frame` has a saved placement, `install` synchronously calls `hide()` + `set_placement` on first render, computes cover accent sync, and `reveal_when_ready` calls `show()` once `restore_last` finishes (capped at `REVEAL_MAX_MS`). Windows subclasses the window in `zeo_shell::start` (`WM_DESTROY`, SMTC `hwnd`, `zeo_shell::restart()` sets `RESTARTING`).
- **File operations**: Moves/renames use `LibraryStore::move_path` + `store::move_playlist_paths`; deletes use `remove_under` + `remove_playlist_paths_under`. Watcher pairs renames by `(size, mtime)`. macOS trash uses `DeleteMethod::NsFileManager`.

## Audio Engine, Tray & Memory

- **Output & null fallback**: `sync_stream` ties `Output.running` to `playing` (pause immediately on rebuild). Without an audio device, `Sink::Null` drives timing/stats/gapless switch while `probe_stream` retries every 1s.
- **Gapless & EQ**: `prefetch_next` + `boundary` frame counter handles gapless transitions (`reset_buffers` drops prefetch on seek/stop/queue/loop change; ~2 ms underrun fade). EQ (`engine/eq.rs`) uses 10× RBJ peaking biquads with ~5 ms coefficient smoothing, peak headroom scaling, denormal flush, and `eq_gen` + `try_lock`.
- **Tray & playback intent**: Left-click immediately pushes `TrayAction::Toggle` (no double-click detection); "显示 Zeo Player" restores the window. `store::shown_playing()` reads `intent` (`INTENT_TTL_MS`) before falling back to engine state.
- **Tray-hide memory trim (`store.rs`)**: Immediately dispose cover owners, clear artwork/lyrics/waveform/MetaCache in-memory LRUs & `sqlite::shrink_memory`. After 1s, clear `nsh_core::image::budget()` then call `window::trim_memory()` (`malloc_zone_pressure_relief(NULL)` on macOS; `HeapCompact` + `EmptyWorkingSet` on Windows), repeating every 30s while hidden. Keep the system allocator (mimalloc regresses footprint).
- **Windows media keys (issue #11)**: `zeo-player-kbhook` installs `WH_KEYBOARD_LL` to intercept and swallow the 4 media VKs; `WM_ACTIVATE` sends `WM_REHOOK_MEDIA` so the last-focused player stays at chain head.

## App Icons

- `resources/icon.svg` is the single source (`icon-windows.png`: full-bleed 1024; `icon.png`: 824 px `rx=231` padded to 1024 with soft shadow; `icon-windows.ico`: derived from `icon-windows.png`; title bar embeds `icon.svg`). Transparent-pixel RGB must be merged from a `rx=0` square render scaled to matching geometry to avoid dark fringe artifacts.
- Tray icons reuse `zeo_icons::play_pause_svg` (macOS: `ImageSrc::svg_markup` template; Windows: rasterized 64 px non-premultiplied `ImageSrc::rgba` on rounded tile).

```sh
# Regenerate macOS & Windows PNGs from resources/icon.svg
sed 's/rx="[0-9.]*"/rx="0"/' resources/icon.svg > .nsh/tmp/icon-square.svg
sed 's/rx="[0-9.]*"/rx="231"/' resources/icon.svg > .nsh/tmp/icon-macos.svg
# macOS (sips) / Windows (pnpm dlx sharp-cli):
sips -s format png -z 1024 1024 .nsh/tmp/icon-square.svg --out .nsh/tmp/icon-square.png
sips -s format png -z 1024 1024 .nsh/tmp/icon-macos.svg --out .nsh/tmp/icon-macos.png
sips -s format png -z 1024 1024 resources/icon.svg --out .nsh/tmp/icon-normal.png
ffmpeg -i .nsh/tmp/icon-square.png -i .nsh/tmp/icon-normal.png -filter_complex \
  "[1:v]format=rgba,alphaextract[a];[0:v]format=rgb24[rgb];[rgb][a]alphamerge[out]" \
  -map "[out]" resources/icon-windows.png
ffmpeg -i .nsh/tmp/icon-square.png -i .nsh/tmp/icon-macos.png -filter_complex \
  "[0:v]scale=824:824,format=rgb24,pad=1024:1024:100:100:color=black[rgb]; \
   [1:v]scale=824:824,format=rgba,pad=1024:1024:100:100:color=black@0,alphaextract[a]; \
   [rgb][a]alphamerge[out]" -map "[out]" .nsh/tmp/icon-tile.png
ffmpeg -i .nsh/tmp/icon-macos.png -vf \
  "scale=824:824,format=rgba,pad=1024:1024:100:100:color=black@0,alphaextract,gblur=sigma=8,lut=y='val*0.32'" \
  .nsh/tmp/icon-shadow.png
ffmpeg -f lavfi -i "color=black:s=1024x1024,format=rgba" -i .nsh/tmp/icon-shadow.png -i .nsh/tmp/icon-tile.png -filter_complex \
  "[0:v][1:v]alphamerge[sh];color=black@0:s=1024x1024,format=rgba[bg];[bg][sh]overlay=0:8[b];[b][2:v]overlay=0:0[out]" \
  -map "[out]" -frames:v 1 resources/icon.png
uv run --with pillow python -c "from PIL import Image; Image.open('resources/icon-windows.png').save('resources/icon-windows.ico', format='ICO', sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])"
```

## Commands & Testing

```sh
sh scripts/update-nsh.sh  # sync the stable version across Cargo.toml, AGENTS.md, Cargo.lock + nsh check
nsh check .               # cargo check + type check
nsh test .                # unit tests + tests/ui.rs (headless UI tests)
nsh run .                 # run with rebuild on change (uses .nsh/dev-data; override via ZEO_PLAYER_DATA_DIR)
nsh package . --target macos
nsh package . --target windows --icon resources/icon-windows.png
```

- **Pre-completion**: Run `nsh check .` and `nsh test .` before reporting a change as done. CI (`.github/workflows/ci.yml`) runs both on macOS & Windows.
- **Test fixtures**: Run `sh scripts/make-test-music.sh` (requires `ffmpeg`) to generate `.nsh/tmp/zeo-player-test-music` and `.nsh/tmp/zeo-player-test-formats` (keep new format samples out of `zeo-player-test-music`).
- **UI & ignored tests**: Wait for fading overlays with `wait_for_gone` before `press`. Tests writing `playlists.json` must hold `play_lock()`. Run `#[ignore]` tests one at a time (`cargo test --release -- --ignored scan_reports_progress` / `watcher_rescans` / `real_track_waveform`).
- **Windows tests & cross-check**: Windows tests need `LIBCLANG_PATH` pointing to LLVM `bin` (`libclang.dll`). Cross-check Windows from macOS via `cargo-xwin`:
  `PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH" cargo xwin check --target x86_64-pc-windows-msvc`
- **Memory bench & release**: `ZEO_PLAYER_BENCH=1 nsh run .` (`scroll`/`hidden`/`close`, debug/test only). Release updates use `nsh update-manifest` (`latest.json` + `.sig`); macOS distribution requires Developer ID signing + `xcrun notarytool` + `xcrun stapler`.
