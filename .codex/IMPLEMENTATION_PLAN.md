# mdictlib Implementation Roadmap

Last updated: 2026-09-05 (v0.2.4 package candidate; v0.2.3 published)

## 1. Release State And Active Program

`mdictlib` `0.1.0` is the first public release and supports MDict major version
2 only. Every milestone in the released roadmap (sections 6 and 10) is complete.

The **MDict version 1 compatibility program is implemented**. Milestones 1
through 6 are complete, and its `0.2.0` release decision is historical. Crate
metadata is now at the unreleased **`0.2.4`** persistent-index candidate while
crates.io remains at **`0.2.3`**; the repository still exposes only the
`v0.1.0` tag. Any future publish, tag, or push remains an explicit maintainer
action.

Real version 1 MDD evidence is complete: 16 candidates were acquired under an
explicitly approved bounded proposal and all 16 passed full validation, 14 of
them declaring version 1.2. Publishing, tagging, and pushing remain
explicit maintainer actions governed by the `0.x` compatibility policy in
`AGENTS.md`.

Evidence rules (unchanged, and binding on the v1 program):

- the checked-in source and executable tests are authoritative;
- `draft/` supplied audit observations and test targets, not importable code;
- dictionary corpus bytes are never committed to this source repository,
  including through Git LFS; even separately redistributable assets require an
  explicit artifact-hosting decision;
- discovery metadata is a candidate input until a human review records source,
  authorization, and immutable acquisition facts;
- corpus claims must name a manifest digest, host/toolchain, and exact command;
- performance numbers are baselines on the measured host, not universal SLAs;
- a number carried forward from an earlier session without a tracked artifact is
  labelled as such and must be re-derived before it can gate anything.

### 1.1 Active `0.2.4` persistent-index release sequence

This is the mdictlib portion of AALookup's 2,000-dictionary program. The
application-level roadmap is
[`aalookup/docs/dictionary-scale-plan.md`](../../aalookup/docs/dictionary-scale-plan.md), and the
ownership/cutover boundary is
[`aalookup/docs/mdictlib-integration.md`](../../aalookup/docs/mdictlib-integration.md).

Implemented in this candidate:

1. Additive root APIs expose lightweight source identity, bounded construction,
   fixed-cost lazy index opening, raw/normalized lookup, normalized prefix
   lookup, physical traversal, stable revisions, and structured rejection.
2. Construction reads the same parser-owned source handle, never builds the
   process locator as a prerequisite, spills bounded sort runs, merges with a
   bounded fan-in, checks cancellation, and never rereads final output.
3. The artifact verifies its header and independently checksummed chunks;
   every lazy positional read interprets the exact bytes whose contributing
   chunks were verified. The unkeyed checksums detect corruption rather than
   adversarial replacement. Raw digests are filters only and source bytes prove
   a raw match, so collisions cannot change lookup semantics. The loader
   reads the checksum directory through one bounded lazy page, so opening cost
   does not grow with artifact size.
4. Synthetic v1/v2 differential, duplicate, prefix, cancellation, source-change,
   hostile-geometry, truncation, corruption, revision/identity mismatch, digest-
   collision, readable-encrypted-source indexing, fixed-open-byte, and
   greater-than-32-run merge tests are present. The current Windows package
   candidate passes formatting, all-target/all-feature tests, clippy with
   warnings denied, strict rustdoc/doctests, and offline package verification;
   exact evidence is recorded in `.codex/STATUS.md`.

The handoff deliberately remains split. mdictlib owns parser semantics, the
portable artifact and its validation. AALookup owns source-location-scoped placement,
same-filesystem atomic publication, build locks and durable jobs, disk/job/handle
budgets, live leases, garbage collection, runtime pools, UI state, and fallback.

AALookup integration has now started against the adjacent `0.2.4` checkout.
Its normal build compiles the persistent-index API by default, with no Cargo
feature or build-script environment `cfg` gate. This path dependency is a local
integration bridge only: the production dependency cutover still requires an
authorized `0.2.4` publication and matching registry updates in AALookup and
its dictionary-scale harness.

Release gates, in order:

1. Benchmark construction, cold/warm open and positional reads,
   retained memory, and handle residency on an authorized large corpus. Record
   host, toolchain, immutable manifest identity, command, repetitions, and raw
   output; do not turn one host's observation into a universal SLA.
2. Resolve or explicitly accept the outstanding release-hygiene and corpus risks
   in `.codex/STATUS.md`. No known ambiguity may be relaxed merely to publish the
   index API.
3. Obtain explicit maintainer authorization before publishing, tagging, or
   pushing `0.2.4`.
4. After authorized publication, switch AALookup and its dictionary-scale
   harness from the adjacent-checkout bridge to registry `0.2.4`; update their
   lockfiles and parser-boundary version assertion together. The default API
   integration must remain free of Cargo-feature and environment-`cfg` gates,
   and `npm run mdictlib:check` must remain green through the cutover.
5. Run AALookup's cold/warm 2,000-source, cancellation, memory/page-cache/handle,
   replacement-source, and Windows/macOS rendering acceptance gates before
   claiming the end-user scale target.

## 2. Goal And Supported Scope

### Shipped in `0.1.0`

A defensive, library-first reader for MDict major version 2 `.mdx` and `.mdd`:

- one shared, file-backed parser for MDX and MDD;
- eager bounded metadata/index parsing with lazy key and record blocks;
- physical `KeyOrdinal` identity for every key, including duplicates;
- deterministic global raw-exact lookup before normalized fallback;
- duplicate-aware match sets in ascending physical order;
- key-only iteration and batched ordinal access;
- decoded MDX entries;
- materialized or source-bound streaming MDD resources;
- per-open limits, aggregate working-memory accounting, and diagnostics;
- a small root-only public API with private wire-format internals.

Supported text encodings are UTF-8, UTF-16LE, GBK/GB2312, GB18030, and Big5.
Supported block compression is none and zlib, plus LZO with the `lzo` feature.
Keyword-index encryption and passcode-protected keyword headers are covered by
independently generated end-to-end fixtures.

One coherent legacy v2 keyword-index layout is also supported internally. The
parser tries the canonical big-endian keyword-header ADLER32 first and selects
the legacy layout only when that check fails but the exact little-endian value
matches; only that layout omits the first/last-summary terminators. Count and
size sums, decoded-summary validity, complete index consumption, key-block
checksums, and decoded boundary comparisons remain mandatory. This is a private
wire-format decision inside the v2 grammar and does not change the public API.
It is a checksum-byte-order disambiguation within one major version, **not** a
cross-version grammar retry, and section 8's no-retry rule does not forbid it.

### Delivered by the v1 program

MDict major version 1 `.mdx` and `.mdd` behind the same public API, the same
shared core, and the same limits. Adding v1 did not change the public API.

### Added in the `0.2.4` candidate

A production persistent MDX key-index facility behind additive APIs. mdictlib
owns normalization, index revisions and bytes, bounded construction, source
identity, lookup semantics, and validation. The host owns source-location-scoped
placement, scratch/disk preflight, build scheduling, atomic publication,
artifact leases, quotas, and garbage collection.

The facility must not construct the existing full locator as a prerequisite,
must remain version-blind below `ValidatedLayout`, and must preserve the exact
`RawExact`/`HeaderNormalized`, duplicate, prefix, and physical traversal
contracts. It uses safe bounded positional reads; mmap is not introduced.

### Still out of scope

Future-major layouts, writing, HTML/style processing, multi-volume discovery,
library-owned fuzzy ranking/correction, resource extraction policy, mmap, and
persistent MDD indexes. Normalized prefix lookup and early-break physical key
traversal are implemented; completion and edit-distance policy remain with the
caller.

## 3. Architecture As Implemented

```text
public root API
  ├── MdxFile / MdxEntry
  ├── MddFile / MddResource / MddResourceSpan
  ├── KeyEntry / KeyOrdinal / KeyMatches / KeyMatchPage / MatchBasis
  ├── KeyIndex / KeyIndexOptions / source identity, build, rejection, revisions
  ├── Header / OpenOptions / Passcode / Limits / MemoryUsage
  └── Error / Result
            │
            ▼
src/core/            version-blind shared core
  ├── mod.rs       open state, caches, memory accounting
  ├── keys.rs      lazy key blocks and ordinal resolution
  ├── records.rs   record descriptors, lazy blocks, span traversal
  ├── iter.rs      shared fused physical iterators
  ├── normalize.rs header-controlled key normalization
  ├── locator.rs   lazy global duplicate-aware locator
  └── persistent.rs bounded builder and lazy file-backed index access
            │
            ▼
src/format/mod.rs    common header, WireVersion, the only version match
            │
            ▼
      ValidatedLayout
       /           \
src/format/v1/   src/format/v2/
  mod.rs           mod.rs
  keyword.rs       keyword.rs
  record.rs        record.rs
                   crypto.rs

src/format/common/   descriptors, header, cursor, checked, encoding,
                     compression, checksum, crypto, source
private cross-cutting module: limits.rs
public index values and policy: index.rs
```

