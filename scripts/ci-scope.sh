#!/bin/sh
# 算出本次 diff 要测 / 要查的 crate，CI 只编译它们，绝不全量。
#
#   sh scripts/ci-scope.sh <base-ref> [head-ref]
#
# 输出两行（source 或写进 GITHUB_OUTPUT 都行）：
#   TEST_PKGS="zeo-metadata zeo-db"   —— 有改动的 crate，`cargo test -p` 跑它
#   CHECK_PKGS="zeo-engine zeo-ui …"  —— 传递依赖它的 crate，`cargo check` 防
#                                        API 破坏（check 不产生代码，便宜）
# 任一输出为 "FULL" 表示该跑全量（manifest / CI 本身改动、算不出 base 等）。

set -eu
cd "$(dirname "$0")/.."

base="${1:?usage: ci-scope.sh <base-ref> [head-ref]}"
head="${2:-HEAD}"

if ! git rev-parse --verify "$base" >/dev/null 2>&1; then
    echo "TEST_PKGS=FULL"
    echo "CHECK_PKGS=FULL"
    exit 0
fi

changed="$(git diff --name-only "$base" "$head")"
[ -z "$changed" ] && {
    echo "TEST_PKGS="
    echo "CHECK_PKGS="
    exit 0
}

pkgs=""
full=""
for f in $changed; do
    case "$f" in
        crates/*/src/* | crates/*/Cargo.toml)
            d="${f#crates/}"
            d="${d%%/*}"
            pkgs="$pkgs zeo-$d"
            ;;
        src/* | tests/* | nsh.json | theme.json | resources/*)
            pkgs="$pkgs zeo-player"
            ;;
        # 工作区级文件影响所有 crate：全量（少数情况，不取巧）。
        Cargo.toml | Cargo.lock | .github/* | scripts/* | .cargo/*)
            full=1
            ;;
        *) ;;
    esac
done

if [ -n "$full" ]; then
    echo "TEST_PKGS=FULL"
    echo "CHECK_PKGS=FULL"
    exit 0
fi

pkgs="$(echo "$pkgs" | tr ' ' '\n' | sort -u | tr '\n' ' ' | sed 's/ *$//')"
[ -z "$pkgs" ] && {
    echo "TEST_PKGS="
    echo "CHECK_PKGS="
    exit 0
}

# 反向依赖闭包：改了 A 就要 check 所有传递依赖 A 的 crate（API 破坏它们才
# 显现）。cargo metadata 给 workspace 内的依赖边，python3 做传递闭包。
deps="$(cargo metadata --format-version 1 --no-deps | python3 -c '
import json, sys
meta = json.load(sys.stdin)
names = {p["name"] for p in meta["packages"]}
edges = {}  # dep -> [dependents]
for p in meta["packages"]:
    for d in p["dependencies"]:
        if d["name"] in names and not d.get("kind") == "dev":
            edges.setdefault(d["name"], set()).add(p["name"])
for k in sorted(edges):
    print(k + ":" + " ".join(sorted(edges[k])))
')"

all="$pkgs"
frontier="$pkgs"
while [ -n "$frontier" ]; do
    next=""
    for p in $frontier; do
        revs="$(echo "$deps" | sed -n "s/^$p://p")"
        for r in $revs; do
            case " $all " in
                *" $r "*) ;;
                *)
                    all="$all $r"
                    next="$next $r"
                    ;;
            esac
        done
    done
    frontier="$next"
done

check=""
for p in $all; do
    case " $pkgs " in
        *" $p "*) ;;
        *) check="$check $p" ;;
    esac
done

echo "TEST_PKGS=$pkgs"
echo "CHECK_PKGS=$(echo "$check" | sed 's/^ *//')"
