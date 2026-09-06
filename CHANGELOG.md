# Changelog

All notable changes are recorded here.

## [Unreleased]

## [0.2.6] - 2026-09-07

### Added

- `file://` URLs are accepted wherever a dictionary path is, which is how a
  mobile file picker names a file: iOS answers its document picker with
  `NSURL`s and Android's Storage Access Framework does the same, so an
  application passing that answer through used to fail with a bare "No such
  file or directory". Percent-escaped names decode, `file://localhost/…` is the
  same as `file:///…`, and a Windows drive keeps its letter. A URL naming
  another host is refused with `Error::InvalidData` rather than read as a local
  path. Paths are untouched, including one that merely contains the text.

### Package

- Bumped the package from `0.2.5` to `0.2.6` for the compatible
  `file://` URL input accepted wherever a dictionary path is accepted.

## [0.2.5] - 2026-09-05

### Added

- `ChecksumPolicy::{Skip, Verify}` and `OpenOptions::with_checksum_policy`.
  `Skip` is the default MDict wire-decoding policy for throughput; it retains
  structural, size, range, decompression, and complete-stream validation while
  bypassing optional checksum comparisons. `Verify` restores fail-closed
  checksum mismatch errors. `KeyIndexOptions` exposes the same policy for
  persistent artifacts and defaults to `Skip`, avoiding chunk checksum work
  during index construction unless `Verify` is selected.

- `KeyMatchPage`, `MdxFile::locate_page`, `MddFile::locate_page`, and
  `MdxFile::locate_page_with_key_index`. These additive APIs preserve global
  raw-exact precedence, exact totals, duplicates, and physical order while
  retaining only the requested ordinal window.
- A production persistent MDX key-index API: stable format/parser/normalization
  revision constants, lightweight source metadata identity, bounded and
  cancellable construction to a caller-provided seekable sink or create-new
  path, fixed-cost open, and indexed exact/prefix/physical traversal.
- A file-backed index format with checked 64-bit aligned sections, physical
  normalized text and bounds, normalized-order ordinals, raw-digest filters,
  a checksummed fixed header, lazily paged checksum metadata, and independently
  verified lazy section chunks. Construction uses external merge runs and does
  not instantiate the existing process-lifetime locator.
- Structured `KeyIndexRejection`, cancellation, and observed source-change
  errors. Corrupt, stale, oversized, truncated, or incompatible sidecars remain
  isolated from the readable MDX source.

### Performance

- Persistent-index construction now buffers section/run writes, bypasses
  scratch-run serialization for a one-batch sort, appends normalized keys to a
  shared arena, uses bounded per-run read buffers and allocation reuse during
  external merge, and writes the final merge directly to the ordinal section.
- Construction no longer performs full-source hashing, rereads the completed
  sidecar, or rescans every source row. Final destinations require only
  `Write + Seek`; open reads a fixed 248 bytes before lazy use.

### Security

- Persistent indexes are plaintext normalized-headword derivatives. Every
  readable encrypted MDX follows the ordinary index path without a policy
  option; hosts own storage and lifecycle policy.
- Raw-key digests remain candidate filters only. A positive exact result is
  always rechecked against the current source key block, including collisions.
- Persistent normalized scans verify each visited source key's normalized
  text and raw digest before exposing index text, rejecting same-layout source
  mutation as `SourceKeyMismatch`.
- Lazy reads interpret the exact chunk bytes whose checksums were verified,
  rather than caching a verification flag across a later mutable reread.
  Fixed header bytes, one bounded checksum page, runtime chunk/read buffers,
  and persistent match ordinals are aggregate-memory accounted; pathological
  equal ranges are capped by `Limits::locator_bytes`.
- Source length and modification time are a local freshness stamp, not content
  authentication or cross-path deduplication. Unkeyed checksums detect ordinary
  corruption only; hosts namespace rebuildable caches by stable source location
  plus `KEY_INDEX_REVISION`.

### Package

- Bumped the package from `0.2.4` to `0.2.5` for explicit source and
  persistent-index `ChecksumPolicy` controls, with `Skip` as the default.

## [0.2.3] - 2026-08-13

### Added

- `Limits::large_dictionary()`, a finite, opt-in high-headroom policy for
  unusually large dictionaries.

### Fixed

- Header XML accepts both standard attribute quote styles and both `&#x...;`
  and `&#X...;` hexadecimal entities, rejects odd UTF-16LE lengths before
  decoding, and rejects content after the one top-level header tag.
- Version 1 and 2 MDD headers that omit `KeyCaseSensitive` now use the
  case-sensitive resource-path default used by the sibling `mdx` metadata
  reader; explicit header values still win, and the MDX default remains
  unchanged. Reader-specific MDD sort-key folding remains outside this fix.
- MDD lookup now treats leading separators and `/` versus `\\` as equivalent
  resource-path spelling differences, so callers do not need a compatibility
  retry ladder.
- Restored finite default parser ceilings after the temporary unlimited policy;
  the large-dictionary preset keeps the TLD-sized workload supported without
  making untrusted opens unbounded.
- Complete `GeneratedByEngineVersion` and `RequiredEngineVersion` spellings are
  validated in the parser, future required majors are refused, and incoherent
  version 1-generated/version 2-required headers are rejected. Dispatch still
  uses `GeneratedByEngineVersion` exactly once.

## [0.2.2] - 2026-08-12

### Added

- `MdxFile::prefix_keys`, returning the physical entries whose key starts with a
  prefix under the header's own normalization, in normalized order.
- `MdxFile::scan_normalized_keys`, which lends every entry's normalized key in
  physical order without copying it. Together these let a caller apply its own
  completion or edit-distance policy across the whole key space without building
  and retaining a second copy of it.