Dependency rules, all enforced by `tests/architecture.rs`:

- `mdx.rs` and `mdd.rs` are payload-specific facades only;
- `src/core`, `src/mdx.rs`, and `src/mdd.rs` name no wire version;
- `format::v1` and `format::v2` import neither the core, the facades, nor each
  other, and only the facade names `WireVersion`;
- the parsing path uses no trait-object dispatch and no version conversion;
- MDX and MDD share key, locator, record-descriptor, cache, and budget paths;
- normalization is lookup policy, not text encoding;
- persistent MDX indexing consumes only the version-blind `MdictFile` and
  `ValidatedLayout`; it does not introduce a second grammar or a version branch;
- concrete iterators, parser/index types, codecs, sources, and caches stay
  private;
- the separate fuzz crate reaches only narrow doc-hidden adapters under
  cargo-fuzz's checked `cfg(fuzzing)`; no fuzz-only Cargo feature is published.

### The four version-2 leaks that were removed

| Former site | Resolution |
| --- | --- |
| `MdictFile::open()` checking `Header::is_v2()` | replaced by `format::open_layout`, which resolves `WireVersion` once |
| `parse_key_index()` repeating the check and owning the v2 keyword grammar | moved verbatim to `format::v2::keyword` |
| `parse_record_index()` repeating the check and owning the v2 record grammar | moved verbatim to `format::v2::record` |
| `core::keys::parse_key_block_entries()` hard-coding a `u64` record offset | moved to `format::v2::keyword::decode_key_rows` and reached through `WireOperations` |

`Header::is_v2()` was removed. Version resolution now reads the major component
of `GeneratedByEngineVersion` inside `format::mod.rs`, preserving the shipped
`0.1.0` behavior exactly; section 9 keeps changing that attribute a gated
decision.

## 4. Version Dispatch, As Built

### 4.1 One decision point

The wire version is detected exactly once, immediately after the bounded common
header is parsed and before any keyword or record bytes are read:

```text
common header
    -> one private WireVersion decision
    -> exactly one concrete match:
         V1 => format::v1::parse(...)
         V2 => format::v2::parse(...)
    -> shared ValidatedLayout
    -> unchanged shared core
```

`WireVersion` is a private enum in `src/format/mod.rs`. `src/format/mod.rs` is
the only file in the crate that contains a `match` on it.

The generated version is resolved before `RequiredEngineVersion` is examined,
so an unsupported generated grammar takes precedence over malformed
compatibility metadata. Every dot-separated component of both attributes must
be a non-empty ASCII decimal integer; only the generated major component
selects the grammar. Each `parse_layout` receives the same `LayoutRequest`, and
any grammar error propagates without trying another version.

### 4.2 Physical module layout

```text
src/format/
  mod.rs                    common-header entry point and the only version match
  common/
    mod.rs
    descriptors.rs          validated version-neutral descriptors
    header.rs               (moved from src/format/header.rs)
    cursor.rs               (moved)
    checked.rs              checked widening/arithmetic used by both grammars
    encoding.rs             (moved)
    compression.rs          (moved; shared 8-byte block envelope)
    checksum.rs             (moved)
    crypto.rs               only proven shared algorithms/framing
    source.rs               (moved from src/source.rs)
  v1/
    mod.rs                  parse(): keyword + record sections -> ValidatedLayout
    keyword.rs              v1 keyword header, key metadata, key-row decoder
    record.rs               v1 record header and record index
  v2/
    mod.rs                  parse(): keyword + record sections -> ValidatedLayout
    keyword.rs              current v2 grammar + the narrow legacy-v2 variant
    record.rs               current v2 record grammar
    crypto.rs               only if keyword-header/index framing proves v2-specific
```

Moves are mechanical; `src/source.rs` becomes `src/format/common/source.rs` and
its only consumers are `core/mod.rs` and the format layer, so the move does not
touch `MddResourceSpan`'s retained `Arc<FileSource>` semantics.

### 4.3 Dependency rules

- `format::v1` and `format::v2` must not import `core`, `mdx`, `mdd`, or each
  other. They may import `format::common`, `crate::error`, `crate::limits`, and
  `crate::types` (`Header`, `Limits`, `OpenOptions`, `Passcode`,
  `ContainerKind`).
- `src/core`, `src/mdx.rs`, and `src/mdd.rs` must not name `WireVersion`, `v1`,
  or `v2` in any identifier, import, string, or error message.
- No version check may appear in lookup, iteration, ordinal access, record
  decoding, or MDD streaming.
- One shared MDX/MDD parsing core. No separate v1 MDX and v1 MDD cores.
- No trait-object dispatch anywhere on this path.
- No runtime conversion of v1 bytes into v2 bytes, in memory or on disk.
- Lazy key and record decoding is preserved exactly.

These are enforceable mechanically; section 9 turns them into gates.

### 4.4 `ValidatedLayout`: the descriptor boundary

`format::common::descriptors` owns every type that crosses from a grammar module
into the core. Nothing version-specific crosses it.

```rust
pub(crate) struct ValidatedLayout {
    pub(crate) header: Header,
    pub(crate) key_encoding: TextEncoding,
    pub(crate) record_encoding: Option<TextEncoding>,
    pub(crate) sections: SectionRanges,
    pub(crate) total_entries: u64,
    pub(crate) total_decoded_record_len: u64,
    pub(crate) key_blocks: Vec<KeyBlockDescriptor>,
    pub(crate) record_blocks: Vec<RecordBlockDescriptor>,
    pub(crate) wire: WireOps,
    pub(crate) retained: RetainedBudget,
}
```

- `key_encoding` / `record_encoding` are **separate**. Today one
  `key_encoding` field serves both key decoding and MDX record-text decoding
  ([`src/core/mod.rs:316`](../src/core/mod.rs:316) feeds
  [`src/mdx.rs:219`](../src/mdx.rs:219)). Splitting them is a behavioral no-op
  for v2 — MDX sets both to the header encoding; MDD sets `key_encoding` to
  UTF-16LE and `record_encoding` to `None` because MDD payloads stay bytes — and
  it is a precondition for stating MDD's v1 key/payload asymmetry without a
  container-kind fork in the core.
- `SectionRanges` holds exact, already-`ensure_range`-checked
  `(offset, len)` pairs for the header, keyword header, keyword index, keyword
  block data, record header, record index, and record block data.
- `total_entries` and `total_decoded_record_len` replace today's
  `KeyIndex::num_entries` / `RecordIndex::total_decompressed_len`. The
  key-versus-record entry-count cross-check
  ([`src/core/mod.rs:235`](../src/core/mod.rs:235)) moves into
  `format::mod.rs` so the core receives one reconciled count.
- `KeyBlockDescriptor` and `RecordBlockDescriptor` are today's `KeyBlockInfo` and
  `RecordBlockInfo` with cumulative `u64` offsets, cumulative `u64` entry-start
  indexes, decoded `String` summaries, and `u64` sizes, unchanged in shape.
- `RetainedBudget` carries the metadata `retained_bytes` figures and the
  `MemoryReservation` values that `MdictFile` holds today, so accounting and
  `MemoryUsage` output are unchanged.

### 4.5 Selected static lazy wire operations

v1 key rows carry a `u32` big-endian record offset; v2 rows carry a `u64`. That
difference is the only version-specific decision that survives past open time,
and it is resolved by selecting a private non-capturing function at the single
match:

```rust
pub(crate) type DecodeKeyRows =
    fn(&[u8], &KeyRowContext) -> Result<Vec<DecodedKeyRow>>;

pub(crate) struct WireOps {
    pub(crate) decode_key_rows: DecodeKeyRows,
    // add further static fns only if block-envelope parsing proves
    // version-specific; same pattern, never a trait object.
}
```

- `DecodedKeyRow { key: String, record_start: u64 }` lives in
  `format::common::descriptors`; the core's `DecodedKeyEntry` becomes an alias
  or a direct re-use of it.
- `KeyRowContext` carries what the decoder needs without touching the core:
  key encoding, expected entry count, and `total_decoded_record_len` for the
  offset bound. It deliberately carries **no** limits or memory budget: the
  block's entry count is already bounded when the keyword metadata is parsed,
  and the decoded block's size is charged by the caller before the decoder
  runs, so passing them would be unused surface.
