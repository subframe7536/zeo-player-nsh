#!/bin/sh
# Re-vendor nsh-platform-{macos,windows} at version $1 and re-apply the cache patch.
set -eu
version="$1"
cd "$(dirname "$0")"
for c in nsh-platform-macos nsh-platform-windows; do
  rm -rf "$c"
  curl -sSfL "https://crates.io/api/v1/crates/$c/$version/download" | tar xz
  mv "$c-$version" "$c"
  rm -f "$c/Cargo.lock" "$c/Cargo.toml.orig"
  sed -i.bak 's|^const CACHE_BYTES: usize = 128 << 20;|// ZEO PATCH: 128 MB → 24 MB (see vendor/README.md).\nconst CACHE_BYTES: usize = 24 << 20;|' "$c/src/image.rs"
  rm -f "$c/src/image.rs.bak"
  grep -q "ZEO PATCH" "$c/src/image.rs" || { echo "patch did not apply to $c" >&2; exit 1; }
done
