# mdictlib Status

Last updated: 2026-09-05 (v0.2.4 package candidate; v0.2.3 published)

## Current Snapshot

- `mdictlib` `0.2.4` is the current source/package candidate; `0.2.3` remains
  the current crates.io release. The crate supports MDict major versions 1 and
  2 for MDX and MDD.
- Version 1 support is implemented, tested against independent synthetic
  fixtures, fuzzed, and validated against 453 authorized real v1.2 MDX
  artifacts. **407 of 453 complete full validation**; every rejected artifact
  carries a structured retained classification.
- The compatible persistent MDX key-index facility is implemented in the
  `0.2.4` candidate. Publishing, tagging, or pushing it still requires explicit
  maintainer authorization.
- AALookup integration has started against this adjacent `0.2.4` checkout: its
  normal build now compiles the persistent-index API by default, without a
  Cargo feature or build-script environment `cfg` gate. This local path is an
  integration bridge, not a published dependency or release cutover.
- Real v1 MDD is **validated**: 16 approved artifacts were acquired into the
  ignored cache and all 16 passed full validation, 14 of them declaring
  version 1.2.
- The canonical repository is `https://github.com/lonelam/mdictlib`; the
  repository's only release tag is `v0.1.0`, while `0.2.3` is published through
  crates.io.
- Rust is pinned to `1.97.1`; MSRV is `1.97`, edition 2024.
- MDX and MDD, and both wire versions, use one defensive, file-backed parser
  core. The wire version is resolved once during open and never reaches lookup,
  iteration, ordinal access, record decoding, or MDD streaming.
- Header and block indexes are parsed eagerly under limits; key and record
  blocks are decoded lazily.
- Unsafe code is forbidden.
- The version 1 refactor preserved the `v0.1.0` API. Version `0.2.2` later
  added compatible `MdxFile` scan/completion methods, `0.2.3` added the
  compatible `Limits::large_dictionary()` constructor plus compatibility
  fixes, and the `0.2.4` candidate adds persistent MDX indexing without
  changing existing methods.
- Public corpus metadata and acquisition tooling are tracked separately from
  ignored, locally authorized dictionary bytes under `.corpus/`.

## 0.x Compatibility Policy

The `0.1.0` public API is a published contract. Compatible fixes and additive
APIs use patch releases. Intentional breaking public-API changes require a
minor version bump and a changelog entry; local-only predecessor shapes remain
irrelevant. The `0.2.0` version 1 program itself made no public-API change.
Subsequent compatible additions are tracked explicitly in the changelog.

## Architecture

Public root facade:

- `MdxFile`, `MdxEntry`
- `MddFile`, `MddResource`, `MddResourceSpan`
- `KeyEntry`, `KeyOrdinal`, `KeyMatches`, `KeyMatchPage`, `MatchBasis`
- `KeyIndex`, `KeyIndexOptions`, `KeyIndexSourceIdentity`, `KeyIndexBuild`,
  `KeyIndexRejection`, and stable key-index revision constants
- `Header`, `OpenOptions`, `Passcode`, `Limits`, `MemoryUsage`
- `Error`, `Result`

Private implementation:

- `src/core/`: shared open state, caches, memory accounting, lazy key blocks,
  ordinal access, record descriptors and spans, fused iteration, header-driven
  normalization, the lazy duplicate-aware locator, and the bounded external
  persistent-index builder/file-backed accessor. The persistent-index code is
  organized under `src/core/persistent/` by cache, query, build, sort, format,
  and tests responsibilities. **Version-blind.**
- `src/index.rs`: public persistent-index policy, source identity, handle, revision, and
  structured-rejection values; storage/publication policy remains outside the
  crate.
- `src/format/mod.rs`: the bounded common header, the single `WireVersion`
  resolution, and the crate's only version `match`.
- `src/format/common/`: `descriptors.rs` (the `ValidatedLayout` boundary),
  `header.rs`, `cursor.rs`, `checked.rs`, `encoding.rs`, `compression.rs`,
  `checksum.rs`, `crypto.rs` (shared algorithms only), and `source.rs`.
- `src/format/v1/`: `mod.rs`, `keyword.rs`, `record.rs`.
- `src/format/v2/`: `mod.rs`, `keyword.rs`, `record.rs`, `crypto.rs`
  (version 2 encryption framing).
- `src/limits.rs`: budget and policy machinery.

Dependency direction:

```text
mdx.rs / mdd.rs -> core -> format facade -> ValidatedLayout
                                         -> format::common
                                         -> format::v1 | format::v2
```

`format::v1` and `format::v2` import neither the core, the facades, nor each
other. `src/core`, `src/mdx.rs`, and `src/mdd.rs` name no wire version.

The former implicit binary is absent; examples are the only executable targets.
The separate fuzz crate uses narrow doc-hidden adapters only under cargo-fuzz's
checked `cfg(fuzzing)`; the package exposes no fuzz-only Cargo feature.

### How one core serves two wire versions

The version is resolved once, immediately after the bounded common header, and
both grammars emit the same private `ValidatedLayout`: the parsed header,
separate key and record encodings, exact checked `SectionRanges`, the total
entry count, the total decoded record length, `Box<[KeyBlockDescriptor]>`,
`Box<[RecordBlockDescriptor]>`, `WireOperations`, and the retained metadata
reservations.

One difference survives past open: version 1 key rows carry a `u32` record
offset and version 2 rows carry a `u64`. That is resolved by selecting one
private non-capturing function at the same match:

```text
type DecodeKeyRows = fn(&[u8], &KeyRowContext) -> Result<Vec<DecodedKeyRow>>;
struct WireOperations { decode_key_rows: DecodeKeyRows }
```

The core stores `WireOperations` and calls it on a lazy cache miss. There is no
version enum in the core, no per-entry branch, and no trait object.
`tests/architecture.rs` asserts all of this against the source text.

## Implemented Behavior

### Keys, ordinals, and lookup