- The core stores `WireOps` and calls `(self.wire.decode_key_rows)(bytes, &ctx)`
  without knowing the version. The monotonicity checks — non-decreasing
  `record_start` within a block and across block boundaries — stay in the core,
  because they are the same invariant for both grammars.
- `format::v2::keyword::decode_key_rows` is today's
  `core::keys::parse_key_block_entries` moved verbatim.
  `format::v1::keyword::decode_key_rows` reads a `u32` and widens it.

This is concrete function-pointer dispatch: monomorphic, non-capturing, no
`dyn`, no vtable, and no per-row branch.

### 4.6 Widening and validation rules for `format::v1`

- Every `u32` read by `format::v1` is widened to `u64` with an explicit
  `u64::from`, and every derived value is checked, **before** it leaves
  `format::v1`. No unchecked `as` casts.
- All section arithmetic, cumulative counts, ranges, limits, and `usize`
  conversions are validated inside the grammar module; descriptors reaching the
  core are already valid.
- The existing limit names and error variants are reused unchanged, so v1
  rejections are indistinguishable in kind from v2 rejections at the public API.
- v1 reuses `format::common::compression::decode_block`, so the shared
  eight-byte block envelope (four-byte tag, big-endian ADLER32, payload) and its
  exact-length and checksum rules apply identically. If a v1 file proves to use
  a different envelope, that is a finding for section 8.3, not a fallback.

## 5. Public API Contract (unchanged at the v1 cutover)

### Physical identity

`keys()` yields owned `KeyEntry` values in physical order. `key_at()` returns
the same type. `keys_at()` preserves caller order, repeated ordinals, and
out-of-range `None` values. Ordinals refer only to the same unchanged file
snapshot.

### Lookup

`locate()` returns an opaque, non-empty `KeyMatches` with a `MatchBasis`:

1. the entire global raw-exact range is searched first;
2. header-normalized lookup occurs only after a global raw miss;
3. every duplicate ordinal is retained in ascending physical order;
4. `lookup()` and `lookup_span()` select the lowest matching ordinal and then
   use the same ordinal route as direct access.

The additive `MdxFile::locate_page()` and `MddFile::locate_page()` APIs accept
an offset and limit and return `KeyMatchPage`: global basis and exact total are
preserved, but only the requested ascending physical ordinals are retained.
An offset at or beyond the total returns an empty `Some` page rather than a
query miss.

The in-memory locator retains normalized text in one shared arena, plus row
bounds, normalized ordering, and raw-text digests. It does not retain a second
raw-text copy per row. Raw equality implies normalized equality, so all raw
candidates lie inside the normalized equal range. Digests filter that range;
source key blocks prove each digest hit. The text arena uses amortized growth to
avoid reallocating all preceding keys for every appended key. Paging resolves
raw-exact precedence across the entire range before returning a window.

Known logical header attributes are resolved ASCII-case-insensitively while
raw spellings remain inspectable. Semantically equivalent aliases are accepted
and conflicting aliases are rejected. Both XML attribute quote styles and
both cases of hexadecimal numeric entities are accepted, but content after the
one top-level header element is rejected; a matching empty closing tag remains
compatible. When `KeyCaseSensitive` is omitted,
supported MDD files default to case-sensitive resource paths while MDX keeps
its historical case-insensitive default; explicit values override either
default. This follows the sibling `mdx` metadata default; reader-specific MDD
sort-key folding remains a separate compatibility question. When `StripKey` is
enabled, comparison removes non-alphanumeric ASCII
code points and leaves non-ASCII characters unchanged; case folding remains
controlled separately by the header.
MDD lookup also treats an optional leading separator and `/` versus `\\` as
equivalent resource-path spelling, keeping that compatibility behavior in the
shared normalizer rather than in an application adapter.
`GeneratedByEngineVersion` remains the sole grammar-dispatch authority;
`RequiredEngineVersion` is validated independently for complete numeric
spelling, supported major range, and the v1-generated/v2-required conflict,
but never selects a grammar.

### MDX

```text
open / open_with_options / header / len / is_empty / memory_usage
keys / key_at / keys_at
locate / lookup / entry_at / entries
```

Every `MdxEntry` contains its physical key row and decoded text.

### Persistent MDX key index

The additive root API is:

```text
KeyIndexOptions / KeyIndexSourceIdentity / KeyIndexBuild / KeyIndex
KeyIndexRejection
KEY_INDEX_FORMAT_REVISION / KEY_INDEX_PARSER_REVISION
KEY_INDEX_NORMALIZATION_REVISION / KEY_INDEX_REVISION

MdxFile::key_index_source_identity
MdxFile::build_key_index / build_key_index_to_path
MdxFile::open_key_index
MdxFile::locate_with_key_index / locate_page_with_key_index
MdxFile::prefix_keys_with_index
MdxFile::scan_normalized_keys_with_index
```

`KEY_INDEX_REVISION` is a filesystem-safe aggregate of independent format,
parser/layout, and normalization revisions. `KeyIndexSourceIdentity` contains
source length, filesystem modification time, and physical key count read from
the already-open parser `FileSource` without scanning contents. The host must
namespace local artifacts by stable source location plus this revision and use
the metadata identity only as that location's freshness stamp. It is neither a
content hash nor a cross-path deduplication identity.

The binary format is little-endian and contains:

- fixed magic, endian marker, format revision, header length, and total length;
- parser/layout and normalization revisions plus checksum chunk length;
- the metadata source identity and normalized-text length;
- four checked, eight-byte-aligned `u64` section descriptors;
- normalized UTF-8 text and `u64` bounds in physical row order;
- one `u32` raw-text digest per physical row;
- one `u32` physical ordinal per row sorted by normalized text then ordinal;
- a header checksum and a contiguous checksum directory for every
  section chunk.

The normal loader reads a 24-byte prefix plus the fixed 224-byte header,
validates the metadata identity and complete checked section geometry, and
keeps one file handle plus bounded caches. It does not eagerly read the
checksum directory or any data section. The expected checksum is loaded through
one lazy 4 KiB page; every row is interpreted from the exact verified section
bytes and validates local bounds/UTF-8. Runtime sidecar reads are serialized per
index. Header, checksum-page, chunk, and returned-byte memory is charged to the
originating dictionary budget. Adler-32 reduces its accumulators once per
5,552-byte block rather than once per byte; this preserves the format checksum
while avoiding a modulo operation in the key/record decode hot path. The
bounded section cache still permits a random query to re-read and re-check a
chunk after another chunk replaces it.

Construction streams physical text/bounds/digests through bounded scratch
write buffers and appends normalized bytes to one arena with offset records.
A one-batch sort writes order directly. External merge uses bounded per-run read
buffers, reuses popped key allocations, and writes its final pass directly to
order instead of serializing and rereading one last run. The final destination
requires `Write + Seek` and is written without any readback. The path API uses
create-new semantics, flushes, and syncs; the host builds a unique partial in
the destination filesystem and performs its own atomic rename. A final metadata
check detects an observed source change. No builder call initializes the old
process-lifetime `KeyLocator`.

Exact lookup binary-searches the normalized-order ordinal section, expands the
complete equal range, tests raw digests only as filters, and reads source key
blocks to prove every candidate. A raw collision can add a source probe but can
never establish equality. With no raw hit, every normalized candidate is also
checked against the source. The equal-range ordinal set is materialized only
once, bounded by `Limits::locator_bytes`, and charged to aggregate working
memory for the lifetime of `KeyMatches`. Prefix positives receive the same
source-row check; physical scan reads physical text order directly. All duplicates
retain ascending physical ordinal within an equal normalized range.

Paged lookup performs the same complete-range raw classification and source
checks but retains and charges only `min(limit, total - offset)` ordinals. The
complete scan per independent call is the tight correctness boundary: without
it, a raw-exact row outside the requested window could be overlooked and a
normalized page returned under the wrong basis. Hosts doing sequential display
pagination should cache a bounded page. Persistent physical scan verifies the
current source key's normalized text and raw digest before each visitor call;
same-layout source mutation is a structured `SourceKeyMismatch`.

Malformed, truncated, corrupt, stale, incompatible, or source-mismatched
artifacts report `Error::KeyIndexRejected(KeyIndexRejection)` without changing
MDX readability. Chunk checksums are unkeyed corruption detection, not
authentication; source metadata can likewise be spoofed. The artifact is a
local rebuildable cache, while every positive or visited result is lazily
checked against its source row. Every readable encrypted MDX follows the
ordinary index path even though the
derivative exposes plaintext headwords; mdictlib has no policy gate and the
host owns storage policy. Passcode-protected sources must open before indexing.
MDD persistent indexing remains evidence-gated.

