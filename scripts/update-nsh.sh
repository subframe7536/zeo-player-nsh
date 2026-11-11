#!/usr/bin/env bash
# 更新 NSH 到 nightly 并把 Cargo.toml 的 nsh-* rev 钉到 CLI 报告的提交。
#
#   sh scripts/update-nsh.sh
#
# nightly 不发 crates.io，依赖走 git rev；`nsh update nightly` 只换 CLI，
# 这里把 rev、AGENTS.md 规则里的提交串、Cargo.lock 一起对齐，最后 `nsh check`。

set -euo pipefail
cd "$(dirname "$0")/.."

cli_commit() {
  # `nsh 0.4.0 (36b9168 nightly)` → 36b9168；稳定版（无括号提交）返回空。
  nsh --version | sed -nE 's/.*\(([0-9a-f]+).*/\1/p'
}

echo "==> nsh update nightly"
nsh update nightly

short="$(cli_commit)"
if [ -z "$short" ]; then
  echo "nsh: $(nsh --version) 不是 nightly 频道（无提交号），不改依赖" >&2
  exit 1
fi
echo "==> CLI: $(nsh --version)"

# nightly tag 就建在 nightly 的提交上，取它的完整 SHA 并核对短号。
rev="$(git ls-remote --tags https://github.com/longbridge/nsh nightly | awk '{print $1}')"
case "$rev" in
  "$short"*) ;;
  *)
    echo "nsh: nightly tag ($rev) 与 CLI 提交的短号 $short 对不上，先确认 tag 是不是又前移了" >&2
    exit 1
    ;;
esac

current="$(sed -nE 's/.*rev = "([0-9a-f]{40})".*/\1/p' Cargo.toml | head -1)"
if [ "$current" = "$rev" ]; then
  echo "==> rev 已是 $rev，跳过重写"
else
  echo "==> rev: $current -> $rev"
  # 全部 nsh-* 行共用同一个 rev，一次替换。
  sed -i '' -E "s/rev = \"[0-9a-f]{40}\"/rev = \"$rev\"/" Cargo.toml
fi

# AGENTS.md 规则里的「currently」串与 rev 同步。
version="$(nsh --version | sed -E 's/^nsh ([0-9]+[^ ]*).*/\1/')"
sed -i '' -E "s/currently \`[0-9.]+ \([0-9a-f]+ nightly\)\`, rev \`[0-9a-f]{40}\`/currently \`$version ($short nightly)\`, rev \`$rev\`/" AGENTS.md

echo "==> cargo update（新 rev 要重写 lock；别的 pin 太旧时按需级联）"
if ! cargo update -p nsh-core -p nsh-base -p nsh-engine -p nsh-testing -p nsh-macros; then
  cargo update
fi

echo "==> nsh check ."
nsh check .