- `keys()` yields fused `Result<KeyEntry>` rows in physical order.
- `key_at()` and `keys_at()` use the same physical identity; batched access
  preserves caller order, repeats, and out-of-range `None` values.
- `locate()` builds one lazy, budgeted global locator shared by MDX and MDD.
- `locate_page()` on MDX and MDD reports the same global basis, exact total,
  duplicate identity, and order while retaining only the requested ordinals.
- Global raw-exact matches always win; header-normalized lookup occurs only
  after a complete raw miss.
- `KeyMatches` reports `MatchBasis` and every duplicate ordinal in ascending
  physical order.
- Single-result lookup chooses the lowest physical ordinal and then uses direct
  ordinal access.
- Known header attributes are ASCII-case-insensitive, semantically equivalent
  aliases are accepted, and conflicts are rejected.
- Header attributes accept either XML quote style and uppercase or lowercase
  hexadecimal numeric entities. Only one top-level header element is accepted;
  non-whitespace trailing content is rejected, while a matching empty closing
  tag remains compatible.
- When `KeyCaseSensitive` is omitted, supported MDD files default to
  case-sensitive resource paths while MDX retains its historical
  case-insensitive default. Omitted `StripKey` remains disabled for both.
  This follows the sibling `mdx` metadata default; other readers may still
  fold MDD sort keys independently of that metadata, so this is not a claim
  of universal cross-reader sort-key parity.
- `StripKey` removes non-alphanumeric ASCII for comparison while preserving
  non-ASCII characters; case sensitivity remains an independent header flag.
- MDD resource lookup normalizes optional leading separators and `/` versus
  `\\` spelling in the shared core, so callers do not need a retry ladder.
- `GeneratedByEngineVersion` remains the sole grammar-dispatch authority;
  `RequiredEngineVersion` is validated independently for complete numeric
  spelling, supported major range, and impossible v1-generated/v2-required
  combinations.

### MDX

- `entry_at()` resolves and decodes one physical entry.
- `entries()` is lazy and fused after key, descriptor, record, limit, or text
  decode failure.
- `lookup()` returns an ordinal-bearing `MdxEntry`.
- Encoded and worst-case decoded text sizes are preflighted and jointly charged
  before record materialization.

### Persistent MDX key indexes

- `KEY_INDEX_REVISION` aggregates independently exposed format,
  parser/layout, and normalization revisions (`f3-p1-n1` in this candidate).
- `key_index_source_identity()` reads source length and filesystem modification
  time from the already-open `FileSource` without scanning contents, and binds
  those values plus the parsed physical key count. Hosts namespace each local
  cache by stable source location plus `KEY_INDEX_REVISION`; the metadata value
  is only that location's freshness stamp, never a cross-path deduplication key.
- `build_key_index()` and `build_key_index_to_path()` use bounded sort batches,
  buffered scratch-backed external merge runs, and cancellation checkpoints.
  Normalized bytes append to one reusable arena with offset records instead of
  one allocation per key. A one-batch sort writes its order directly; larger
  merges use bounded per-run read buffers, reuse active key allocations, and
  write the final merge directly to the order section. They never initialize
  the existing `OnceLock<KeyLocator>`.
- Final destinations require `Write + Seek`, are streamed once, and are never
  read back. The create-new path builder flushes and syncs but deliberately does
  not publish atomically: callers build at a unique partial path and perform a
  same-filesystem rename. Job leases, quotas, and garbage collection remain
  host responsibilities.
- The format contains magic/endian/revision/header/total-length fields, the
  metadata source identity, checked 64-bit aligned section descriptors, physical
  normalized text and `u64` bounds, physical raw digests, and physical ordinals
  sorted by normalized text then ordinal.
- Open reads a fixed 24-byte prefix and 224-byte header, validates its checksum
  when `KeyIndexOptions::checksum_policy` is `Verify`, and always validates all
  section geometry without reading data sections. Verify-mode expected
  checksums are fetched through one lazy bounded page; exact verified section
  bytes, rather than a verification flag followed by a second read, are
  interpreted. Skip mode does not allocate that checksum page. Those caches
  and transient results remain charged to the originating dictionary memory
  budget.
- Indexed lookup preserves global raw-exact precedence, normalized fallback,
  all duplicate physical ordinals, normalized prefix order, and physical scan
  order. A raw digest only filters candidates; a source key-block read proves
  every positive raw match. Equal-range ordinal materialization is capped by
  `Limits::locator_bytes`, uses one vector for raw filtering and fallback, and
  remains charged to aggregate working memory while `KeyMatches` is alive.
- `locate_page_with_key_index()` charges and retains only the requested
  ordinal window. Each independent call still inspects the complete normalized
  equal range because an out-of-window raw match must suppress normalized
  fallback; sequential hosts should cache bounded windows rather than request
  one row at a time.
- Persistent normalized scans source-verify every visited row's normalized text
  and raw digest before invoking the visitor, so a same-layout source-key
  mutation cannot be returned as stale index text.
- Stale, corrupt, incompatible, malformed, truncated, and source-mismatched
  artifacts return `Error::KeyIndexRejected`; the underlying MDX remains usable.
- Length/mtime identity and unkeyed Adler-32 checksums detect ordinary cache
  staleness or corruption; neither authenticates adversarial replacement.
  Sidecars are disposable local cache data and are rebuilt after rejection.
- Every readable encrypted MDX uses the ordinary persistent-index path. The
  derivative contains plaintext normalized headwords, but mdictlib applies no
  encrypted-source policy gate; storage policy belongs to the host.
  Passcode-protected sources must still open successfully before indexing.
  Persistent MDD indexing remains deferred.

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
- `OpenOptions` also exposes `ChecksumPolicy`; `Skip` is the default for MDict
  wire checksum comparisons, while `Verify` restores mismatch detection.
- Limits cover header XML/attributes, indexes, block metadata, compressed and
  decoded blocks, per-block key counts, materialized records, locator rows and
  bytes, and aggregate working memory.