### MDD

```text
open / open_with_options / header / len / is_empty / memory_usage
keys / key_at / keys_at
locate / lookup / lookup_span / resource_at / span_at / resources
```

`MddResource` is materialized. `MddResourceSpan` is opaque and source-bound;
it retains the originating open file and exposes `read()` and `copy_to()`.
Streaming is independent of the whole-resource materialization ceiling while
each decoded block remains bounded.

### Policy and metadata

- `OpenOptions` is reusable by reference and owns a `Limits` policy;
- all public limit builders are wired to parser boundaries;
- `Limits::new()` is finite; `Limits::large_dictionary()` is an explicit,
  finite high-headroom preset measured against the 4,362,467-entry TLD sample;
  aggregate multi-file budgets remain an application responsibility;
- `MemoryUsage` reports conservative accounted current/peak, metadata,
  locator, and cache estimates;
- `Passcode::new()` validates borrowed input before cloning, bounds the user
  identity, uses fallible allocation, and redacts `Debug`;
- `Header` exposes semantic getters plus exact raw attribute iteration;
- public payload debug output reports lengths rather than dumping content;
- `KeyIndexOptions` separately bounds artifact/metadata/chunk sizes and external
  sort memory and selects scratch placement. Build-only sort buffers, scratch
  files, artifact disk bytes, and host-wide handle/job budgets remain outside
  per-open parser `MemoryUsage`; an open `KeyIndex`'s checksum-page cache,
  verified chunk cache, and transient reads are included in its
  dictionary's accounted current/peak memory.

## 6. Defensive Invariants

Every file-derived count, length, offset, range, decoded size, reservation, and
section sum is checked before the corresponding read or allocation. The shipped
release enforces:

- the header ceiling before reading its declared body;
- checked `u64`/`usize` conversions and checked add/multiply throughout;
- compressed, decompressed, index, metadata, locator, materialization, and
  aggregate per-open ceilings;
- fallible `Vec`/`String` reservations on untrusted-size paths;
- bounded zlib output with full compressed-input consumption;
- bounded optional LZO output;
- exact record-index length and complete section/range validation;
- key-block count plausibility, checksums, terminators, and creator-compatible
  normalized summary checks;
- fail-closed canonical/legacy keyword-index layout detection with no general
  checksum, summary-terminator, or boundary relaxation;
- nondecreasing record offsets within and across key blocks;
- lazy descriptor/span validation separate from materialization limits;
- serialized cache misses and locator construction so successful concurrent
  first access performs one retained build;
- persistent-index checked header/section arithmetic, bounded external-sort
  batches and merge fan-in, shared normalized-key arenas, create-new path
  output, fixed-cost open, lazy checksum-page and section-chunk verification,
  source-row validation on use, and structured rejection;
- same-handle length/mtime observation before and after construction, with host
  source-location namespacing and no claim of content authentication;
- iterator error fusion after descriptor or payload failures;
- no `unsafe` code.

The v1 program adds no relaxation. It adds section 4.6's widening rules and
requires that every v1-specific rejection reuse an existing `Error` variant.

## 7. Completed v2 Milestones And Exit Evidence

| Milestone | Status | Exit evidence |
| --- | --- | --- |
| A. Architecture and API foundation | complete | Root-only API, private layers, one shared core, opaque fused iterators, ordinal-bearing values, source-bound spans |
| 0. Independent executable evidence | complete | Independent v2 writer, ADLER32, RIPEMD128, encryption, zlib and literal LZO fixture paths; active synthetic lookup/structure suites; explicit manifest failures instead of silent skips |
| 1. Trust-boundary hardening | complete | Limits precede reads/reservations; exact section math; bounded codecs; aggregate budget; structured corruption and sparse hostile-declaration tests |
| 2. Ordinal-to-record validation | complete | Duplicate/equal-offset/cross-block fixtures plus 804,572-row private-corpus ordinal and payload audits |
| 3. Lookup semantics and normalization | complete | Raw-before-normalized, aliases/conflicts, author-profile StripKey, non-ASCII controls, five-block collisions, nonmonotonic summaries, and duplicate order tests |
| 4. Complete shared locator | complete | Lazy budgeted global locator shared by MDX/MDD; exhaustive corpus raw queries and duplicate ordinals pass |
| 5. Generic-v2 conformance | complete | Full synthetic MDX/MDD files for all encodings, none/zlib/LZO, both encryption modes, corruption, tight limits, concurrent first lookup, seven fuzz targets, multi-OS CI definition |
| 6. Performance and release audit | complete | Versioned private-manifest evidence, locator/warm lookup/full scan/ordinal/stream/materialize/concurrency/RSS measurements, docs and package gates |
| 7. Persistent MDX key index | implementation complete; release pending | Additive `0.2.4` API, path-scoped metadata identity, bounded cancellable arena/direct/external sort, write-only final publication input, fixed-cost open, lazily checksummed positional access, O(page) retained match windows, source-verified positives/scans, synthetic equivalence/corruption/staleness/collision/large-duplicate tests, and 158,987-row, 137,212-row, plus 4,362,467-row Windows construction measurements; cross-platform large-corpus performance evidence remains pending |

The active all-feature suite contains complete synthetic dictionaries rather
than only primitive parser tests. LZO evidence includes whole files covering
every block class and a hand-authored lookbehind-match stream. The encrypted
fixtures independently implement the required cryptographic transformations.
All seven fuzz targets build with AddressSanitizer and libFuzzer coverage under
pinned cargo-fuzz 0.13.2/nightly-2026-08-09; bounded 32-run campaigns pass.

The private release corpus evidence is recorded in
`.codex/benchmarks/2026-08-10-macos-arm64.md`: seven authorized files, two MDX
and five MDD, totaling 3,605,052,185 bytes and 804,572 physical entries. Every
raw key resolved, every ordinal/payload route agreed, and streaming/materialized
MDD hashes matched. Private file bytes and identifying paths are excluded.

## 8. V1 Compatibility Program

### 8.1 Clean-room and no-conversion rules

These rules are absolute and apply to every milestone below.

1. **No copying from copyleft sources.** Do not copy code, pseudocode,
   identifier names, module structure, comment text, or control flow from any
   GPL- or AGPL-licensed reference into `mdictlib`. Behavioral observation only:
   what bytes go in, what values come out, which inputs are rejected.
2. **Permissive sources are still not a shortcut.** MIT-licensed references may
   be run as oracles and read, but any actual reuse of their text requires an
   explicit, recorded attribution decision before it happens. Default to
   independent derivation.
3. **Ports and repacks are not independent confirmation.** Two projects in the
   same lineage agreeing about a field width is one observation, not two.
   Section 8.2 records lineage for exactly this reason.
4. **Writers are oracles, not specifications.** A writer's output proves a
   decoder accepts what that writer emits. It never proves real creators emit
   the same bytes. Any writer-derived expectation must be corroborated by an
   authorized real artifact before it can gate acceptance.
5. **No runtime conversion.** v1 bytes are never rewritten into a v2 shape, in
   memory or on disk, to reuse the v2 grammar.
6. **No cross-version grammar retry.** A file that fails the v1 grammar is never
   re-attempted as v2, or the reverse. The version decision is made once, from
   the header, and a grammar failure is a failure. (The existing legacy-v2
   keyword-index checksum disambiguation in section 2 is inside one major
   version and is unaffected.)
7. **No permissive parsing.** No brute-force parsing, no grammar retries, no
   silent checksum bypasses, no heuristic encoding changes, no lossy fallback.
   An unresolved artifact stays classified as unresolved.
8. **Conversion and writer tools stay outside the library.** Any converter or
   writer built during this program is a separately reviewed diagnostic or test
   oracle. It is never linked into the published crate and never called by
   parser code.

### 8.2 Source, license, and provenance matrix

> Historical planning record: sections 8.2 through 8.4 preserve the dated pre-implementation v1
> evidence and milestone instructions. Their future tense and “none/not reproduced/not exercised”
> labels describe that baseline, not current status. Current outcomes are recorded in sections 8.5
> and 8.6 and in `.codex/STATUS.md`; all seven milestones completed, including validation of 16 real
> v1 MDD artifacts.

Lineage classes: **A** = Python `mdict-analysis` family, **B** = GoldenDict C++
family, **C** = JavaScript family, **D** = Java, **E** = Rust, **W** = writer.
Independence means "different lineage class", not "different repository".

#### Cloned and pinned locally (outside this repository)

