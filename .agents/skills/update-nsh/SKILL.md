---
name: update-nsh
description: 'Update the NSH dependency pin in this repo: run scripts/update-nsh.sh, re-check every upstream.md workaround against the new upstream revision, remove entries whose fix landed, and verify the build. Use whenever asked to update/bump nsh or to re-check upstream.md.'
---

# Update NSH and re-check upstream workarounds

Two halves: (1) move the `nsh-*` pins to the newest **stable** release
(the repo tracks stable only — never nightly), (2) audit `upstream.md`
against the new upstream sources so fixed entries and their workarounds
get removed. Do both — an update that skips the audit leaves stale docs;
an audit without the pin update checks nothing new.

## 1. Run the script

```sh
sh scripts/update-nsh.sh
```

It runs `nsh update stable`, rewrites the `nsh-*` specifiers in the root
`Cargo.toml` to `version = ...` (a leftover `git = ..., rev = ...`
nightly pin is converted back), refreshes the `<!-- nsh-pin -->` note in
`AGENTS.md`, runs `cargo update` on the nsh crates and finishes with
`nsh check .`.

**Sanity-check the version parse before trusting the diff.** The script
parses `nsh --version` (`nsh 0.7.0 (3f8ecd8)`) for the version and the
build hash. Compare `nsh --version` with what the script prints and with
the resulting diff: Cargo.toml lines must be `version = "x.y.z"` and the
pin note must say `stable`. If the parse is wrong, fix the regex in
`cli_version()` / `cli_build()` and re-run — never hand-edit Cargo.toml
around the script.

## 2. CI

`.github/actions/setup/action.yml` installs the stable CLI: bare
`install.sh | sh` on macOS/Linux, plain `install.ps1` on Windows (no
`--nightly`, no `NSH_VERSION`). It should not need touching — it only
matters if upstream changes the install interface.

## 3. Audit upstream.md against the new revision

Crates.io publishes only `nsh-core` / `nsh-base` / `nsh-engine` /
`nsh-testing` / `nsh-updater` (plus internals); the platform crates
(`nsh-platform-macos`, `nsh-platform-windows`) that several upstream.md
entries cite are NOT published. To diff entry files or read unpublished
sources, fetch the two version tags into a scratch clone:

```sh
tmp="$(mktemp -d)" && git -C "$tmp" init -q
git -C "$tmp" fetch -q --depth 1 https://github.com/longbridge/nsh \
  "refs/tags/v<old>:refs/tags/old" "refs/tags/v<new>:refs/tags/new"
git -C "$tmp" fetch -q --deepen 200 https://github.com/longbridge/nsh \
  "refs/tags/v<old>:refs/tags/old"  # only if log/diff needs history
git -C "$tmp" diff old new -- crates/nsh-base/src/data_table/ \
  crates/nsh-core/src/keymap.rs \
  crates/nsh-platform-macos/src/visual.rs \
  crates/nsh-platform-windows/src/text_input.rs \
  crates/nsh-testing/src/headless/ # …the files each entry names
```

Published crates are also on disk under
`${CARGO_HOME:-$HOME/.cargo}/registry/src/*/nsh-<crate>-<ver>/` — fine for
reading current code, but they carry no git history for a delta.

Per upstream.md entry: read the code it cites (e.g. `title_content`,
`resize_move`, `InputState::record`, headless `right_click`). A merged PR
number alone doesn't prove the documented pattern changed — confirm the
actual lines. When an entry's issue is fixed:

- remove the entry AND the app-side workaround it describes (follow the
  file references in the entry; `git grep` the symbol if moved)
- note the removal + fix PR in the header paragraph of upstream.md
- entries marked 待实机复核 need a real Windows machine — keep them
  unless the code clearly changed; mark what the new code does

Entries whose pattern is unchanged: leave them, but update stale claims
(old rev numbers in "X 复核" notes).

## 4. Verify

```sh
cargo check --workspace --all-targets
cargo test --workspace   # dep changes => CI does a full run; match it
# Windows-only code paths:
PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH" \
  cargo xwin check --target x86_64-pc-windows-msvc
cargo fmt
```

Watch for upstream behavior changes that break headless tests even when
the entries don't mention them (focus/keymap ordering, scroll anchoring,
overlay press rules have all shifted between revs).

## 5. Commit

English message, summary line + bullet list per area (see `git log`).
The repo rule stands: **ask before `git commit` / `git push`**.