- `Limits::new()` is finite again after the temporary unlimited policy. The
  explicit `Limits::large_dictionary()` preset is finite and sized from the
  measured 4,362,467-entry TLD sample (about 121 MB retained after indexing),
  while applications opening many files still need an aggregate budget.
- `MemoryUsage` exposes conservative accounted current/peak work plus metadata,
  locator, key-cache, and record-cache estimates.
- Aggregate reservations are returned through RAII and concurrent successful
  locator/cache construction is serialized.
- `Passcode::new()` validates borrowed inputs before cloning, caps the user ID
  at 4096 UTF-8 bytes, uses fallible cloning, and redacts debug output.
- `KeyIndexOptions` separately caps artifact/metadata/chunk sizes and external
  sort memory and selects scratch placement. Build-only sort buffers, scratch
  files, and artifact disk bytes are not steady-state parser heap and are not
  reported through `MemoryUsage`. Once a `KeyIndex` is open, however, its exact
  retained checksum directory and verified chunk cache, plus transient verified
  reads, are charged to the dictionary budget and reflected in `MemoryUsage`
  current/peak.

## Supported And Fixture-Proven Paths

Common to both wire versions:

- MDX and MDD sections
- UTF-8, UTF-16LE, GBK/GB2312, full GB18030, and Big5 decoding
- the shared eight-byte block envelope with big-endian ADLER32
- optional LZO behind `lzo`
- lazy iteration, physical-ordinal access, and duplicate-aware lookup

Version 2 additionally:

- uncompressed and zlib blocks
- keyword-index encryption
- passcode-protected keyword-header encryption
- combined header/index encryption with compressed sections
- a narrowly signaled legacy v2 keyword-index layout: exact little-endian
  keyword-header ADLER32 and omitted summary terminators, accepted only after
  the canonical big-endian checksum fails

Version 1:

- a 16-byte keyword header of four `u32` big-endian fields, with no
  decompressed-size field and no keyword-header checksum
- raw, uncompressed keyword metadata
- one-byte summary lengths counting encoding units, with no terminators
- `u32` big-endian key-row record offsets, widened to checked `u64`
- a 16-byte record header of four `u32` fields
- eight-byte `u32` record-index rows
- uncompressed and LZO key and record blocks

### Explicitly refused, with a precise structured error

| Input | Error |
| --- | --- |
| ISO8859-1 text label (either version) | `Unsupported("ISO8859-1 text encoding (MDict byte semantics unresolved)")` |
| version 1 declaring any encryption bit | `Unsupported("encrypted MDict version 1 keyword sections")` |
| any major version other than 1 or 2 | `Unsupported("MDict format major version other than 1 or 2")` |
| LZO block without the `lzo` feature | `Unsupported("LZO compressed blocks (enable the `lzo` feature)")` |

**Not claimed:** encrypted v1, zlib-v1 creator compatibility (the shared
envelope decodes it, but no authorized artifact was observed using it), and
ISO8859-1. Real v1 MDD support is evidence-backed for the bounded 16-file
sample documented below, but that sample is not a claim about every MDD
producer or extension variant.

Independent full-file fixtures cover every supported encoding, none/zlib/LZO,
both encrypted paths, mixed compression, multiple key/record blocks, duplicate
keys, equal offsets, cross-block records, and source-bound MDD streaming, for
**both** wire versions. The v1 LZO suite includes an encoder that emits real
lookbehind matches, not only literal streams.

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
- Key-block counts are checked before each push, terminators are validated,
  checksum comparisons follow `ChecksumPolicy`, and first/last summaries use
  creator-compatible normalization.
- The legacy v2 keyword-index fallback retains exact header/index checksums,
  count and size sums, full index consumption, decoded text, and block-boundary
  validation; it is not a general permissive parse mode.
- Record starts are nondecreasing within and across key blocks, including
  direct access to interior rows of a later block.
- Streaming span validation is separate from whole-resource materialization.
- Parser iterators yield at most one error and then remain exhausted.

## Verification Snapshot

Local gates on 2026-08-11, after version 1 support landed:

- `cargo fmt --all -- --check`: passed
- `cargo test --locked --all-targets`: 164 passed, 0 failed, 3 ignored
- `cargo test --locked --all-targets --all-features`: 165 passed, 0 failed,
  3 ignored
- `cargo test --locked --all-targets --no-default-features`: 164 passed,
  0 failed, 3 ignored
- `cargo test --locked --test conformance_v2 --no-default-features`: 17 passed
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: passed
- `cargo clippy --locked --all-targets --no-default-features -- -D warnings`:
  passed
- strict rustdoc (`-D warnings -D missing_docs`): passed
- `cargo test --locked --doc --all-features`: passed
- cargo-fuzz 0.13.2 / nightly-2026-08-09 AddressSanitizer build of all ten
  targets: passed
- ten bounded 64-run coverage-guided fuzz smoke campaigns: passed

The three ignored tests are the explicit private-corpus tests, unchanged.

Current compatibility/limit pass on 2026-08-13:

- `cargo fmt --all -- --check`: passed
- `cargo test --locked --all-features`: 59 library tests plus all integration
  suites passed; only the three private-corpus tests remain ignored
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: passed
- the measured 4,362,467-entry TLD sample opened and looked up `apple` in the
  release example (`entries=4,362,467`)

The historical v1 gate counts above remain retained as the independently dated
2026-08-11 evidence snapshot; the current pass adds the compatibility and
finite-limit changes described in this status file.

Persistent-index candidate validation on Windows on 2026-09-04:

- `cargo fmt --all -- --check`: passed
- `cargo test --locked --all-targets` and
  `cargo test --locked --all-targets --all-features`: passed; only the same
  three private-corpus tests were ignored
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: passed
- doctests and rustdoc with warnings, missing docs, and broken links denied:
  passed
