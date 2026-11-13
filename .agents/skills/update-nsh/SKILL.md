---
name: update-nsh
description: 'Update the NSH dependency pin in this repo: run scripts/update-nsh.sh, re-check every upstream.md workaround against the new upstream revision, remove entries whose fix landed, and verify the build. Use whenever asked to update/bump nsh or to re-check upstream.md.'
---

# Update NSH and re-check upstream workarounds

Two halves: (1) move the `nsh-*` pins to the newest NSH channel, (2) audit
`upstream.md` against the new upstream sources so fixed entries and their
workarounds get removed. Do both — an update that skips the audit leaves stale
docs; an audit without the pin update checks nothing new.

## 1. Run the script

```sh
sh scripts/update-nsh.sh
```

It runs `nsh update nightly`, falls back to `nsh update stable` when the
nightly tag sits behind the latest stable release, rewrites the `nsh-*`
specifiers in the root `Cargo.toml` (nightly → `git = ..., rev = ...`; stable
→ `version = ...`), refreshes the `<!-- nsh-pin -->` note in `AGENTS.md`,
runs `cargo update` on the nsh crates and finishes with `nsh check .`.

**Sanity-check its channel detection before trusting the diff.** The script
parses `nsh --version`. Upstream changed the format once already
(`nsh 0.5.0 (0edac01 nightly 2026-11-12)` — a build date after `nightly` broke
the old `nightly\)` regex and a nightly got mislabeled stable in AGENTS.md).
Compare `nsh --version` with what the script prints and with the resulting
diff:

- nightly CLI → Cargo.toml lines must be `git = "...", rev = "<40-hex>"` and
  the pin note must say `nightly`; the rev must equal
  `git ls-remote --tags https://github.com/longbridge/nsh nightly`
- stable CLI → `version = "x.y.z"` and the pin note says `stable`

If the parse is wrong, fix the regex in `cli_commit()` / `cli_build()` and
re-run — never hand-edit Cargo.toml around the script.

## 2. Sync the CI channel

`.github/actions/setup/action.yml` installs the CLI matching the pin:

- nightly → macOS step uses `install.sh | sh -s -- --nightly` and the Windows
  step sets `$env:NSH_VERSION = 'nightly'`
- stable → bare `install.sh | sh` and no `NSH_VERSION`

Also fix the "… today" wording in the AGENTS.md pin sentence if the channel
changed. `git log -p .github/actions/setup/action.yml` shows both shapes.

## 3. Audit upstream.md against the new revision

The nsh sources are already on disk — cargo fetched the git checkout. Cargo's
home comes from `$CARGO_HOME`, not a hardcoded `~/.cargo`:

```sh
CH=${CARGO_HOME:-$HOME/.cargo}
# git deps (nightly): one dir per rev, short-hash named
NSH=$(find "$CH/git/checkouts" -maxdepth 2 -type d -name "<shortrev>" | head -1)
# crates.io deps (stable): "$CH/registry/src/*/nsh-*/"  (per-crate)
```

Get the delta, then check each entry's named files:

```sh
cd "$NSH" && git log --oneline <old-rev>..<new-rev>
git log --oneline <old>..<new> -- crates/nsh-base/src/data_table/ \
  crates/nsh-core/src/overlay.rs crates/nsh-core/src/keymap.rs \
  crates/nsh-platform-macos/src/visual.rs \
  crates/nsh-platform-windows/src/text_input.rs \
  crates/nsh-platform-windows/src/window_state.rs \
  crates/nsh-testing/src/headless/ # …the files each entry names
```

Per upstream.md entry: read the code it cites (e.g. `title_content`,
`resize_move`, `InputState::record`, headless `right_click`). A merged PR
number alone doesn't prove the documented pattern changed — confirm the
actual lines. When an entry's issue is fixed:

- remove the entry AND the app-side workaround it describes (follow the file
  references in the entry; `git grep` the symbol if moved)
- note the removal + fix PR in the header paragraph of upstream.md
- entries marked 待实机复核 need a real Windows machine — keep them unless
  the code clearly changed; mark what the new code does

Entries whose pattern is unchanged: leave them, but update stale claims
(e.g. "nightly 标签冻结" was true until it wasn't — re-verify
`git ls-remote --tags … nightly` each time).

## 4. Verify

```sh
cargo check --workspace --all-targets
cargo test --workspace   # dep changes => CI does a full run; match it
cargo fmt
```

Watch for upstream behavior changes that break headless tests even when the
entries don't mention them (focus/keymap ordering, scroll anchoring, overlay
press rules have all shifted between revs).

## 5. Commit

English message, summary line + bullet list per area (see `git log`). The
repo rule stands: **ask before `git commit` / `git push`**.