| Project | Revision | Local path | License facts | Lineage | Claimed MDX/MDD | Use |
| --- | --- | --- | --- | --- | --- | --- |
| `ffreemt/readmdict` | `d9c53dc1515f99ae5a2d6b1d89212c9b680e0c59` | `../mdict-v1-references/readmdict` | `pyproject` metadata says MIT, but the copied parser retains a GPLv3 source header — **treat as GPLv3** | A (repack of `csarron/mdict-analysis`) | MDX + MDD | clean-room behavioral only; **not** independent confirmation |
| `goldendict/goldendict` | `b4f3fcdc4861975ec49ae2bb894f907c072fc8f6` | `../mdict-v1-references/goldendict` | GPLv3-or-later | B | MDX + MDD | clean-room behavioral only |
| `binhetech/mdict-parser` | `257885176aa572953b044e9ff68b88fecc86cdf9` | `../mdict-v1-references/mdict-parser` | `readmdict.py` carries a GPLv3 header; appears to be another `readmdict` copy | A | MDX + MDD | clean-room behavioral only; **not** independent confirmation |
| `lengyijun/mdict-cli-rs` | `1df631794440d2837fbfb9f81e47d85077074386` | `../mdict-v1-references/mdict-cli-rs` | **no `LICENSE` file in tree and no README license statement** | none — contains no parser; `src/mdict_wrapper.rs` delegates to the crates.io `mdict` 0.1 crate | n/a | **excluded.** Not a reader lineage. Present locally but unplanned; do not use for behavioral research until its license and the `mdict` crate's lineage are separately reviewed |

#### Original milestone 1 clone list (completed)

| Project | Revision | License facts | Lineage | Claimed MDX/MDD | Use |
| --- | --- | --- | --- | --- | --- |
| `csarron/mdict-analysis` | `e99cfca7a969cc8020d6f92d76c254e075f4110a` | GPLv3 parser header | A (root of the lineage) | MDX + MDD | clean-room behavioral only |
| `terasum/js-mdict` | `044fbf5101bb491942bac1bfffb39778a84cf84a` | current source AGPL-3.0 | C | MDX + MDD | clean-room behavioral only |
| `zhansliu/writemdict` | `f0240b30cabd2f0470d3ee1a0641fc7f8c38dcf5` | MIT | W | explicit v1.2 MDX/MDD **writer** | reviewed synthetic/diagnostic oracle; **never** the sole expected-result source (rule 8.1.4) |
| `ikey4u/wikit` | `f1acb7ae6e75f910ded2f757ee4e9a22df52b87e` | MIT | E | MDX with explicit v1 paths; **no MDD** | oracle plus permissively-licensed cross-check; rule 8.1.2 applies |
| `xiaoyifang/goldendict-ng` | `9054c3436fc4d7c29ea1a9901fa28ad79362b7ef` | GPLv3+ | B | shared MDX/MDD parser | clean-room behavioral only; same lineage as `goldendict` |
| `mdict4j` (Codeberg) | `eed1a78b51081684720f1fa50beddfec5f5dd4b6` | GPL-3.0-or-later | D | v1 MDX, but **forces MDD through v2** | clean-room behavioral only; its MDD path is evidence of a *reader's* choice, not of the format |
| `jeka-kiselyov/mdict` | `77fda616cb9aa0afe06f814c78ecfb4d8fd6a994` | MIT parser text, but **bundled LZO code is GPL** | C | MDX + MDD | not independent of `js-mdict`; treat the bundled LZO as GPL-contaminated and do not read it |

For each row, milestone 1 records: the immutable revision, the exact
source-file and license-file URLs at that revision, the lineage class and the
evidence for it, the claimed MDX and MDD support, and the observation method
used (source reading versus running the tool against a fixture).

Independence budget: the strongest available pairing is **A ∪ B**, optionally
strengthened by **D** or **E**. `readmdict` + `mdict-parser` + `mdict-analysis`
together count as **one** observation.

### 8.3 Evidence versus hypotheses

#### Independently reproduced in this repository on 2026-08-11

Reproduced from tracked artifacts, by bounded metadata-only reads. Method:
filter [`corpus/mdict-org-2026-08-10.acquisition-outcomes.json`](../corpus/mdict-org-2026-08-10.acquisition-outcomes.json)
(SHA-256 `f19ced0c1b844277689cc723d15a762ea81678d3b4a19ddf5e28fe1af568ea65`,
verified) to `status == "excluded"` rows whose observer error is
`Unsupported("MDict format major version other than 2")`, then read only each
file's four-byte big-endian length prefix and declared UTF-16LE header XML from
the ignored local `.corpus/` cache.

| Fact | Value |
| --- | --- |
| non-v2 MDX rows in the tracked ledger | 453 |
| total bytes | 2,677,098,909 |
| all 453 locally present under `.corpus/` | yes |
| header-read failures | 0 |
| top-level tag | `Dictionary` × 453 |
| `GeneratedByEngineVersion` | `1.2` × 453 |
| `RequiredEngineVersion` | `1.2` × 453 |
| `Encrypted` | `No` × 453 |
| `Format` | `Html` × 453 |

Encoding strata (count / bytes):

| Encoding | Files | Bytes |
| --- | --- | --- |
| UTF-16 | 207 | 1,519,004,266 |
| GBK | 167 | 478,677,660 |
| UTF-8 | 62 | 558,412,728 |
| ISO8859-1 | 11 | 80,572,016 |
| BIG5 | 6 | 40,432,239 |

MDD discovery, reproduced from
[`corpus/mdict-org-2026-08-10.inventory.json`](../corpus/mdict-org-2026-08-10.inventory.json)
(SHA-256 `51ba61e985e42351ce7a1ee0c7713ac1f9f02284870383a12c3350ad2b5fa74d`,
verified):

| Fact | Value |
| --- | --- |
| direct MDD rows in the snapshot | 335 |
| advertised bytes | 47,594,522,494 |
| exact-stem MDD candidates paired with a v1 MDX row | 16 |
| advertised bytes of those 16 | 59,842,819 |
| authorized, acquired, or version-inspected at this baseline | **none**; 16 were later acquired and validated (section 8.5) |

#### Historical carry-forward state (subsequently re-derived)

| Claim | Status |
| --- | --- |
| derived v1 subset identity `e73f7e7697062cf4316ffe40af570af135a4a526858823e407bf3e07955d904b` | **not reproduced.** No derivation rule is recorded anywhere in the repository and fourteen plausible record formulations over the 453 rows all produced different digests. Milestone 1 must pin an exact rule, emit a tracked artifact, and re-derive the value |
| 448 of 453 fit the canonical v1 geometry through exact section EOF | carried forward from a prior session; no tracked artifact |
| 46,083,934 declared entries; 71,243 key blocks; 179,587 record blocks | carried forward; no tracked artifact |
| 446 files use LZO for every key and record block | carried forward; no tracked artifact |
| 2 files contain one or more uncompressed key blocks with LZO records | carried forward; no tracked artifact |
| no zlib block and no uncompressed record block observed | carried forward; no tracked artifact |
| 4 files have coherent headers/indexes but truncated record sections | carried forward; retained as a failure class |
| 1 file's keyword metadata stops fitting the canonical grammar while its record geometry reaches exact EOF | carried forward; stays classified **"corruption versus creator variant unresolved"**. Do not invent a fallback to accept it |

None of the above has decoded a single LZO payload, key, checksum, ordinal,
duplicate, or record. All of it is geometry.

#### Historical hypotheses (subsequently exercised)

At this baseline each remained a hypothesis until independently exercised by synthetic fixtures
(milestone 3) **and** authorized real files (milestone 6). Section 8.6 records the resulting verdicts.

- v1 numeric fields are unsigned 32-bit big-endian; v2 uses 64-bit.
- the v1 keyword header is 16 bytes / four `u32` fields: key-block count, entry
  count, raw key-info size, key-block-data size.
- v1 omits the v2 key-info decompressed-size field and the keyword-header
  checksum.
- v1 keyword metadata is raw/uncompressed.
- each key metadata row is: `u32` entry count; `u8` first-summary length plus
  bytes; `u8` last-summary length plus bytes; `u32` compressed size; `u32`
  decompressed size.
- v1 summaries have no v2-style terminators.
- UTF-16 summary lengths count 16-bit units; other encodings count bytes.
- decoded key rows are a `u32` big-endian record offset followed by a
  NUL-terminated key.
- the v1 record header is four `u32` fields.
- record-index rows are `u32` compressed/decompressed-size pairs, eight bytes
  each — so the exact-length check is `block_count * 8`, against v2's
  `block_count * 16`.
