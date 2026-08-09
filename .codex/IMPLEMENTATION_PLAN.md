# mdictlib Implementation Roadmap

Last updated: 2026-08-10

## 1. Release State And Design Premise

`mdictlib` is a first-release candidate. It has never been published, so the
repository intentionally contains one coherent API rather than compatibility
shims for earlier local-only designs. The candidate version is `0.1.0`.

All implementation milestones in this roadmap are complete. `publish = false`
remains a deliberate guard: publishing a crate, creating a release tag, and
pushing to the canonical repository are irreversible external actions and
require explicit maintainer authorization. The release-transition checklist in
section 8 records those actions without treating them as parser work.

Evidence rules:

- the checked-in source and executable tests are authoritative;
- `draft/` supplied audit observations and test targets, not importable code;
- private corpus bytes are never committed;
- corpus claims must name a manifest digest, host/toolchain, and exact command;
- performance numbers are baselines on the measured host, not universal SLAs.

## 2. Goal And Supported Scope

The release candidate is a defensive, library-first reader for MDict major
version 2 `.mdx` and `.mdd` files. It provides:

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
candidate additionally enforces:

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
| 6. Performance and release-candidate audit | complete | Versioned private-manifest evidence, locator/warm lookup/full scan/ordinal/stream/materialize/concurrency/RSS measurements, docs and package gates |

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

## 7. Reproducibility And Performance Policy

Private corpus tests use `MDICT_CORPUS_DIR` plus `mdictlib-corpus.tsv`. Each row
declares a normalized relative path, kind, byte count, SHA-256, and entry count.
Explicit ignored-suite invocation fails with setup instructions if the corpus
or any fact is absent or wrong.

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

## 8. First-Release Transition (Explicit Authorization Required)

The repository is prepared for, but does not autonomously perform, these
external actions:

1. confirm the canonical remote and run the committed CI workflow on Linux,
   macOS, and Windows;
2. review the packaged `0.1.0` file list and release notes one final time;
3. explicitly authorize removing `publish = false`;
4. publish `mdictlib` and create/push the `v0.1.0` tag;
5. after publication, replace this pre-release API policy with an explicit
   compatibility/versioning policy.

No parser, test, documentation, benchmark, or package implementation work is
deferred to this transition.

## 9. Deferred And Out Of Scope

- version 1.x or future-major layouts;
- mmap, persistent sidecars, larger/LRU caches, async I/O, or parallel
  decompression until profiling justifies them;
- writer support, compact-text rewriting, stylesheet expansion, HTML/JS/CSS
  interpretation, filesystem extraction, and multi-volume discovery policy;
- prefix, completion, fuzzy search, redirects, and duplicate-record merging;
- host application integration, UI/resource ordering, and rollout policy.

Update this roadmap and `.codex/STATUS.md` whenever architecture, API, scope,
risks, fixtures, or evidence changes. Update `AGENTS.md` when publication changes
the compatibility premise.
