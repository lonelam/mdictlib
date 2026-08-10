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
- Public corpus metadata and acquisition tooling are tracked separately from
  ignored, locally authorized dictionary bytes under `.corpus/`.

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
- a narrowly signaled legacy v2 keyword-index layout: exact little-endian
  keyword-header ADLER32 and omitted summary terminators, accepted only after
  the canonical big-endian checksum fails

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
- The legacy v2 keyword-index fallback retains exact header/index checksums,
  count and size sums, full index consumption, decoded text, and block-boundary
  validation; it is not a general permissive parse mode.
- Record starts are nondecreasing within and across key blocks, including
  direct access to interior rows of a later block.
- Streaming span validation is separate from whole-resource materialization.
- Parser iterators yield at most one error and then remain exhausted.

## Verification Snapshot

Current local gates on 2026-08-10:

- `cargo test --locked --all-targets`: passed
- `cargo test --locked --all-targets --all-features`: passed
  - 91 active tests passed
  - 3 explicit private-corpus tests ignored by default
- `cargo test --locked --test conformance_v2 --no-default-features`: 16 passed
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

Metadata open/count promoted 792 files totaling 37,377,272,230 bytes and
89,051,220 declared entries. The complete outcome report retains 462
exclusions: 453 non-v2 formats, six keyword-summary decode failures, and three
truncated record sections. The generated 792-row local manifest is 107,469
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
`.codex/benchmarks/2026-08-10-macos-arm64.md`.

## Release Hygiene

- `.github/workflows/ci.yml` exists.
- `CHANGELOG.md` has dated `0.1.0` release notes.
- `README.md`, crate rustdoc, examples, public API tests, and package metadata
  describe the same released behavior.
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
- Package: `https://crates.io/crates/mdictlib/0.1.0`

Future releases require a version decision, synchronized changelog and docs,
the same release gates, and explicit maintainer authorization.