- normal v1 key/record blocks retain the common eight-byte envelope: four-byte
  compression tag, big-endian ADLER32, then payload.
- compression tag `01 00 00 00` is raw LZO1X.
- MDX and MDD share the physical layout; MDD keys are UTF-16LE while payloads
  remain bytes.

#### Open disagreements and unknowns

1. Missing `Encoding` defaults: readers disagree between UTF-8 and UTF-16LE.
   `mdictlib` currently defaults MDX to UTF-8
   ([`src/format/common/encoding.rs:21`](../src/format/common/encoding.rs:21)).
   This remains a separately scoped compatibility decision because changing it
   changes key and record decoding for files that omit the attribute.
2. Real versions other than 1.2. All 453 local v1 files declare exactly 1.2, so
   the local corpus provides **no** evidence about 1.0 or 1.1.
3. Empty v1 sections (zero key blocks, zero record blocks, zero entries).
4. v1 encryption flags and passcode behavior. All 453 local files declare
   `Encrypted=No`, so the local corpus provides **no** v1 encryption evidence.
5. Whether key-info encryption exists in real v1 files at all.
6. Real v1 zlib blocks.
7. Uncompressed v1 record blocks.
8. Exact LZO termination, checksum, and trailing-input rules.
9. Records crossing decoded record-block boundaries in v1.
10. ISO-8859-1 behavior — 11 local files declare it, and it is **not** in
   `mdictlib`'s supported encoding set today
    ([`src/format/common/encoding.rs:8`](../src/format/common/encoding.rs:8)), so those
   files would be rejected as `Unsupported("text encoding")` even with a
   working v1 grammar. Adding it is a separate scoped decision.
11. Creator-specific variants and malformed historical files.

### 8.4 Original milestones (completed)

The imperative text below is retained as the implementation record. Section 8.5 is the current
completion ledger.

#### Milestone 1 — Evidence matrix

Research and tracking only; no source change.

- Clone and pin every section 8.2 row that is not yet local, at the exact stated
  revision. Record for each: revision, source-file and license-file URLs at that
  revision, lineage class and its evidence, claimed MDX/MDD support, and
  observation method.
- Resolve the `mdict-cli-rs` anomaly: it is present locally, was not part of the
  planned reference set, has no license file, and contains no parser. Either
  review it properly or remove it from the reference directory.
- Compare at least two genuinely independent readers (lineage A versus B, ideally
  plus D or E) on every section 8.3 hypothesis. Record agreement, disagreement,
  and silence separately. Silence is not agreement.
- Pin an exact, reproducible derivation rule for the v1 subset identity, emit it
  as a tracked artifact, and re-derive the digest under that rule.
- Re-derive and track the physical geometry figures currently carried forward
  without an artifact (448/453, entry/block counts, compression strata,
  truncation and grammar-mismatch failure classes), with the exact command,
  host, and toolchain.
- Classify all 453 MDX rows into: canonical-geometry candidate, truncated,
  unresolved-grammar, or unsupported-encoding.
- Establish an authorized v1 **MDD** denominator. The 16 exact-stem candidates
  are discovery metadata only; a separate explicit local-testing and license
  review must precede any acquisition.

Exit: every section 8.3 hypothesis carries at least one independent-pair verdict
or an explicit "no independent evidence" marker; every 453 row has a class; the
MDD denominator is either authorized and recorded or explicitly recorded as
still absent.

#### Milestone 2 — Isolate v2

- Create `src/format/common/`, `src/format/v1/`, `src/format/v2/` per section 4.2
  and move the shared modules, including `src/source.rs`.
- Move the entire v2 keyword grammar out of `src/format/key_index.rs` into
  `format::v2::keyword`, including the narrow legacy-v2 variant, and the v2
  record grammar out of `src/format/record_index.rs` into `format::v2::record`.
- Move `core::keys::parse_key_block_entries` verbatim into
  `format::v2::keyword::decode_key_rows` and reach it through `WireOps`.
- Introduce `format::common::descriptors`, `ValidatedLayout`, `SectionRanges`,
  `WireOps`, `RetainedBudget`, and the split `key_encoding` /
  `record_encoding` fields.
- Delete the duplicated `is_v2()` checks in the two index parsers and leave one
  private `WireVersion` match in `src/format/mod.rs`. At this milestone the `V1`
  arm returns the existing `Unsupported` error, so behavior is unchanged.
- Re-point `src/fuzzing.rs` and the fuzz targets at the moved module paths.

Exit: every existing v2 test, corpus hash, limit, lazy behavior, and benchmark
is unchanged (section 9), and the version-1 cutover public API diff against
`v0.1.0` is empty. Later additive patch-release APIs are tracked separately.

#### Milestone 3 — Independent synthetic v1 fixtures

- Build independent v1 MDX **and** MDD fixture encoders alongside the existing
  v2 `FixtureBuilder` in `tests/support/`.
- Writer code must not call parser code, and the parser must not call writer
  code. Checksums and any cryptography in the fixtures are implemented
  independently, as the v2 fixtures already do.
- Cover: empty and multiblock sections; duplicate keys; equal record offsets;
  records crossing decoded record-block boundaries; `none` and LZO compression;
  every encoding the v1 corpus declares; corruption; truncation; and hostile
  declarations (counts, sizes, and offsets that overflow or exceed the file).
- Cross-check a subset against `zhansliu/writemdict` output as a diagnostic
  oracle, under rule 8.1.4.

Exit: v1 fixtures exist for every hypothesis in section 8.3 and every failure
class in milestone 1, and each one has a recorded expected outcome derived
without running `mdictlib`.

#### Milestone 4 — Implement `format::v1`

- Implement only grammar confirmed by milestone 1 and exercised by milestone 3.
- Widen every checked `u32` into `u64` internal descriptors per section 4.6.
- Feed the unchanged shared core through `ValidatedLayout`.
- Anything unconfirmed stays rejected with an existing `Error` variant. No
  speculative acceptance.

Exit: the shared-core behavioral test suite passes against synthetic v1 and v2
with the same assertions.

#### Milestone 5 — Safety and fuzzing

Cover: version detection; field widths; arithmetic overflow; summary parsing;
key and record indexes; LZO; truncation; checksums; limits; whole MDX and MDD
files; and version fallthrough (a v1-declaring file with v2 bytes and the
reverse must both fail cleanly and must not retry the other grammar).

Exit: malformed input cannot panic or bypass limits, under AddressSanitizer,
with the pinned cargo-fuzz toolchain.

#### Milestone 6 — Corpus and differential validation

- Attempt every authorized artifact and retain every outcome, success or not.
- For accepted artifacts, compare physical keys, record offsets, entry counts,
  duplicate order, raw lookup results, and complete MDX payload hashes or MDD
  streamed-span hashes.
- Where an independent reader can be run on the same authorized artifact, record
  a differential verdict; where it cannot, say so.

Exit: every accepted artifact has full ordinal, raw-lookup, duplicate, and
payload/span evidence; every rejected artifact has a structured retained
classification.

#### Milestone 7 — Documentation and release decision

Update `README.md`, `.codex/STATUS.md`, this roadmap, and `CHANGELOG.md` only
after the section 9 gates pass. Select a compatible `0.x` release version
separately. Do not publish, tag, or push without explicit maintainer
authorization.

### 8.5 Milestone outcomes

| Milestone | Status | Evidence |
| --- | --- | --- |
| 1. Evidence matrix | complete | `scripts/corpus/audit-v1.mjs` reproduces the geometry, block totals, compression strata, and all five failure classifications from a reproducible command. The subset-identity digest was re-derived under a rule that is now recorded in the tool itself. `lengyijun/mdict-cli-rs` was confirmed to contain no parser and is excluded. |
| 2. Isolate v2 | complete | `format::common` / `format::v1` / `format::v2`; one `WireVersion` match; `ValidatedLayout` boundary; `tests/architecture.rs` enforces it. Public API identical to `v0.1.0`; eight v2 corpus artifacts produce byte-identical logical facts against the pre-refactor build. |
| 3. Synthetic v1 fixtures | complete | `tests/support/v1.rs`, independent of parser code, including an LZO encoder emitting real lookbehind matches. Drives `conformance_v1`, `shared_core_parity`, `hardening_v1`. |
| 4. Implement `format::v1` | complete | Only evidence-backed grammar. Encryption and ISO8859-1 refused precisely. |
| 5. Safety and fuzzing | complete | Ten fuzz targets under AddressSanitizer, including `v1_whole_file`, `v1_truncation`, and `version_dispatch`; mutation and truncation sweeps over whole v1 MDX and MDD files. |
| 6. Corpus and differential validation | complete | 407 of 453 real artifacts fully validated; 46 rejected with retained classifications; two independent observations agree on all 453. Differential against one independent lineage with zero unexplained disagreements. |
| 7. MDD validation, documentation, release decision | MDD and docs complete; the historical `0.2.0` decision was followed by compatible `0.2.1`/`0.2.2`/`0.2.3` releases | README, STATUS, this roadmap, `CHANGELOG.md`, and crate rustdoc are synchronized; future releases remain subject to maintainer authorization. |

