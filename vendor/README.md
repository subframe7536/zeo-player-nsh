# Vendored NSH platform crates

`nsh-platform-macos` and `nsh-platform-windows` 0.3.0 are the crates.io sources
with one change each: the in-memory cache of decoded images (`CACHE_BYTES` in
`src/image.rs`) is 24 MB instead of 128 MB. Zeo shows hundreds of album covers;
with the stock cap the cache alone reached 128 MB (plus Core Animation's copies
of the same images), keeping the app above 300 MB after scrolling the album grid.

`.cargo/config.toml` redirects both crates here with `[patch.crates-io]`, which
also applies to the runners nsh generates under `.nsh/rust`.

When upgrading nsh, run `sh vendor/update.sh <version>` and update the
`nsh-core` / `nsh-base` versions in `Cargo.toml` to match.
