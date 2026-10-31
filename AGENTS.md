# AGENTS.md

Zeo Player is a local-first desktop music player written as a pure-Rust NSH (Native Shell) app for macOS (AppKit) and Windows (Win32 / Direct2D). Read `README.md` for features, layout, and platform differences, and `plan.md` for design decisions and algorithm specs.

## Rules

- **Ask before every `git commit` and `git push`.** Never commit or push on your own.
- For the full-screen player, lyrics, and waveform, `plan.md` ("audio-playground 实现规格") is the spec. Do not fetch or parse the audio-playground repository unless explicitly asked.
- `nsh-core`, `nsh-base`, `nsh-engine`, and `nsh-testing` in `Cargo.toml` must match `nsh --version` (currently 0.3.0). Bump them together with the CLI.
- Keep both platforms building. Platform code goes behind `cfg(target_os = ...)`; don't add a macOS-only path without a Windows counterpart or a documented gap.
- When behavior changes, update `README.md` (it's the user-facing feature list). Record upstream NSH limitations under "待上游（NSH）".
- Don't patch NSH crates locally to work around problems. Work around them in the app and document the limitation.

## Conventions

- Docs (`README.md`, `plan.md`) and code comments are in Chinese. Match the surrounding style.
- Commit messages are in English: a short summary line, then a bullet list per area (see `git log`).
- UI is in `src/ui/`; state and commands live in `src/ui/store.rs` as signals. Engine and I/O code stays out of `src/ui/` and is reached through `src/engine.rs`.
- Large lists are virtualized; keep them that way. Watch memory: no data URIs, no full-size artwork in caches (see "内存" in the README).
- Persisted files (`library.json`, `playlists.json`, settings) live in the data directory from `dirs`. Keep their formats backward compatible or migrate them.

## Commands

```sh
nsh check .        # cargo check + type check
nsh test .         # unit tests + tests/ui.rs (headless UI tests)
nsh run .          # run the app with rebuild on change
```

- Run `nsh check .` and `nsh test .` before reporting a change as done.
- Playback e2e tests and track-dependent UI tests need `/tmp/zeo-player-test-music` (`sh scripts/make-test-music.sh`, requires ffmpeg). Without it they skip silently, so generate it when touching audio, lyrics, waveform, or the player UI.
- `#[ignore]` tests use the real library / filesystem and share global state. Run them one at a time (commands in the README).
- Cross-check Windows from a Mac with `cargo xwin check --target x86_64-pc-windows-msvc` (setup in the README) when touching platform code.
- Memory benchmark: `ZEO_PLAYER_BENCH=1 nsh run .` and sample with `footprint <pid>`.

CI (`.github/workflows/ci.yml`) runs `nsh check` and `nsh test` on macOS and Windows for every push and PR.