### 8.6 What was learned that changed the plan

- The **derived subset identity carried in the previous plan was not
  reproducible** and has been replaced by a digest computed under a rule the
  tooling records: SHA-256 over sorted `<sha256>\t<bytes>\t<sourcePath>\n`
  records. The current value is
  `7b841b9191420684c3f0275007e0087068bbe654454f957d60059ffbefc4f1ed`.
- Every hypothesis in section 8.3 that the corpus could exercise was
  **confirmed**: four-`u32` keyword header, raw keyword metadata, one-byte
  unit-counted summary lengths without terminators, `u32` key-row offsets, the
  four-`u32` record header, eight-byte record-index rows, and the shared
  eight-byte envelope with big-endian ADLER32.
- The key-row grammar was additionally confirmed **against raw bytes** using
  the two artifacts that store key blocks uncompressed, so it does not rest on
  the LZO decoder being correct.
- `KeyRowContext` does **not** need the limits or memory budget: the block's
  entry count is already bounded at parse time and its decoded size is charged
  by the caller. The struct was trimmed accordingly.
- Splitting `key_encoding` from `record_encoding` was a behavioral no-op for
  version 2, as predicted, and is what lets MDD express UTF-16LE keys with
  opaque payloads without a container-kind fork in the core.

## 9. Required Regression And Exit Gates

All gates below were run on 2026-08-11. Their status is recorded inline.

### Structural gates (mechanically checkable)

All are enforced by `tests/architecture.rs` and **pass**.

| Gate | Check |
| --- | --- |
| one dispatch point | `WireVersion` appears in exactly one file, `src/format/mod.rs`, and is matched exactly once |
| no version leakage into the core | no occurrence of `WireVersion`, `v1`, or `v2` as an identifier, import, or message in `src/core/`, `src/mdx.rs`, `src/mdd.rs` |
| grammar isolation | `format::v1` and `format::v2` contain no import of `crate::core`, `crate::mdx`, `crate::mdd`, or each other |
| no version branch on hot paths | lookup, iteration, ordinal access, record decoding, and MDD streaming contain no version conditional |
| no dynamic dispatch | no `dyn` and no trait object on the open or decode path |
| one shared core | one `MdictFile`; no v1-specific MDX or MDD core |
| no conversion | no code path rewrites v1 bytes into a v2 shape |
| no cross-version retry | a grammar failure never re-enters the other grammar |
| laziness preserved | key and record blocks are still decoded on demand, with the same cache behavior |
| no `unsafe` | unchanged |

### Behavioral gates

| Gate | Status |
| --- | --- |
| existing v2 tests pass unchanged (default, all-features, no-default-features, `conformance_v2 --no-default-features`) | pass |
| existing v2 corpus logical facts unchanged | pass — eight artifacts byte-identical against the `1b3f6bb` build |
| limits and `MemoryUsage` unchanged for v2 | pass |
| the same shared-core tests run against synthetic v1 and v2 | pass — `tests/shared_core_parity.rs` |
| every accepted v1 artifact completes ordinal, raw-lookup, duplicate, and payload validation | pass — 407 of 453 |
| every rejected artifact has a structured retained classification | pass — 46 of 46 |
| malformed input cannot panic or bypass limits | pass — mutation and truncation sweeps plus ten AddressSanitizer fuzz targets |
| public API diff at the version 1 cutover is empty | pass — 126 items identical; later `0.2.2` scan/completion methods, the `0.2.3` large-limit preset, and the `0.2.4` persistent-index candidate are compatible additions |
| benchmark baseline within its 2x diagnostic threshold | **not re-measured**; the checked-in baseline predates this work |
| real v1 MDD validation | pass — 16 of 16 acquired artifacts fully validated |

The original gate list, for reference:

- Existing v2 tests pass unchanged: `cargo test --locked --all-targets`,
  `--all-features`, and `--test conformance_v2 --no-default-features`.
- Existing v2 corpus hashes, entry counts, and failure classifications are
  byte-identical to the 2026-08-10 exhaustive ledger.
- Existing limits and `MemoryUsage` accounting are unchanged for v2 inputs.
- The benchmark baseline in `.codex/benchmarks/2026-08-10-macos-arm64.md` holds
  within its recorded 2x diagnostic threshold on the recorded host.
- The same shared-core behavioral tests run against synthetic v1 and synthetic
  v2 with identical assertions.
- Every accepted v1 artifact completes ordinal, raw-lookup, duplicate, and MDX
  payload or MDD span/stream validation.
- Every rejected artifact has a structured retained classification.
- Malformed input cannot panic or bypass limits.
- No breaking public API change unless separately approved under the `0.x`
  policy; compatible additive APIs are recorded in the changelog and must not
  alter the version-1 cutover contract. The current Required-version checks
  centralize the AALookup adapter's former preflight policy; they do not change
  which grammar `GeneratedByEngineVersion` selects.

### Gated decisions

Two decisions may change which files `mdictlib` accepts and must not be made as
a side effect of refactoring:

1. **Dispatch attribute.** Any change from `GeneratedByEngineVersion` to
   `RequiredEngineVersion`, or any future change that lets the requirement
   select a grammar, requires re-running the exhaustive corpus audit and
   comparing the full outcome ledger before and after. The current parser only
   centralizes the adapter's pre-existing malformed/future/conflict checks;
   it never lets the requirement select a grammar. A change in any artifact's
   class still blocks a future dispatch change until reviewed.
2. **ISO-8859-1 support.** Adding it changes the supported-encoding set and
   affects 11 local v1 files. It is a separate scoped decision with its own
   fixtures, not part of milestone 4.

## 10. Corpus Provenance And Acquisition Policy

The repository uses deliberately separate corpus states:

```text
candidate inventory
  -> exact-inventory-bound local-testing selection
  -> bounded bootstrap in ignored .corpus + draft/outcome report
  -> reviewed hash lock -> deterministic mdictlib-corpus.tsv -> parser evidence
```

- Candidate inventories record what a named source exposed at a point in time.
  Discovery is not authorization, integrity verification, or parser evidence.
- The selection pins the source inventory byte digest plus the complete
  selected source-path/type/URL/size set. Bootstrap revalidates that exact
  inventory denominator and binds the selection digest, entry/artifact counts,
  advertised bytes, and collision-safe local paths into every draft/outcome
  transition; promotion rejects missing or extra outcome rows.
- A reviewed lock records stable source identity, explicit use/redistribution
  review, expected size and payload digest, source and local collision-safe
  paths, the bounded metadata-open/count observation plus its exact observer
  binary identity, and whether entry/logical baselines are independent or
  `mdictlib`-self-observed.
- Bootstrap lock/outcome verification rejects logical fields, while a separate
  exact derivation gate verifies the pre-baseline lock plus exhaustive
  ledger/TSV against any logical-baseline lock. Both links and their inputs are
  retained when logical baselines are tracked.
- Bootstrap and repeat acquisition are opt-in, bounded by per-file/total byte
  ceilings, inactivity timeouts, and absolute deadlines, and write to the
  ignored `.corpus/` cache. Production URLs are credential/query-free HTTPS,
  must resolve exclusively to public addresses, connect through a validated
  pinned DNS answer, and may redirect only within the reviewed origin; a
  different origin requires a new review. Git LFS is not a corpus store: it
  would still redistribute the payloads, consumes metered storage/bandwidth,
  and cannot hold every observed file under common hosted per-object limits.
- A local `mdictlib-corpus.tsv` selects acquired bytes for the Rust harness.
  The harness independently verifies containment, kind, byte count, and file
  SHA-256 before parsing.
- Corpus evidence retains every selected row. Unsupported format/version,
  encrypted/passcode-required, corrupt/truncated, authorization-denied, and
  acquisition-failed outcomes remain visible instead of being filtered away.
  Bootstrap acquisition and promotion preserve a complete, selection-bound
  outcome report even though only successful metadata-open/count observations
  enter the lock and exhaustive regression manifest. This bootstrap count-only
  observation does not decode keys or payloads and is not structural or
  exhaustive validation. Promotion records the canonical source-draft and
  promoted-lock identities; the reviewed lock and complete outcome report are
  an independently verifiable pair and must be tracked together.

