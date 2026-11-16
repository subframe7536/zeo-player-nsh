#!/usr/bin/env bash
# 发版：版本写进 nsh.json（version = 入参，build 自增 1），提交
# 「Release v<ver>」、打 v<ver> tag 并推送（默认推当前分支与该 tag；
# --no-push 只留在本地）。
#
#   sh scripts/release.sh 0.3.0            # 改版本 + 提交 + 打 tag + 推送
#   sh scripts/release.sh 0.3.0 --no-push  # 同上但不推送
#
# 推送 tag 即触发 release.yml：两端打包、生成 latest.json、发布 GitHub
# Release。
#
# Cargo.toml 的版本常驻 0.0.0（pre-alpha 不维护）：应用版本以 nsh.json
# 为准——产物文件名 / 更新清单 / 关于页（zeo_updater 的 app_version /
# app_build）都读它。

set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=""
PUSH=1
for arg in "$@"; do
    case "$arg" in
        --no-push) PUSH=0 ;;
        -h | --help)
            sed -n '2,15p' "$0"
            exit 0
            ;;
        *) VERSION="$arg" ;;
    esac
done
VERSION="${VERSION#v}"

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
    echo "用法: sh scripts/release.sh <semver> [--no-push]（如 0.3.0）" >&2
    exit 1
fi

# 发版提交只含 nsh.json：工作区不干净就没法保证 tag 钉在想要的提交上。
if [ -n "$(git status --porcelain)" ]; then
    echo "工作区不干净：先把改动提交或 stash，再发版" >&2
    exit 1
fi
if git rev-parse -q --verify "refs/tags/v$VERSION" >/dev/null; then
    echo "tag v$VERSION 已存在" >&2
    exit 1
fi

build="$(sed -nE 's/.*"build":[[:space:]]*([0-9]+).*/\1/p' nsh.json)"
build=$(( ${build:-0} + 1 ))

echo "==> nsh.json: version $VERSION, build $build"
sed -i.bak -E \
    -e "s/(\"version\":[[:space:]]*\")[^\"]+/\1$VERSION/" \
    -e "s/(\"build\":[[:space:]]*)[0-9]+/\1$build/" \
    nsh.json
rm -f nsh.json.bak

echo "==> git commit + tag v$VERSION"
git add nsh.json
git commit -m "Release v$VERSION"
git tag "v$VERSION"

if [ "$PUSH" -eq 1 ]; then
    echo "==> git push 当前分支与 v$VERSION（触发 release.yml）"
    git push origin HEAD "v$VERSION"
else
    echo "完成（未推送）。发版：git push origin HEAD v$VERSION"
fi
