#!/usr/bin/env bash
# 更新 NSH 并把 Cargo.toml 的 nsh-* 依赖对齐到 CLI 所在频道。
#
#   sh scripts/update-nsh.sh
#
# 只跟 stable：`nsh update stable` 装最新正式版 CLI，依赖写 crates.io
# 版本号。未安装 nsh 时用官方脚本装 stable。
#
# 最后同步 Cargo.toml 依赖说明符（历史遗留的 git/rev nightly 形态也会改回
# version）、AGENTS.md 的 nsh-pin 段、Cargo.lock，并 `nsh check`。

set -euo pipefail
cd "$(dirname "$0")/.."

cli_build() {
  # 括号里的提交号：`nsh 0.7.0 (3f8ecd8)` → 3f8ecd8
  nsh --version | sed -nE 's/.*\(([0-9a-f]+)[^)]*\).*/\1/p'
}

cli_version() {
  # `nsh 0.7.0 (3f8ecd8)` / `nsh 0.7.0` → 0.7.0
  nsh --version | sed -E 's/^nsh ([0-9]+[^ ]*).*/\1/'
}

if ! command -v nsh >/dev/null 2>&1; then
  echo "==> nsh 未安装，用官方脚本安装"
  curl -fsSL https://nsh.dev/install.sh | sh
  # 装进 ~/.local/bin，当前 shell 的 PATH 未必有。
  export PATH="$HOME/.local/bin:$PATH"
fi

echo "==> nsh update stable"
nsh update stable
echo "==> CLI: $(nsh --version)"

version="$(cli_version)"

# Cargo.toml 的 nsh-* 行依赖说明符：git/rev（历史 nightly）与 version 两种
# 形态统一替换为 version。
dep_re='(git = "https://github\.com/longbridge/nsh", rev = "[0-9a-f]{40}"|version = "[0-9]+\.[0-9]+\.[0-9]+[^"]*")'

spec="version = \"$version\""
pin_note="currently \`$version ($(cli_build) stable)\`, version \`$version\`"

if grep -E "^nsh-" Cargo.toml | grep -qvF "$spec"; then
  echo "==> 依赖说明符改为: $spec"
  sed -i '' -E "/^nsh-/s@$dep_re@$spec@" Cargo.toml
else
  echo "==> 依赖已是 $spec，跳过重写"
fi

# AGENTS.md 规则里的 nsh-pin 段与版本同步。
if grep -q '<!-- nsh-pin -->' AGENTS.md; then
  sed -i '' -E "s@<!-- nsh-pin -->[^<]*<!-- /nsh-pin -->@<!-- nsh-pin -->$pin_note<!-- /nsh-pin -->@" AGENTS.md
else
  echo "nsh: AGENTS.md 缺 <!-- nsh-pin --> 标记，跳过规则同步" >&2
fi

echo "==> cargo update（版本变化要重写 lock；别的 pin 太旧时按需级联）"
if ! cargo update -p nsh-core -p nsh-base -p nsh-engine -p nsh-testing -p nsh-updater -p nsh-macros; then
  cargo update
fi

echo "==> nsh check ."
nsh check .
