# mdictlib Status

Last updated: 2026-08-10

## Current Snapshot

- `mdictlib` `0.1.0` is the first public release.
- The canonical repository is `https://github.com/lonelam/mdictlib`; the
  release tag is `v0.1.0` and the crate is published through crates.io.
- Rust is pinned to `1.97.1`; MSRV is `1.97`, edition 2024.
- MDX and MDD major version 2 use one defensive, file-backed parser core.
- Header and block indexes are parsed eagerly under limits; key and record
  blocks are decoded lazily.
- Unsafe code is forbidden.
- Package metadata and `LICENSE` declare MIT.

All implementation milestones and the first-release transition in
`.codex/IMPLEMENTATION_PLAN.md` are complete.

## 0.x Compatibility Policy

The `0.1.0` public API is a published contract. Compatible fixes use patch
releases. Intentional breaking public-API changes require a minor version bump
and a changelog entry; local-only predecessor shapes remain irrelevant.

## Architecture

Public root facade:

- `MdxFile`, `MdxEntry`
- `MddFile`, `MddResource`, `MddResourceSpan`
- `KeyEntry`, `KeyOrdinal`, `KeyMatches`, `MatchBasis`
- `Header`, `OpenOptions`, `Passcode`, `Limits`, `MemoryUsage`
- `Error`, `Result`

Private implementation:

- `src/core/mod.rs`: shared open state, caches, and memory accounting
- `src/core/keys.rs`: lazy key blocks and physical ordinal access
- `src/core/records.rs`: record descriptors, blocks, and span traversal
- `src/core/iter.rs`: fused shared key/record iteration
- `src/core/normalize.rs`: header-controlled lookup normalization
- `src/core/locator.rs`: lazy global duplicate-aware locator
- `src/format/`: headers, indexes, codecs, checksums, compression, and crypto
- `src/source.rs` and `src/limits.rs`: bounded file I/O and policy machinery

The former implicit binary is absent; examples are the only executable targets.
The separate fuzz crate uses narrow doc-hidden adapters only under cargo-fuzz's
checked `cfg(fuzzing)`; the package exposes no fuzz-only Cargo feature.

## Implemented Behavior

### Keys, ordinals, and lookup

- `keys()` yields fused `Result<KeyEntry>` rows in physical order.
- `key_at()` and `keys_at()` use the same physical identity; batched access
  preserves caller order, repeats, and out-of-range `None` values.
- `locate()` builds one lazy, budgeted global locator shared by MDX and MDD.
- Global raw-exact matches always win; header-normalized lookup occurs only
  after a complete raw miss.
- `KeyMatches` reports `MatchBasis` and every duplicate ordinal in ascending
  physical order.
- Single-result lookup chooses the lowest physical ordinal and then uses direct
  ordinal access.
- Known header attributes are ASCII-case-insensitive, semantically equivalent
  aliases are accepted, and conflicts are rejected.
- `StripKey` removes non-alphanumeric ASCII for comparison while preserving
  non-ASCII characters; case sensitivity remains an independent header flag.

### MDX

- `entry_at()` resolves and decodes one physical entry.
- `entries()` is lazy and fused after key, descriptor, record, limit, or text
  decode failure.
- `lookup()` returns an ordinal-bearing `MdxEntry`.
- Encoded and worst-case decoded text sizes are preflighted and jointly charged
  before record materialization.

### MDD

- `resource_at()` and `lookup()` return materialized ordinal-bearing resources.
- `span_at()` and `lookup_span()` return opaque source-bound handles.
- `MddResourceSpan::copy_to()` streams through bounded record blocks and is not
  constrained by the whole-resource materialization limit.
- Spans retain their originating open file, keep offsets private, and survive
  dropping the original `MddFile` handle.
- `resources()` is lazy and fused after payload failures.

### Limits and diagnostics

- `OpenOptions` borrows reusable options and accepts a fully wired `Limits`.
- Limits cover header XML/attributes, indexes, block metadata, compressed and
  decoded blocks, per-block key counts, materialized records, locator rows and
  bytes, and aggregate working memory.
- `MemoryUsage` exposes conservative accounted current/peak work plus metadata,
  locator, key-cache, and record-cache estimates.
- Aggregate reservations are returned through RAII and concurrent successful
  locator/cache construction is serialized.
- `Passcode::new()` validates borrowed inputs before cloning, caps the user ID
  at 4096 UTF-8 bytes, uses fallible cloning, and redacts debug output.

## Supported And Fixture-Proven Paths

- MDX and MDD v2-style sections
- UTF-8, UTF-16LE, GBK/GB2312, full GB18030, and Big5 decoding
- uncompressed and zlib blocks
- optional LZO behind `lzo`
- keyword-index encryption
- passcode-protected keyword-header encryption
- combined header/index encryption with compressed sections