The AALookup `generate-dictionary-catalog.mjs` output is only a candidate
source. Its model prompt requests a "reasonable set," explicitly says it need
not visit every file, and defaults to 25 browsing steps. No current draft was
available in the referenced checkout. Consequently, importing such a draft
must never auto-promote entries into a reviewed lock.

For the deterministic audit started at `2026-08-10T04:06:35.081Z`, "all direct
MDX files" means the 1,254 direct `.mdx`/`.MDX` rows exposed while recursively
traversing 990 same-origin auto-index directories below
`https://mdx.mdict.org/`. The exact tracked inventory file SHA-256 is
`51ba61e985e42351ce7a1ee0c7713ac1f9f02284870383a12c3350ad2b5fa74d`.
The inventory itself is
[`corpus/mdict-org-2026-08-10.inventory.json`](../corpus/mdict-org-2026-08-10.inventory.json).
The MDX rows advertised 40,084,630,153 bytes and their directory-listing
fingerprint was
`cfa8cdc0e3b1579280398a295e45b7b56fb7c5ee856aa138492cbc72e6eac77d`.
That scope excludes the rest of the Internet and files hidden inside archives
or generated by scripts. It is not a payload-integrity claim. The same crawl
found 335 direct MDD links totaling 47,594,522,494 bytes with listing
fingerprint
`5bd6e1a9106b128b34770a35232c2a289c47c39c628d68bcdb42d00ec9b3d823`.
Four MDX and two MDD files exceeded 2 GiB. NFC-normalized,
Unicode-lowercased decoded basenames collided across 59 groups/123 MDX rows
and 25 groups/54 MDD rows.
Across every listed type, it recorded 2,992 direct files advertising
144,841,177,042 bytes.

The 2026-08-10 local acquisition completed all 1,254 selected MDX transfers,
exactly 40,084,630,153 bytes, with zero acquisition errors. Redistribution was
not authorized, so the payloads remain ignored under `.corpus/` and are not Git
or Git LFS objects. The tracked acquisition pair is
[`corpus/catalog.lock.json`](../corpus/catalog.lock.json) (2,197,293 bytes;
SHA-256
`d1baaaddc642d926e7f74a33e6d49dc1c302871c5a3dda3de91a872b2c4a4e2d`)
and
[`corpus/mdict-org-2026-08-10.acquisition-outcomes.json`](../corpus/mdict-org-2026-08-10.acquisition-outcomes.json)
(3,383,244 bytes; SHA-256
`f19ced0c1b844277689cc723d15a762ea81678d3b4a19ddf5e28fe1af568ea65`).
Count-only bootstrap promoted 792 files with 37,377,272,230 bytes and
89,051,220 declared entries; its complete outcome denominator retains 453
non-v2, six summary-decode, and three truncated-record exclusions. Those 453
non-v2 rows are the v1 evidence base described in section 8.3.

The final full run used isolated artifact processes at concurrency 2 with a
21,600,000-ms timeout on macOS 26.6 / Darwin 25.6 arm64 (T6020), rustc/cargo
1.97.1 for `aarch64-apple-darwin`, and Node.js 26.5.1. The tracked exhaustive
ledger
[`corpus/mdict-org-2026-08-10.exhaustive-outcomes.json`](../corpus/mdict-org-2026-08-10.exhaustive-outcomes.json)
(612,424 bytes; SHA-256
`ba3ac714348f07fa2f90762f08878294dd41289d01bf0db17f31ca92dc26835c`)
records 757 whole-artifact passes covering 27,098,834,819 bytes and 78,368,836
fully traversed entries. The remaining 35 artifacts, totaling 10,278,437,411
bytes and declaring 10,682,384 entries, stopped at the first recorded failure:
27 record-decode failures, three key-decode failures, two zlib stream failures,
two zlib ADLER32 mismatches, and one summary-boundary mismatch. Therefore the
run did not emit a logical TSV or L1 lock. These results are self-observed
regression evidence, not independent correctness proof. The failure classes
are recorded at the strict parser boundary; follow-up source forensics found no
parser change warranted. The exact acquisition, promotion, sync, and exhaustive
commands are recorded verbatim in `corpus/README.md`.

The corpus also supplied positive whole-file evidence for the narrowly
supported legacy v2 keyword-index variant: one real 213,587-entry artifact
completed the full audit. Exhaustive duplicate checking was changed to validate
each complete duplicate group once and use logarithmic membership probes, with
a dedicated large noncontiguous-duplicate regression.

**MDD evidence status.** The initial reviewed selection contained no MDD
payloads. A subsequent, explicitly approved local-testing and license review
acquired 16 exact-stem candidates under the bounded workflow described above;
all 16 were version-inspected and passed the real-file audit (14 declared
version 1.2 and 2 declared version 2.0). The remaining 335 inventory rows are
discovery-only and are not parser evidence. This narrow sample does not
generalize to every MDD producer or extension variant.

CI covers schemas, deterministic transformations, bounded local acquisition
fixtures, manifest parsing, isolated-runner failure handling, and synthetic
dictionaries. Full remote acquisition and exhaustive corpus validation are
manual or self-hosted because they require authorization review plus
substantial network, disk, and runtime budgets. Full validation clears stale
outputs before building, identifies and rechecks the audit executable, runs one
exact-lock artifact per timeout-bounded subprocess, and atomically records a
complete outcome report bound to the catalog identity, artifact denominator,
and runner identity; it emits a digest-pinned logical-audit TSV only on complete
success.

## 11. Reproducibility And Performance Policy

Authorized corpus tests use `MDICT_CORPUS_DIR` plus `mdictlib-corpus.tsv`.
Version 1 rows declare a normalized relative path, kind, byte count, SHA-256,
and entry count. Version 2 rows add optional `key_sha256` and
`payload_sha256`; the isolated exhaustive audit verifies each logical digest
that is present. A successful exact-denominator full run can atomically emit an
exact logical-audit TSV, but promoting it into the reviewed lock requires the
matching canonical complete-success outcome ledger; exact catalog,
denominator, runner, and audit identities; an explicit self-observed
acknowledgement; and provenance fields. Explicit ignored-suite invocation fails
with setup instructions if the corpus or any required fact is absent or wrong.

The benchmark harness reports:

- cold open-through-first-lookup and metadata open;
- locator construction and warm p50/p95/p99 lookup;
- complete key scans and deterministic key hashes;
- sequential and ordinal MDX payload hashes;
- streamed and materialized MDD hashes;
- concurrent first lookup;
- accounted parser memory and externally measured peak RSS.

The checked-in 2026-08-10 baseline uses a 2x diagnostic threshold after three
runs on the recorded host/toolchain. It is a regression investigation trigger,
not a cross-machine performance promise. Default limits remain intentionally
well above the largest observed valid-corpus peak while still bounding hostile
metadata.

## 12. First-Release Transition (Complete)

The authorized transition completed these external actions:

1. set `git@github.com:lonelam/mdictlib.git` as the canonical remote;
2. reviewed the exact `0.1.0` package contents and release notes;
3. made the root package publishable while keeping the fuzz crate unpublished;
4. published `mdictlib` `0.1.0` and pushed the `v0.1.0` tag;
5. replaced the pre-release policy with the `0.x` compatibility policy.

No parser, test, documentation, benchmark, or package implementation work was
deferred to the transition.

## 13. Deferred And Out Of Scope

- future-major layouts beyond version 1 and version 2;
- ISO-8859-1 text decoding, until separately scoped (section 9);
- mmap, persistent MDD sidecars, larger/LRU caches, async I/O, or parallel
  decompression until profiling justifies them; the shipped MDX index uses safe
  bounded positional reads and makes no zero-residency claim;
- writer support in the published crate, compact-text rewriting, stylesheet
  expansion, HTML/JS/CSS interpretation, filesystem extraction, and
  multi-volume discovery policy — v1 fixture encoders and any diagnostic
  converter are test-only and never linked into the library;
- library-owned completion ranking, fuzzy ranking/correction, redirect
  resolution, and duplicate-record merging. Normalized prefix lookup and
  early-break normalized-key traversal are implemented by `prefix_keys` and
  `scan_normalized_keys`, with persistent-index equivalents; callers own the
  completion/edit-distance policy and its work, deadline, and cancellation
  budgets;
- host application integration, UI/resource ordering, and rollout policy.

Update this roadmap and `.codex/STATUS.md` whenever architecture, API, scope,
risks, fixtures, or evidence changes. Update `AGENTS.md` when the compatibility
or release policy changes.
