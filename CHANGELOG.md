# Changelog

## Unreleased

### Changed

- Rewrote `io::IndentWriter` from scratch.
    - Emits far fewer torn writes (lines are written together with their newlines)
    - Uses vectored writes to pair indents with their lines
    - Simpler internal state
    - Now correctly handles inconsistent `write` calls
    - `flush` no longer completes partially-written indents
- Add `#[must_use]` to a handful of non-mutating methods
- Rewrote internals of `fmt::IndentWriter`. It makes fewer write calls (by making fewer strings splits) and has fewer internal branches.

### Internal

- Comment style changes to `fmt.rs`

### Compatibility Notes

This change introduced a handful of behavioral changes that are not observable except in extremely unusual edge cases (where you're changing the content you're writing mid-write). It intentionally changes the behavior of the writer (it now uses `write_vectored`) and also adds `#[must_use]` to a handful of sensible places.

Additionally bumped the MSRV to 1.76

## 2.2.0

### Added

- Added `indentable` module, which contains utilities for indenting objects via `Display`, rather than adapting writers.
- Add `std` feature, which is enabled by default. When disabled, `indent-write` operates in `no_std` mode.

## 2.1.0

### Added

- Added a new constructor, `new_skip_indent`, to `fmt::IndentWriter` and `io::IndentWriter`. This constructor configures the write to omit the indent on the initial line.

## 2.0.0

### Removed

- Removed `IndentedWrite`, which was left over from a previous (unreleased) version of `indent-write` which I simply forgot to delete from the code before releasing

## 1.0.0

Initial release!