Independent full-file fixtures cover every supported encoding, none/zlib/LZO,
both encrypted paths, mixed compression, multiple key/record blocks, duplicate
keys, equal offsets, cross-block records, and source-bound MDD streaming. The
LZO suite includes every file block class plus a hand-authored lookbehind match,
not only literal streams.

## Defensive Validation

- Header, index, compressed-block, decoded-block, metadata, locator,
  materialization, and aggregate ceilings are enforced before corresponding
  reads/reservations.
- File-derived arithmetic and platform conversions are checked.
- Untrusted-size Vec/String paths use fallible reservations.
- zlib output is exactly bounded and must consume the entire compressed input;
  LZO output is exactly bounded.
- Record-index length is exactly `block_count * 16`; trailing data and invalid
  source ranges are rejected.
- Key-block counts are checked before each push, terminators/checksums are
  validated, and first/last summaries use creator-compatible normalization.
- Record starts are nondecreasing within and across key blocks, including
  direct access to interior rows of a later block.
- Streaming span validation is separate from whole-resource materialization.
- Parser iterators yield at most one error and then remain exhausted.

## Verification Snapshot

Local `0.1.0` release gates on 2026-08-10:

- `cargo test --locked --all-targets`: passed
- `cargo test --locked --all-targets --all-features`: passed
  - 76 active tests passed
  - 3 explicit private-corpus tests ignored by default
- `cargo test --locked --test conformance_v2 --no-default-features`: 12 passed
- `cargo fmt --all -- --check`: passed
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: passed
- strict rustdoc (`-D warnings -D missing_docs` plus link checks): passed
- `cargo test --locked --doc --all-features`: passed
- cargo-fuzz 0.13.2 / nightly-2026-08-09 AddressSanitizer and coverage build:
  passed
- seven bounded 32-run coverage-guided fuzz smoke targets: passed
- offline packaged-crate build/test verification: passed

The committed CI workflow repeats default/all-feature tests on Linux, macOS,
and Windows, and runs formatting, Clippy, strict docs, pinned
AddressSanitizer/coverage-guided fuzz build/smoke, and offline package
verification on Linux. Hosted results are recorded by the canonical GitHub
repository rather than duplicated as static claims here.

## Private Corpus And Benchmark Evidence

Private bytes are excluded. The manifest-driven suites use
`MDICT_CORPUS_DIR/mdictlib-corpus.tsv`, verify normalized relative paths, byte
counts, SHA-256 values, kinds, and physical counts before parsing, and fail with
setup instructions when explicitly invoked without valid assets.

The 2026-08-10 release audit used a private manifest with SHA-256
`f4a61bb746601fae3c46d0cf80f2c49426be8c3cd5414a42d2b0942a3f0672f9`:

- 7 authorized v2 files: 2 MDX and 5 MDD
- 3,605,052,185 total file bytes
- 804,572 physical entries
- every file opened under default limits
- every raw physical key resolved through lookup
- every ordinal round-tripped to the same key and payload/span
- duplicate ordering and record/resource hashes were stable
- all seven deidentified logical key/payload digests were machine-checked when
  the release manifest digest matched

Representative measured results on Apple M2 Pro / macOS 26.6 / Rust 1.97.1:

- 293,877-row MDX locator: 167.659 ms; warm p99 lookup: 0.667 µs
- 160,806-row MDD locator: 103.678 ms; warm p99 lookup: 0.667 µs
- accounted representative peaks: 20.43 MB MDX, 13.84 MB MDD
- representative peak RSS: 50,397,184 bytes MDX, 39,059,456 bytes MDD
- full-corpus process peak: 107,724,800 bytes

Commands, exact route hashes, all seven deidentified corpus hashes, and the 2x
diagnostic regression policy are recorded in
`.codex/benchmarks/2026-08-10-macos-arm64.md`.

## Release Hygiene

- `.github/workflows/ci.yml` exists.
- `CHANGELOG.md` has dated `0.1.0` release notes.
- `README.md`, crate rustdoc, examples, public API tests, and package metadata
  describe the same released behavior.
- `Cargo.toml` has `autobins = false` and a deliberate package include list.
- Private corpus bytes, private manifests, temporary files, benchmark raw
  output, and `draft/` are not packaged.
- The `v0.1.0` tag identifies the exact source used for the first package.

## Release State

- Source: `https://github.com/lonelam/mdictlib`
- Tag: `https://github.com/lonelam/mdictlib/tree/v0.1.0`
- Package: `https://crates.io/crates/mdictlib/0.1.0`

Future releases require a version decision, synchronized changelog and docs,
the same release gates, and explicit maintainer authorization.
