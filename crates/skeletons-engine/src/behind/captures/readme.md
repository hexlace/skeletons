# Captured crates.io sparse-index files

Byte-for-byte captures of three real crates.io sparse-index entries, laid
out the way the real index is
(<https://doc.rust-lang.org/cargo/reference/registry-index.html#index-files>),
read back verbatim by both `behind/index_entries.rs`'s own unit tests
(`include_bytes!`) and, through `SKELETONS_TEST_ONLY_CRATES_IO_INDEX`, the
`ritual` crate's registry-`behind` tests: where `check` would
otherwise `GET https://index.crates.io/<p>`, a test build reads
`$SKELETONS_TEST_ONLY_CRATES_IO_INDEX/<p>` instead, and
`ritual/tests/check_behind_registry.rs` points that variable at this
directory. Nothing here is hand-written or hand-edited: each file is
exactly what `curl` received.

Captured 2026-09-27, with:

```
curl -sS https://index.crates.io/se/mv/semver -o se/mv/semver
curl -sS https://index.crates.io/ru/st/rustls -o ru/st/rustls
curl -sS https://index.crates.io/to/ml/toml -o to/ml/toml
```

What each capture is used to prove, and the lines that prove it (verified
against the bytes actually captured above):

- **`se/mv/semver`** — a registry-pinned skeleton reporting **behind**: locked at
  `1.0.7`, the newest non-yanked, non-prerelease release captured is
  `1.0.28`. The capture also carries a yanked version above the locked one
  (`1.0.8`, yanked) and two prereleases below it (`1.0.0-rc.1`,
  `1.0.0-rc.2`), neither of which this suite's `behind` scenarios exercise
  directly, but which are real data a correct "newest non-yanked,
  non-prerelease" reader must already skip past correctly.
- **`ru/st/rustls`** — a registry-pinned skeleton reporting **current** despite a
  newer version string existing: locked at `0.23.45`, the newest capture
  entry, `0.24.0-dev.1`, is a prerelease and must not count as newer.
- **`to/ml/toml`** — a registry-pinned skeleton reporting **behind** where both
  the locked and the newer version's build metadata contain a `-`
  (`1.1.5+spec-1.1.0` locked, `1.1.6+spec-1.1.0` newer): proves the reader
  does not mistake build metadata's own `-` for a prerelease marker.