- `cargo package --locked --offline --allow-dirty`: packaged and verified the
  `0.2.4` candidate
- `cargo fetch --locked --manifest-path fuzz/Cargo.toml`: passed after updating
  the fuzz lock for the package version and new dependencies; cargo-fuzz was not
  installed on this host, so sanitizer fuzz builds were not rerun
- `node --test scripts/corpus/*.test.mjs scripts/corpus/test/*.test.mjs`: all
  46 tests passed after treating Windows directory-`fsync` `EPERM` as the
  platform's unsupported directory-sync operation and making cargo-build
  outcomes injectable in the cross-platform bootstrap test
- `tests/persistent_index.rs` covers v1/v2 locator equivalence, no implicit
  locator construction, raw/normalized basis, duplicates, prefix and physical
  traversal, forced external merging, empty files, cancellation, observed
  source mutation, length/mtime staleness, lazy checksum rejection, hostile
  geometry, truncation, revision/identity mismatch, a real FNV-1a collision,
  fixed-cost open, writer destinations without `Read`, and readable-encrypted
  sources. Private core regressions prove fixed 248-byte opening for both tiny
  and large checksum directories, streaming Adler equivalence, amortized arena
  growth, and 33 initial sort runs through the 32-way merge-fan-in boundary.

Persistent-index construction follow-up on Windows on 2026-09-05:

- `cargo test --locked --lib`: 63 passed;
  `cargo test --locked --test persistent_index`: 22 passed; and
  `cargo test --locked --test public_api`: 1 passed after the format-2
  metadata-identity/lazy-checksum redesign.
- `cargo test --locked --all-targets --all-features` passed (three private
  corpus tests ignored by design); strict all-target/all-feature clippy,
  rustdoc, and doctests also passed.
- The release `examples/persistent_index_cost` harness used the default 32 MiB
  build-memory and 64 KiB chunk limits. On the 158,987-row `oaldZhEn.mdx`, three
  warmed in-memory locator builds took 40/42/42 ms, create-new persistent builds
  took 58/57/57 ms, and generic writer builds took 59/62/58 ms. The sidecar is
  4,185,324 bytes versus 3,548,878 retained locator bytes.
- On the measured 4,362,467-row TLD sample, three warmed in-memory locator
  builds took 2,012/1,991/1,985 ms, create-new persistent builds took
  2,170/2,114/2,121 ms, and writer builds took 2,113/2,151/2,163 ms. The
  sidecar is 137,027,356 bytes versus 119,568,874 retained locator bytes. The
  large-case persistent cost is now about 6–8% over the in-memory build; the
  remainder is durable section/run I/O, checksum generation, and sync rather
  than source/artifact hashing or full-output verification. These are local
  warm-cache diagnostic observations, not a cross-machine performance promise.
- The AALookup host integration then measured the same release implementation
  three times for each requested source. Median in-memory/persistent construction
  was 40.934/54.710 ms for 158,987-row `oaldZhEn.mdx`, 31.996/46.618 ms for
  137,212-row `辭海第七版.mdx`, and 2,002.529/2,515.845 ms for the 4,362,467-row
  TLD source. Median host warm-open/first-positive-query time was
  0.372/3.541 ms, 0.398/3.601 ms, and 2.582/7.953 ms respectively. The host
  counters include required MDX metadata reads; the isolated sidecar open test
  remains the exact size-independent result at two reads and 248 bytes.

Checksum hot-path follow-up on 2026-09-05:

- Common Adler-32 now reduces its accumulators once per 5,552-byte block,
  matching the existing persistent-index streaming implementation. A boundary
  regression compares the optimized result with the bytewise reference.
- `ChecksumPolicy::Skip` is now the default for MDict header, outer block, and
  zlib inner checksum comparisons. It does not skip size, range, decompression,
  complete-stream, or structural checks. `Verify` restores fail-closed checksum
  mismatch errors. The v2 encrypted keyword-index checksum remains necessary
  as key material, and the small v2 header checksum remains a layout signal.
- Persistent-index checksums use their own `KeyIndexOptions::checksum_policy`,
  also defaulting to `Skip`. `Verify` checks decoded sidecar chunks on first
  use, while repeated access to a cached chunk does not recompute it. The
  persistent index retains one chunk per section, so random cross-chunk access
  can still incur another read and checksum under `Verify`.
- A standalone 512 MiB release microbenchmark measured the old bytewise Adler
  loop at about 854 ms versus about 95 ms after block reduction (same result,
  roughly 9x faster). This is a checksum microbenchmark, not a full-corpus
  performance claim.

Paged-locator follow-up validation:

- `cargo test --locked --all-targets --all-features`, strict clippy, strict
  rustdoc, and doctests passed; only the same three private-corpus tests were
  ignored
- the 4,096-duplicate regression proves the complete-result API can exceed a
  tight match budget while a five-row persistent page succeeds and retains
  exactly its requested ordinal bytes
- AALookup's `npm run mdictlib:check` sibling-candidate seam passed, including
  its bounded ordinal-window reuse and persistent-index budget regressions

### Regression evidence against the pre-v1 baseline

- **Public API at the version 1 cutover**: a source-level comparison of every
  public item reachable from `lib.rs` against the `v0.1.0` worktree was
  **identical** (126 items). The later `0.2.2` scan/completion methods and the
  `0.2.3` large-dictionary preset are deliberate compatible additions. The
  `0.2.4` persistent-index candidate is another additive API and is pinned by
  `tests/public_api.rs`.
- **Version 2 corpus logical facts**: eight v2 artifacts audited with
  `examples/corpus_audit` under both the pre-refactor build (`1b3f6bb`) and the
  current build produced **byte-identical** entry counts, key digests, and
  payload digests.
- **Laziness**: `tests/shared_core_parity.rs` proves opening still decodes no
  key or record block, for both wire versions.

## Version 1 Implementation Status

