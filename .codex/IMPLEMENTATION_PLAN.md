# mdictlib Implementation Roadmap

Last updated: 2026-08-10

## 1. Release State And Design Premise

`mdictlib` `0.1.0` is the first public release. The repository established one
coherent API before publication rather than carrying compatibility shims for
earlier local-only designs.

All implementation milestones and the first-release transition in this roadmap
are complete. Future releases remain explicit maintainer actions governed by
the `0.x` compatibility policy in `AGENTS.md`.

Evidence rules:

- the checked-in source and executable tests are authoritative;
- `draft/` supplied audit observations and test targets, not importable code;
- dictionary corpus bytes are never committed to this source repository,
  including through Git LFS; even separately redistributable assets require an
  explicit artifact-hosting decision;
- discovery metadata is a candidate input until a human review records source,
  authorization, and immutable acquisition facts;
- corpus claims must name a manifest digest, host/toolchain, and exact command;
- performance numbers are baselines on the measured host, not universal SLAs.

## 2. Goal And Supported Scope

The `0.1.0` release is a defensive, library-first reader for MDict major version
2 `.mdx` and `.mdd` files. It provides:

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
wire-format decision and does not change the public API.

Version 1.x and future-major layouts, writing, HTML/style processing,
multi-volume discovery, prefix/fuzzy search, resource extraction policy,
mmap, and persistent sidecar indexes are out of scope.

## 3. Final Architecture

```text
public root API
  ├── MdxFile / MdxEntry
  ├── MddFile / MddResource / MddResourceSpan
  ├── KeyEntry / KeyOrdinal / KeyMatches / MatchBasis
  ├── Header / OpenOptions / Passcode / Limits / MemoryUsage
  └── Error / Result
            │
            ▼
src/core/
  ├── mod.rs       shared open state, caches, memory accounting
  ├── keys.rs      lazy key blocks and ordinal resolution
  ├── records.rs   record descriptors, lazy blocks, span traversal
  ├── iter.rs      shared fused physical iterators
  ├── normalize.rs header-controlled key normalization
  └── locator.rs   lazy global duplicate-aware locator
            │
            ▼
src/format/
  ├── header.rs / key_index.rs / record_index.rs
  ├── cursor.rs / encoding.rs
  └── checksum.rs / compression.rs / crypto.rs

private cross-cutting modules: source.rs and limits.rs
```

Dependency rules:

- `mdx.rs` and `mdd.rs` are payload-specific facades only;
- format parsing never depends on the core or forks by container kind;
- MDX and MDD share key, locator, record-descriptor, cache, and budget paths;
- normalization is lookup policy, not text encoding;
- concrete iterators, parser/index types, codecs, sources, and caches stay
  private;
- the separate fuzz crate reaches only narrow doc-hidden adapters under
  cargo-fuzz's checked `cfg(fuzzing)`; no fuzz-only Cargo feature is published.

## 4. First-Release API Contract

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

Known logical header attributes are resolved ASCII-case-insensitively while
raw spellings remain inspectable. Semantically equivalent aliases are accepted
and conflicting aliases are rejected. When `StripKey` is enabled, comparison
removes non-alphanumeric ASCII code points and leaves non-ASCII characters
unchanged; case folding remains controlled separately by the header.

### MDX

```text
open / open_with_options / header / len / is_empty / memory_usage
keys / key_at / keys_at
locate / lookup / entry_at / entries
```

Every `MdxEntry` contains its physical key row and decoded text.

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
- `MemoryUsage` reports conservative accounted current/peak, metadata,
  locator, and cache estimates;
- `Passcode::new()` validates borrowed input before cloning, bounds the user
  identity, uses fallible allocation, and redacts `Debug`;
- `Header` exposes semantic getters plus exact raw attribute iteration;
- public payload debug output reports lengths rather than dumping content.

## 5. Defensive Invariants

Every file-derived count, length, offset, range, decoded size, reservation, and
section sum is checked before the corresponding read or allocation. The
release additionally enforces:

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
- iterator error fusion after descriptor or payload failures;
- no `unsafe` code.

## 6. Completed Milestones And Exit Evidence

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

## 7. Corpus Provenance And Acquisition Policy

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
non-v2, six summary-decode, and three truncated-record exclusions.

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

## 8. Reproducibility And Performance Policy

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

## 9. First-Release Transition (Complete)

The authorized transition completed these external actions:

1. set `git@github.com:lonelam/mdictlib.git` as the canonical remote;
2. reviewed the exact `0.1.0` package contents and release notes;
3. made the root package publishable while keeping the fuzz crate unpublished;
4. published `mdictlib` `0.1.0` and pushed the `v0.1.0` tag;
5. replaced the pre-release policy with the `0.x` compatibility policy.

No parser, test, documentation, benchmark, or package implementation work was
deferred to the transition.

## 10. Deferred And Out Of Scope

- version 1.x or future-major layouts;
- mmap, persistent sidecars, larger/LRU caches, async I/O, or parallel
  decompression until profiling justifies them;
- writer support, compact-text rewriting, stylesheet expansion, HTML/JS/CSS
  interpretation, filesystem extraction, and multi-volume discovery policy;
- prefix, completion, fuzzy search, redirects, and duplicate-record merging;
- host application integration, UI/resource ordering, and rollout policy.

Update this roadmap and `.codex/STATUS.md` whenever architecture, API, scope,
risks, fixtures, or evidence changes. Update `AGENTS.md` when the compatibility
or release policy changes.