### Changed

- The key locator holds normalized key text in one arena with `u32` bounds and a
  single sorted index, in place of a boxed raw *and* normalized string per row
  plus two sorted indexes. On a 4.36-million-entry, 190 MB MDX this cuts the
  locator from 300 MiB to 114 MiB and its build from 3.5 s to 1.9 s.
- `locate` resolves raw-exact matches inside the normalized equal range rather
  than from a second file-wide index: raw equality implies normalized equality,
  so no match can lie outside it. Rows are filtered by a raw-text digest and a
  digest hit is confirmed against its key block, which makes a locate-only hit
  read one key block that it previously did not. A `locate` immediately followed
  by reading the entry — the ordinary case — is unchanged, because that block is
  the one the record read already needed. A locate that misses got faster.

## [0.2.1] - 2026-08-12

### Added

- `Limits::with_unlimited_locator_entries()` for callers that intentionally
  rely only on the locator's internal `u32` row width.

### Changed

- Temporarily changed default parser ceilings to unlimited values. The current
  unreleased changes restore finite defaults and add a separate large-file
  choice.

## [0.2.0] - 2026-08-11

### Added

- MDict major version 1 MDX and MDD support, read through the unchanged public
  API and the same shared core: a 16-byte four-`u32` keyword header, raw
  keyword metadata, one-byte summary lengths without terminators, `u32`
  big-endian key-row record offsets, a four-`u32` record header, and
  eight-byte record-index rows. Every 32-bit field is widened to a checked
  `u64` before it leaves the version 1 grammar.
- A precise refusal for the ISO8859-1 text label, which real version 1.2
  dictionaries declare but whose MDict byte semantics are unresolved. It is not
  silently mapped onto another decoder.
- A precise refusal for version 1 files that declare encryption, for which no
  framing has been established.

### Changed

- Restructured wire-format parsing into `format::common`, `format::v1`, and
  `format::v2`. The version is resolved once, immediately after the bounded
  common header, and matched in exactly one place; both grammars emit the same
  private `ValidatedLayout`. The lazy key-row grammar moved out of the shared
  core behind a statically selected function pointer, so no version conditional
  reaches lookup, iteration, ordinal access, record decoding, or MDD streaming.
  No trait-object dispatch, no runtime conversion between versions, and no
  cross-version grammar retry.
- Unsupported major versions now report `MDict format major version other than
  1 or 2`. Version resolution still keys on `GeneratedByEngineVersion`,
  unchanged from `0.1.0`; `RequiredEngineVersion` is validated independently
  and cannot redirect dispatch.

This restructuring is behavior-preserving for version 2: the public API is
byte-for-byte identical to `v0.1.0`, and version 2 corpus entry counts, key
digests, and payload digests are unchanged.

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

- Added `tests/architecture.rs`, an executable contract asserting that version
  names cannot leak back into the shared core or the MDX/MDD facades, that the
  two grammars cannot see each other, that only the format facade matches on a
  wire version, and that the parsing path uses no trait-object dispatch.
- Added independent version 1 MDX and MDD fixture encoders in
  `tests/support/v1.rs`, physically separate from the version 2 encoder and
  never calling parser code, including an LZO1X encoder that emits real
  lookbehind matches rather than literal-only streams.
- Added `tests/shared_core_parity.rs`, which builds the same logical dictionary
  under both wire versions and runs the same assertions over both.
- Added `tests/conformance_v1.rs` and `tests/hardening_v1.rs` covering
  encodings, block codings, duplicates, cross-block records, empty
  dictionaries, the full malformed matrix, version fallthrough, mutation and
  truncation sweeps, limit enforcement, and deterministic failure replay.
- Added `v1_whole_file`, `v1_truncation`, and `version_dispatch` fuzz targets
  alongside the retained version 2 targets and seeds.
- Added `examples/v1_audit.rs`, `examples/v1_dump.rs`, and
  `scripts/corpus/audit-v1.mjs`, which re-derive the real version 1 corpus
  evidence from the tracked acquisition ledger using two independent
  observations per artifact and retain a structured outcome for every row.
  407 of 453 real v1.2 MDX artifacts complete full validation covering
  43,185,052 entries; all 46 rejections carry a structured classification.
- Added `scripts/corpus/acquire-v1-mdd.mjs`, a bounded opt-in acquisition of
  the MDD candidates paired with version 1 MDX rows. All 16 were acquired under
  an explicitly approved proposal and all 16 passed full resource, span, and
  streaming validation; 14 declare version 1.2 and 77,863 entries.
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

### Package

- Bumped the crate minor version to `0.2.0` for MDict major version 1 support
  behind the unchanged `0.1.0` public API.

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

[Unreleased]: https://github.com/lonelam/mdictlib/compare/v0.2.6...HEAD
[0.2.6]: https://github.com/lonelam/mdictlib/compare/v0.2.5...v0.2.6
[0.2.5]: https://github.com/lonelam/mdictlib/compare/d00d360...v0.2.5
[0.2.4]: https://github.com/lonelam/mdictlib/compare/31a3e9f...d00d360
[0.2.3]: https://github.com/lonelam/mdictlib/compare/bc72444...31a3e9f
[0.2.2]: https://github.com/lonelam/mdictlib/compare/fe22dfd...bc72444
[0.2.1]: https://github.com/lonelam/mdictlib/compare/ad4afaa...fe22dfd
[0.2.0]: https://github.com/lonelam/mdictlib/compare/v0.1.0...ad4afaa
[0.1.0]: https://github.com/lonelam/mdictlib/tree/v0.1.0