Version 1 MDX and MDD are implemented in `src/format/v1/`. The grammar is
exactly what section "Supported And Fixture-Proven Paths" lists; nothing
speculative was added.

### Synthetic evidence

`tests/support/v1.rs` is an independent version 1 encoder, physically separate
from the version 2 encoder and never calling parser code. It drives:

- `tests/conformance_v1.rs`: 31 tests covering every block coding, every
  supported encoding, UTF-16 unit-counted summaries, duplicates, equal offsets,
  empty records, cross-block records and resources, multiblock files, empty
  dictionaries, precise ISO8859-1 and encryption refusals, both version
  fallthrough directions, and the malformed matrix (every keyword and record
  header field, v2-shaped index rows, trailing metadata bytes, malformed and
  v2-style terminated summaries, wrong block sizes, checksum mismatches,
  unknown and mismatched compression tags, invalid and decreasing record
  offsets, truncation at every section boundary, and hostile `u32`
  declarations).
- `tests/shared_core_parity.rs`: builds the same logical dictionary under both
  wire versions and runs the same assertions over both.
- `tests/hardening_v1.rs`: single-byte mutation sweeps over whole MDX and MDD
  files, truncation at every offset, per-limit enforcement, hostile `u32`
  headers under a small memory ceiling, deterministic failure replay without
  memory amplification, iterator fusion, and lazy-open proof.

### Real MDX corpus evidence

Re-derived on 2026-08-11 by `scripts/corpus/audit-v1.mjs`, which makes two
independent observations per artifact: a from-scratch geometry probe written in
the driver, and the `examples/v1_audit` worker run in an isolated
timeout-bounded subprocess. Neither informs the other.

- ledger: `corpus/mdict-org-2026-08-10.acquisition-outcomes.json`, SHA-256
  `f19ced0c1b844277689cc723d15a762ea81678d3b4a19ddf5e28fe1af568ea65`
- denominator rule: SHA-256 over sorted `<sha256>\t<bytes>\t<sourcePath>\n`
  records, sorted by source path
- denominator digest (453 artifacts):
  `7b841b9191420684c3f0275007e0087068bbe654454f957d60059ffbefc4f1ed`
- runner: `target/release/examples/v1_audit`, 860,640 bytes, SHA-256
  `3480962d96a124b9db6c6f94a797856c640b9ac9bb2f259856fcfa7e1288fbd9`
- host: darwin/arm64, Node.js v26.5.1, concurrency 4

Independent geometry observation — this **reproduces**, from a reproducible
command, every figure that was previously carried forward without an artifact:

| Fact | Value |
| --- | --- |
| artifacts | 453 (2,677,098,909 bytes) |
| fit the canonical v1 geometry through exact section EOF | **448** |
| declared entries | **46,083,934** |
| key blocks | **71,243** |
| record blocks | **179,587** |
| LZO for every key and record block | **446** |
| one or more uncompressed key blocks with LZO records | **2** |
| zlib blocks or uncompressed record blocks observed | **0** |

The five non-conforming artifacts, retained individually:

| Artifact | Classification |
| --- | --- |
| `b4ff52dd…`, `bab32567…`, `b7e8e533…`, `67e6b948…` | record section declared longer than the file (truncated) |
| `e50c5d5d…` | keyword metadata leaves 262 trailing bytes while record geometry reaches exact EOF — **corruption versus creator variant unresolved**; no fallback was invented to accept it |

Parser observation:

| Outcome | Count | Bytes | Entries |
| --- | --- | --- | --- |
| accepted, fully validated | **407** | 2,383,691,169 | **43,185,052** |
| rejected, structured classification | 46 | — | — |

Every accepted artifact completed: exact declared entry count, ordinal
continuity, `key_at` agreement with sequential iteration, raw lookup for every
distinct key, all duplicate ordinals in ascending physical order, `lookup`
selecting the lowest ordinal, and complete payload hashing.

Rejection classes:

| Category | Count | Note |
| --- | --- | --- |
| `record-decode` | 27 | source text not valid in its declared encoding (22 GBK, 3 UTF-8, 2 BIG5) |
| `unsupported-encoding` | 11 | exactly the eleven ISO8859-1 artifacts |
| `truncated` | 4 | the four truncated record sections, refused at open |
| `key-decode` | 2 | BIG5 key text not decodable |
| `compression-failure` | 1 | an LZO record block that would not decompress |
| `limit-exceeded` | 1 | `e50c5d5d…`; the desynchronized metadata declares 6,946,917 entries in one block, above the 2,000,000 ceiling |

Accepted encodings: UTF-16 204, GBK 144, UTF-8 58, BIG5 1.

**No artifact was accepted whose geometry the independent probe rejected.**
The two observations agree on all 453 rows.

### Differential evidence

Compared against `terasum/js-mdict` at
`044fbf5101bb491942bac1bfffb39778a84cf84a` (AGPL-3.0, JavaScript lineage,
independent of the Python `mdict-analysis` lineage), built from source and run
through its public API. Ten accepted artifacts spanning GBK, BIG5, UTF-16, and
UTF-8, totalling 8,551 entries.

- **Entry counts: 10 of 10 agree exactly.**
- **Payload bytes: every entry in all ten artifacts agrees**, once two reader
  policy differences are accounted for.

The two policy differences, both investigated against the raw bytes rather than
resolved by majority vote:

1. **Trailing record NUL.** `mdictlib` trims a record's trailing NUL
   terminator; js-mdict retains it. This is `mdictlib`'s shipped `0.1.0`
   behavior, unchanged.
2. **Leading U+FEFF in a key.** Three artifacts contain exactly one key stored
   with a byte-order mark. `mdictlib` preserves it, matching its documented
   contract that a key is returned exactly as stored; js-mdict strips it. A
   UTF-8 decoder cannot synthesize a BOM, so the bytes must contain one and
   `mdictlib` is the faithful reader here.

After accounting for those two, **zero unexplained disagreements** remain.

