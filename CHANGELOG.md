# Changelog

All notable changes are recorded here.

## [Unreleased]

## [0.1.0] - 2026-08-10

### Added

- Established the first-release API for lazy MDX text entries, MDD binary
  resources, source-bound streaming spans, physical keys, and batched ordinal
  access.
- Added duplicate-aware `locate()` results with `KeyMatches` and `MatchBasis`.
  Lookup searches all raw-exact keys before header-normalized fallback and
  preserves duplicate ordinals in physical order.
- Added reusable `OpenOptions`, fully wired per-open `Limits`, redacted
  `Passcode`, and accounted `MemoryUsage` diagnostics.
- Added UTF-8, UTF-16LE, GBK/GB2312, GB18030, and Big5 decoding; none and zlib
  blocks; optional LZO; keyword-index encryption; and passcode-protected
  keyword-header decryption.

### Safety

- Enforced checked section arithmetic and limit-before-read/reservation policy
  for headers, indexes, block metadata, compressed/decoded blocks, key rows,
  locators, materialized payloads, and aggregate working memory.
- Added fallible reservations, exact record-index validation, bounded zlib/LZO
  output, complete zlib-input consumption, checksum/range/count/terminator
  checks, and within/across-block record-offset validation.
- Kept streaming MDD spans independent of whole-resource materialization limits
  while bounding every decoded block.
- Serialized cache/locator construction, retained deterministic failures to
  prevent retry amplification, and fused public iterators after any error.
- Kept all parser code safe Rust with `unsafe` forbidden.

### Validation And Tooling

- Added an independent synthetic v2 writer and active conformance suites for
  MDX/MDD, every supported encoding and compression path, encrypted sections,
  duplicate lookup semantics, structural corruption, sparse hostile
  declarations, and concurrent first lookup.
- Added seven bounded fuzz targets spanning headers, compression, whole files,
  key indexes/blocks, record indexes, and record spans.
- Added manifest-verified private-corpus tests with exhaustive raw lookup,
  ordinal/payload round trips, deterministic hashes, and no silent skips.
- Added a release benchmark for cold/warm lookup, locator construction, key and
  payload scans, MDD streaming/materialization, concurrency, accounted memory,
  and peak RSS.
- Added Linux/macOS/Windows CI configuration, strict formatting/Clippy/rustdoc
  gates, fuzz build/smoke checks, offline packaging, and extracted-package
  tests.

### Package

- Established a root-only public facade over private shared-core and
  wire-format modules.
- Removed the accidental implicit binary target and restricted packaged files
  to the library, examples, tests, documentation, and license material.
- Released version `0.1.0` as the first public release.

[Unreleased]: https://github.com/lonelam/mdictlib/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/lonelam/mdictlib/tree/v0.1.0
