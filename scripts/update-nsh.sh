#!/usr/bin/env bash
# 更新 NSH 并把 Cargo.toml 的 nsh-* 依赖对齐到 CLI 所在频道。
#
#   sh scripts/update-nsh.sh
#
# 频道选择：先 `nsh update nightly`；若 nightly 的基线版本落后于最新 stable
# tag（nightly 冻结在发布点之前、没跟上），改 `nsh update stable`。
# 未安装 nsh 时用官方脚本装 nightly，装完仍走同一套频道判断。
#
# nightly 不发 crates.io，依赖走 git rev；stable 走 crates.io 版本号。
# 最后同步 Cargo.toml 依赖说明符、AGENTS.md 的 nsh-pin 段、Cargo.lock，
# 并 `nsh check`。

set -euo pipefail
cd "$(dirname "$0")/.."

GIT_URL="https://github.com/longbridge/nsh"

cli_commit() {
  # `nsh 0.4.0 (36b9168 nightly)` → 36b9168；稳定版（无括号提交）返回空。
  nsh --version | sed -nE 's/.*\(([0-9a-f]+).*/\1/p'
}

cli_version() {
  # `nsh 0.4.0 (36b9168 nightly)` / `nsh 0.5.0` → 0.4.0 / 0.5.0
  nsh --version | sed -E 's/^nsh ([0-9]+[^ ]*).*/\1/'
}

version_lt() {
  # version_lt A B：语义化版本 A < B
  [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -1)" = "$1" ]
}

if ! command -v nsh >/dev/null 2>&1; then
  echo "==> nsh 未安装，用官方脚本安装"
  curl -fsSL https://nsh.dev/install.sh | sh -s -- --nightly
  # 装进 ~/.local/bin，当前 shell 的 PATH 未必有。
  export PATH="$HOME/.local/bin:$PATH"
fi

# 最新 stable 发布版本（拿不到就置空，后面不做回退判断）。
latest_stable="$(git ls-remote --tags "$GIT_URL" | sed -nE 's|.*refs/tags/v([0-9]+\.[0-9]+\.[0-9]+)$|\1|p' | sort -V | tail -1)"

echo "==> nsh update nightly"
nsh update nightly
if [ -n "$latest_stable" ] && version_lt "$(cli_version)" "$latest_stable"; then
  echo "==> nightly 停在 $(cli_version)，落后 stable v$latest_stable，改跟 stable"
  nsh update stable
fi
echo "==> CLI: $(nsh --version)"

version="$(cli_version)"
short="$(cli_commit)"

# Cargo.toml 的 nsh-* 行依赖说明符：git/rev 与 version 两种形态互相替换。
dep_re='(git = "https://github\.com/longbridge/nsh", rev = "[0-9a-f]{40}"|version = "[0-9]+\.[0-9]+\.[0-9]+[^"]*")'

if [ -n "$short" ]; then
  # nightly：依赖走 git rev。nightly tag 就建在 nightly 的提交上，取它的完整
  # SHA 并核对短号。
  rev="$(git ls-remote --tags "$GIT_URL" nightly | awk '{print $1}')"
  case "$rev" in
    "$short"*) ;;
    *)
      echo "nsh: nightly tag ($rev) 与 CLI 提交的短号 $short 对不上，先确认 tag 是不是又前移了" >&2
      exit 1
      ;;
  esac
  spec="git = \"$GIT_URL\", rev = \"$rev\""
  pin_note="currently \`$version ($short nightly)\`, rev \`$rev\`"
else
  # stable：依赖走 crates.io 版本号。
  spec="version = \"$version\""
  pin_note="currently \`$version stable\`, version \`$version\`"
fi

if grep -E "^nsh-" Cargo.toml | grep -qvF "$spec"; then
  echo "==> 依赖说明符改为: $spec"
  sed -i '' -E "/^nsh-/s@$dep_re@$spec@" Cargo.toml
else
  echo "==> 依赖已是 $spec，跳过重写"
fi

# AGENTS.md 规则里的 nsh-pin 段与频道 / 版本同步。
if grep -q '<!-- nsh-pin -->' AGENTS.md; then
  sed -i '' -E "s@<!-- nsh-pin -->[^<]*<!-- /nsh-pin -->@<!-- nsh-pin -->$pin_note<!-- /nsh-pin -->@" AGENTS.md
else
  echo "nsh: AGENTS.md 缺 <!-- nsh-pin --> 标记，跳过规则同步" >&2
fi

echo "==> cargo update（频道 / rev 变化要重写 lock；别的 pin 太旧时按需级联）"
if ! cargo update -p nsh-core -p nsh-base -p nsh-engine -p nsh-testing -p nsh-updater -p nsh-macros; then
  cargo update
fi

echo "==> nsh check ."
nsh check .