#### Second lineage: `ffreemt/readmdict` (Python)

`lzo` was installed via Homebrew and `python-lzo` built against it, so the
Python lineage was run over the same ten artifacts.

- **Entry counts: 10 of 10 agree** with `mdictlib`.
- **Complete row sets (key plus payload digest) agree on 4 of 10.**
- On the other six, `readmdict` returns the same number of rows and the same
  keys, but a different payload for some of them.

One differing row was examined in full. Both readers return **151 bytes** with
an identical prefix; the middle differs. That artifact has 570 rows and 570
distinct keys, so duplicate-key association cannot explain it. `mdictlib` and
`js-mdict` — two independent lineages — return byte-identical content for that
row, and `readmdict` is the outlier.

**This is recorded as an open item, not as a passed or failed gate.** The root
cause has not been established, and the honest reading is that two independent
readers agree with `mdictlib` and a third does not, on some rows, for reasons
not yet identified.

### Real MDD evidence

Acquisition of the 16 exact-stem MDD candidates was explicitly approved and
executed on 2026-08-11 by `scripts/corpus/acquire-v1-mdd.mjs`, under a 64 MiB
per-file and 128 MiB aggregate ceiling, from the single reviewed origin
`https://mdx.mdict.org` over credential- and query-free HTTPS. All 16 transfers
completed, totalling exactly the advertised 59,842,819 bytes, with zero errors.
The bytes live only under the ignored `.corpus/` cache and are not committed.

Every one of the 16 was then fully validated by `examples/v1_audit`:

| Declared version | Files | Accepted | Entries | Payload bytes |
| --- | --- | --- | --- | --- |
| 1.2 | 14 | **14** | **77,863** | 73,547,408 |
| 2.0 | 2 | 2 | 416 | 243,312 |
| total | 16 | **16** | 78,279 | 73,790,720 |

Each accepted MDD completed exact physical keys and ordinals, duplicate
ordering, `key_at` and `lookup`, `resource_at`, `span_at` and `lookup_span`,
and a byte-for-byte comparison of `copy_to` streaming against `read`
materialization, with resources crossing record blocks exercised throughout.

Two of the 16 turned out to be **version 2** MDD files paired with version 1
MDX files. That is a real-world finding worth recording: a dictionary's MDX and
MDD need not share a wire version, which is exactly why version resolution is
per-file rather than per-dictionary.

**Real v1 MDD support is now evidence-backed**, not inferred from synthetic
fixtures.

## Corpus Discovery, Validation, And Benchmark Evidence

The corpus workflow treats discovery, authorization, acquisition, and parser
evidence as separate gates. A reachable public URL is not evidence of a
redistribution license. Dictionary payloads therefore remain in the ignored
local `.corpus/` cache and are not committed directly or through Git LFS; Git
tracks only reviewable source metadata, lock data, schemas, and tooling.

The AALookup reference generator is a useful candidate-discovery input, but is
intentionally model-driven and non-exhaustive: its prompt asks for a
"reasonable set," caps browsing at 25 steps by default, and explicitly says it
need not visit every file. No generated AALookup draft was present during this
audit, so it is not evidence of a catalog snapshot or completed download.

A deterministic metadata-only live audit of `https://mdx.mdict.org/` on
2026-08-10 traversed 990 same-origin auto-index directories and observed 1,254
direct `.mdx`/`.MDX` links with 40,084,630,153 advertised bytes. Sorting those
rows by absolute URL and hashing each exact
`<decimal-bytes>\t<absolute-URL>\n` record produced listing fingerprint
`cfa8cdc0e3b1579280398a295e45b7b56fb7c5ee856aa138492cbc72e6eac77d`.
This identifies the directory-listing snapshot only; it is not a payload
checksum. The same audit observed 335 direct `.mdd`/`.MDD` links totaling
47,594,522,494 advertised bytes with analogous listing fingerprint
`5bd6e1a9106b128b34770a35232c2a289c47c39c628d68bcdb42d00ec9b3d823`.
Four MDX and two MDD objects exceeded 2 GiB. NFC-normalized,
Unicode-lowercased decoded basenames collided across 59 groups/123 MDX rows
and 25 groups/54 MDD rows, so local paths must preserve more identity than a
basename.
The complete 2,992-file inventory advertised 144,841,177,042 bytes across all
types. These are discovery counts, not claims that the files were licensed,
downloaded, opened, or accepted by the parser.

The corrected inventory metadata is tracked in
[`corpus/mdict-org-2026-08-10.inventory.json`](../corpus/mdict-org-2026-08-10.inventory.json).
The crawl started at
`2026-08-10T04:06:35.081Z`; the exact inventory file SHA-256 is
`51ba61e985e42351ce7a1ee0c7713ac1f9f02284870383a12c3350ad2b5fa74d`.

The ignored MDX selection records the maintainer's private-local-testing review
at `2026-08-10T04:38:54.272Z`. Its canonical file SHA-256 is
`69a59efa4f6876b542191d3d696915168d5b7672c45e8f130ac1d11263857627`, and
its exact 1,254-row source-set SHA-256 is
`7a481fb209fca5661f04ff2d0d6fccf33c58c6f4876bddb671d0f734e923cfc1`.
That records authorization and the denominator, not acquisition or parser
success.

Tracked dependency-free tooling separates each transition:

- `inventory-mdict-index.mjs` and `import-aalookup-catalog.mjs` create
  metadata-only discovery inputs;
- `select-inventory.mjs`, `lock-corpus.mjs`, and `promote-lock.mjs` require an
  explicit local-testing decision, bind the exact inventory bytes and complete
  selected source-path/type/URL/size denominator, bootstrap bounded hash-pinned
  bytes, run a timeout-bounded metadata-open/count observer pinned by binary
  byte count and SHA-256, retain one outcome for every selected row, and bind
  each promoted lock to its independently verifiable complete outcome report;
  and
