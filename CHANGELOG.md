# Changelog

All notable changes are recorded here.

## [Unreleased]

### Fixed

- Accepted a narrowly identified legacy v2 keyword-index layout only when the
  canonical big-endian keyword-header ADLER32 fails and the exact little-endian
  checksum matches; this layout omits summary terminators while retaining all
  count, size, checksum, complete-consumption, decoding, and boundary checks.
- Changed exhaustive duplicate auditing to validate each complete duplicate
  group once and use logarithmic membership checks, preventing repeated
  whole-group scans for large duplicate sets. The audit example now has five
  active unit tests.

### Validation And Tooling

- Added a deterministic, metadata-only inventory workflow for direct MDict
  files, including an aggregate in-flight page-body cap, with a reviewed
  catalog boundary and bounded acquisition into an ignored local corpus cache.
- Bound reviewed selections and complete acquisition outcomes to the exact
  inventory bytes and selected artifact denominator, and made each promoted
  lock/outcome report an independently verifiable tracked pair; hardened
  downloads to public, credential/query-free HTTPS targets with same-origin
  redirects, inactivity timeouts, and absolute deadlines.
- Split binary-identity-pinned metadata-open/count bootstrap observation from
  payload validation and added timeout-bounded, one-artifact-per-process
  exhaustive audits with reverified executable identity and atomic
  catalog/denominator/runner-bound outcome reports. Logical-baseline promotion
  now requires the exact complete-success outcome ledger and audit TSV, and an
  explicit chain verifier re-derives the logical lock from those inputs.
- Added version 2 corpus manifests with optional exact logical key and payload
  SHA-256 baselines while retaining version 1 manifest support.
- Documented corpus provenance, redistribution review, inventory outcome
  reporting, and the split between CI tooling checks and manual full-corpus
  validation.
- Acquired all 1,254 reviewed direct MDX files (40,084,630,153 bytes) into the
  ignored local cache with no acquisition errors; tracked only the verified
  792-file bootstrap lock and the complete 1,254-row acquisition outcomes.
- Recorded the exact 792-artifact exhaustive ledger: 757 whole-artifact passes
  covering 78,368,836 entries and 35 first-error failures. No logical TSV or
  logical-baseline lock was produced because the run was not wholly successful;
  the results remain self-observed regression evidence.

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