- `sync.mjs`, `validate.mjs`, and `record-logical-baselines.mjs` reproduce the
  reviewed bytes/manifest, run quick validation or isolated one-artifact
  exhaustive audits with a reverified runner binary, write
  catalog/denominator/runner-bound complete outcome reports, and record
  self-observed logical baselines only from the exact successful ledger/TSV
  pair, with an exact pre-baseline-to-logical-lock chain verifier.

Local post-release tooling checks on 2026-08-10:

- `node --test scripts/corpus/*.test.mjs scripts/corpus/test/*.test.mjs`: 46
  passed, including the committed inventory fingerprint, exact selection and
  promoted-lock/outcome binding, aggregate inventory page-memory bounds,
  fail-closed IP/redirect/deadline policy, response cancellation, resumable
  journals, isolated-runner replacement/stale-output cases, and exact
  logical-baseline chain provenance;
- `cargo test --locked --test local_corpus`: 5 passed, 1 explicitly ignored;
- `cargo test --locked --all-features --test synthetic_lookup --test synthetic_v2`:
  28 passed;
- `cargo test --locked --all-features --example corpus_audit`: 5 passed,
  including the large noncontiguous-duplicate regression;
- the all-target/all-feature Rust suite, formatting, and all-target/all-feature
  Clippy with warnings denied passed.

The finalized acquisition downloaded all 1,254 selected MDX files, exactly
40,084,630,153 bytes, with zero acquisition errors. The bytes remain only under
the ignored `.corpus/` tree because their redistribution licenses are
unverified and the corpus is unsuitable for source Git or Git LFS. The final
ignored bootstrap draft is 6,249,797 bytes with SHA-256
`6cd3d21b14f9c8195fa305e36b1cbf886b23213eeba6b09207cf6487413f2243`.

The tracked acquisition evidence is an exact verified pair:

- [`corpus/catalog.lock.json`](../corpus/catalog.lock.json): 2,197,293 bytes,
  SHA-256
  `d1baaaddc642d926e7f74a33e6d49dc1c302871c5a3dda3de91a872b2c4a4e2d`;
- [`corpus/mdict-org-2026-08-10.acquisition-outcomes.json`](../corpus/mdict-org-2026-08-10.acquisition-outcomes.json):
  3,383,244 bytes, SHA-256
  `f19ced0c1b844277689cc723d15a762ea81678d3b4a19ddf5e28fe1af568ea65`.

Both digests were re-verified on 2026-08-11.

Metadata open/count promoted 792 files totaling 37,377,272,230 bytes and
89,051,220 declared entries. The complete outcome report retains 462
exclusions: 453 non-v2 formats, six keyword-summary decode failures, and three
truncated record sections. The 453 non-v2 rows are the v1 evidence base
described above. The generated 792-row local manifest is 107,469
bytes with SHA-256
`f45f5e02ea5eaf3eecf032048c72db62ce191310978b0eef96258d39790daef1`.
These entry counts were observed by `mdictlib`; they are regression baselines,
not independent evidence.

The full isolated audit used the exact tracked lock, concurrency 2, and a
21,600,000-ms per-artifact timeout. It ran on macOS 26.6 / Darwin 25.6 arm64
(T6020) with rustc/cargo 1.97.1 targeting `aarch64-apple-darwin` and Node.js
26.5.1. The 822,368-byte audit runner had SHA-256
`957d958b23e6ecaf1347246b701d6c557290e36223982c3ef81618c90f3f0a0d`;
the exact 792-artifact denominator digest was
`c6155b5f49101898b1d8da3bae5a9ffa3a08c0c44dca8c622a0c15571084e17a`.

The tracked
[`corpus/mdict-org-2026-08-10.exhaustive-outcomes.json`](../corpus/mdict-org-2026-08-10.exhaustive-outcomes.json)
is 612,424 bytes with SHA-256
`ba3ac714348f07fa2f90762f08878294dd41289d01bf0db17f31ca92dc26835c`.
It records 757 whole-artifact successes covering 27,098,834,819 bytes and
78,368,836 fully traversed entries, plus 35 failures covering
10,278,437,411 bytes and 10,682,384 declared entries. Failed artifacts stopped
at the first recorded bad ordinal, so those 10,682,384 entries were not all
traversed. Failure classes are 17 GBK and ten UTF-8 record-decode failures, two
GBK and one UTF-8 key-decode failures, two zlib stream failures, two zlib
ADLER32 mismatches, and one key-summary boundary mismatch. Because exhaustive
success was incomplete, no logical-audit TSV or L1 logical lock exists.
These are the exact source-data failure classes reported at the strict parser
boundary; follow-up forensics identified no parser change warranted for them.

This run also validated a coherent 213,587-entry real dictionary using the
narrow legacy v2 keyword-index layout. The exhaustive runner's duplicate
verification now validates each full duplicate group once and uses logarithmic
membership checks, avoiding repeated whole-group scans. All corpus results are
`mdictlib`-self-observed regression evidence, not independent correctness proof.

"All direct MDX files" means the 1,254 direct MDX links in that named source
snapshot. It does not mean all dictionaries on the Internet, archived or
script-generated files not linked directly by the index, or a complete result
from AALookup's deliberately sampling generator.

Acquisition is bounded and opt-in from reviewed metadata. Production downloads
require credential/query-free HTTPS URLs, connections pinned to validated
public DNS answers, and same-origin HTTPS redirects; cross-origin targets
require a new explicit review. Inactivity timeouts, absolute attempt deadlines,
file/aggregate byte ceilings, and exact hashes bound each transfer. A complete
corpus run must retain every reviewed inventory row and report non-success
outcomes such as unsupported format/version, required encryption credentials,
corruption/truncation, authorization denial, acquisition failure, or bounded
observer failure rather than silently dropping difficult files. CI exercises
deterministic tooling and synthetic parser fixtures; network- and storage-heavy
full-corpus acquisition and exhaustive validation remain explicit manual or
self-hosted jobs.

Private or otherwise non-redistributable bytes are excluded. The
manifest-driven suites use
`MDICT_CORPUS_DIR/mdictlib-corpus.tsv`, verify normalized relative paths, byte
counts, SHA-256 values, kinds, and physical counts before parsing, and fail with
setup instructions when explicitly invoked without valid assets. Version 1
manifests contain those five identity columns. Version 2 adds optional
`key_sha256` and `payload_sha256` columns. Full validation builds one audit
runner and invokes it once per artifact in an isolated, timeout-bounded
subprocess; it pins and rechecks that executable, checks every physical row and
declared logical hash, and records a complete outcome set bound to the exact
lock bytes, artifact denominator, and runner identity. The exact five-column
logical audit TSV is installed atomically only if every artifact succeeds.
Recording those values back into a lock requires the matching successful
outcome ledger and explicit acknowledgement that they are
`mdictlib`-self-observed rather than independent correctness evidence.

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
`.codex/benchmarks/2026-08-10-macos-arm64.md`. These are the frozen v2
baselines the v1 program must not regress.

## Release Hygiene

- `.github/workflows/ci.yml` exists.
- `CHANGELOG.md` records `0.1.0` through the published `0.2.3` plus the
  unreleased `0.2.4` persistent-index candidate.
- `README.md`, crate rustdoc, examples, public API tests, and package metadata
  describe the same current `0.2.4` source API and distinguish it from the
  published `0.2.3` package.
- `Cargo.toml` has `autobins = false` and a deliberate package include list.
- Private corpus bytes, private manifests, temporary files, benchmark raw
  output, and `draft/` are not packaged.
- Repository-maintenance corpus inventory/schema/lock metadata and Node
  acquisition tooling are also excluded from the runtime library package; the
  packaged README links their canonical GitHub paths.
- The `v0.1.0` tag identifies the exact source used for the first package.

## Release State

- Source: `https://github.com/lonelam/mdictlib`
- Tag: `https://github.com/lonelam/mdictlib/tree/v0.1.0`
- Package: `https://crates.io/crates/mdictlib/0.2.3`

`0.2.3` is published on crates.io. Crate metadata now selects the unreleased
`0.2.4` candidate, and the Git repository currently exposes only the `v0.1.0`
release tag. Creating missing or future tags, publishing another package, and
pushing release state remain explicit maintainer actions.

## Active TODOs

1. Root-cause the `readmdict` payload disagreement on six of ten sampled
   artifacts. Two independent lineages agree with `mdictlib` on those rows, so
   this is an open question about the third reader, but it is unresolved.
2. Decide whether to track the version 1 corpus outcome report the way the
   version 2 ledgers are tracked. The run is reproducible from
   `scripts/corpus/audit-v1.mjs` today, but its report is not committed.
3. Re-run the checked-in benchmark harness on the recorded host to refresh the
   performance baseline; the 2026-08-10 numbers predate this work.
4. Reconcile the missing `v0.2.3` Git tag when a maintainer authorizes that
   release-hygiene action. **Not authorized.**
5. Extend the new Windows OALD/辭海/TLD persistent-index measurements to the
   authorized multi-platform corpus, including cold/warm positional reads and
   host-level handle residency, before assigning cross-machine expectations.
6. Obtain explicit maintainer authorization before publishing, tagging, or
   pushing the `0.2.4` candidate. **Not authorized.**
7. After authorized publication, move AALookup and its dictionary-scale harness
   from the adjacent-checkout bridge to the registry `0.2.4` dependency, update
   their lockfiles and parser-boundary assertion together, and keep the default
   integration free of Cargo-feature and environment-`cfg` gates.

## Known Risks

- Real v1 MDD evidence covers **16 artifacts and 77,863 entries** (14 declare
  version 1.2). That is a much smaller denominator than the MDX evidence, and
  all 16 came from one origin and a narrow candidate rule.
- Differential confirmation rests on two independent lineages. They agree with
  `mdictlib` on entry counts everywhere, but the Python lineage disagrees on
  payload content for six of ten sampled artifacts and that is **unresolved**.
  The GoldenDict, Java, and Rust lineages were not attempted.
- 27 of 453 real v1 artifacts fail record text decoding in their declared
  encoding, and 2 fail key decoding. These look like source-data defects — the
  same class the v2 corpus shows — but they have not been confirmed against a
  second reader.
- Eleven real v1 artifacts declare ISO8859-1 and are refused. Resolving that
  label would make them readable and is a separate scoped decision.
- One artifact (`e50c5d5d…`) remains **corruption versus creator variant
  unresolved**. It is refused; if it is a creator variant, some real files use
  a keyword-metadata shape this grammar does not model.
- Version dispatch remains keyed on `GeneratedByEngineVersion`, exactly as
  `0.1.0` did. `RequiredEngineVersion` is now parsed and checked separately:
  malformed/future requirements and an impossible v1-generated/v2-required
  relationship are refused without allowing the requirement to select a
  grammar. This centralizes the former AALookup preflight policy; unusual
  creator spellings outside the reviewed corpus remain a compatibility risk.
- zlib in a v1 file is decoded by the shared envelope but has never been seen in
  an authorized artifact, so creator compatibility is untested.
- The frozen general parser benchmark baseline predates this work. The new
  Windows OALD/辭海/TLD measurements cover persistent construction, but do not
  replace a multi-platform corpus baseline.
- Persistent-index behavior has synthetic corruption, staleness, duplicate,
  collision, cancellation, and source-mutation coverage, but no checked-in
  cross-platform large-corpus performance baseline yet. The current safe
  positional-read backend intentionally makes no mmap or zero-residency claim.
- Source length and modification time are deliberately a low-cost freshness
  stamp for a host's path-scoped cache namespace. Timestamp spoofing or an
  adversary able to rewrite both sidecar and checksums is outside this local,
  rebuildable-cache trust model; positive rows are still source-verified lazily.
