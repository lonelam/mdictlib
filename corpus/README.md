# Validation Corpus

This directory tracks corpus metadata and tooling policy, not dictionary
payloads. Downloaded files belong in the repository-local, Git-ignored
`.corpus/` directory. Do not add them to normal Git objects or Git LFS.

## Why The Bytes Are Local

A file being publicly reachable does not grant a license to redistribute it.
The audited index includes commercial works for which this project has not
established redistribution permission. Git LFS changes storage mechanics but
would still publish those bytes, and its hosted storage and bandwidth are
metered. The observed corpus also contains individual files above common
hosted-LFS object limits. See GitHub's documentation on
[Git LFS storage and billing](https://docs.github.com/en/billing/concepts/product-billing/git-lfs),
[Git LFS limits](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-git-large-file-storage),
and [repository size guidance](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github).

Even an entry with a redistribution grant stays in `.corpus/` under the
current project policy. Moving a separately licensed payload to a dedicated
artifact repository would require an explicit maintainer decision; it must not
silently enter this source repository.

## Evidence Levels

Keep these claims distinct:

1. **Inventory:** a named source listed a URL, type, and advertised size at a
   recorded time.
2. **Review:** a person recorded why local testing is permitted and whether
   redistribution is separately permitted.
3. **Locked acquisition:** the downloaded bytes match an immutable resolved
   URL, byte count, and SHA-256.
4. **Bootstrap open/count observation:** a bounded, count-only `mdictlib`
   subprocess opened the file through its normal eager metadata/index path and
   reported the physical entry count. It did not decode a key, record, or
   resource payload.
5. **Exhaustive regression:** every physical key, raw lookup, duplicate set,
   ordinal payload/span route, logical key digest, and logical payload digest
   agreed with the recorded baseline.
6. **Independent correctness:** evidence from a separately implemented writer,
   specification, publisher, or reader agrees. A value first observed by
   `mdictlib` is useful for regression but is not independent proof that the
   interpretation is correct.

The reviewed lock records entry-count provenance as `independent`,
`publisher`, or `mdictlib-self-observed` so the last distinction is not lost.

## What “All” Means

The referenced AALookup generator is not exhaustive. It asks a model to gather
a "reasonable set," defaults to at most 25 browsing steps, and explicitly says
it need not visit every file. It verifies that submitted URLs resolve, but it
does not record content hashes, sizes, licenses, or proof that the candidate
set is complete. No
`../aalookup/.dev-data/server/dictionary-catalog.draft.json` existed during the
2026-08-10 audit.

For this project, **all direct MDX files in the 2026-08-10 source snapshot**
means exactly the direct `.mdx`/`.MDX` rows observed while recursively crawling
the same-origin auto-index pages below <https://mdx.mdict.org/>. It does not
mean all dictionaries on the Internet, entries hidden inside archives,
resources created by executing scripts, or every file a later crawl may expose.

That tested, row-bounded metadata-only audit traversed 990 directories and
found:

- 1,254 direct MDX files advertising 40,084,630,153 bytes;
- four MDX files larger than 2 GiB and 59 NFC-normalized,
  Unicode-lowercased decoded-basename collision groups containing 123 MDX
  rows;
- 335 direct MDD files advertising 47,594,522,494 bytes;
- two MDD files larger than 2 GiB and, under the same normalized-basename
  procedure, 25 collision groups containing 54 MDD rows;
- 2,992 direct files of all types advertising 144,841,177,042 bytes.

The MDX listing fingerprint is
`cfa8cdc0e3b1579280398a295e45b7b56fb7c5ee856aa138492cbc72e6eac77d`.
It was computed by sorting the 1,254 MDX rows by absolute URL, serializing each
exactly as `<decimal-bytes>\t<absolute-URL>\n`, and hashing that byte stream with
SHA-256. The analogous MDD fingerprint is
`5bd6e1a9106b128b34770a35232c2a289c47c39c628d68bcdb42d00ec9b3d823`.
These fingerprint listing metadata only. They do **not** authenticate the file
bodies, and the inventory crawl did not download payloads.

## Catalog Files

- [`mdict-org-2026-08-10.inventory.json`](mdict-org-2026-08-10.inventory.json)
  is the immutable metadata-only output
  of the corrected row-bounded live crawl started at
  `2026-08-10T04:06:35.081Z`. It retains all 2,992 direct file rows, not
  payload bytes, and has file SHA-256
  `51ba61e985e42351ce7a1ee0c7713ac1f9f02284870383a12c3350ad2b5fa74d`.
- [`catalog.schema.json`](catalog.schema.json) defines the reviewed lock format.
- [`catalog.lock.json`](catalog.lock.json) is the reviewed 792-artifact
  metadata-open/count lock: 2,197,293 bytes, SHA-256
  `d1baaaddc642d926e7f74a33e6d49dc1c302871c5a3dda3de91a872b2c4a4e2d`.
- [`mdict-org-2026-08-10.acquisition-outcomes.json`](mdict-org-2026-08-10.acquisition-outcomes.json)
  is the complete 1,254-row acquisition/bootstrap outcome report paired with
  that lock: 3,383,244 bytes, SHA-256
  `f19ced0c1b844277689cc723d15a762ea81678d3b4a19ddf5e28fe1af568ea65`.
- [`mdict-org-2026-08-10.exhaustive-outcomes.json`](mdict-org-2026-08-10.exhaustive-outcomes.json)
  is the complete 792-row exhaustive ledger: 612,424 bytes, SHA-256
  `ba3ac714348f07fa2f90762f08878294dd41289d01bf0db17f31ca92dc26835c`.

## Completed 2026-08-10 MDX Run

All 1,254 selected direct MDX objects were downloaded into the ignored local
`.corpus/` cache: exactly 40,084,630,153 bytes and zero acquisition errors. The
final ignored bootstrap draft is 6,249,797 bytes with SHA-256
`6cd3d21b14f9c8195fa305e36b1cbf886b23213eeba6b09207cf6487413f2243`.
Unverified redistribution rights and the corpus size are why none of those
payload bytes are in Git or Git LFS.

Bootstrap metadata open/count promoted 792 files totaling 37,377,272,230 bytes
and 89,051,220 declared physical entries. The outcome report keeps all 462
exclusions: 453 non-v2 files, six keyword-summary decode failures, and three
truncated record sections. The generated local manifest has 792 rows, is
107,469 bytes, and has SHA-256
`f45f5e02ea5eaf3eecf032048c72db62ce191310978b0eef96258d39790daef1`.

The isolated exhaustive run completed 757 whole artifacts, covering
27,098,834,819 bytes and 78,368,836 entries. The other 35 artifacts stopped at
their first recorded failure; they total 10,278,437,411 bytes and declare
10,682,384 entries, which were not all traversed. The failure classes are:

- 17 GBK and ten UTF-8 record-decode failures;
- two GBK and one UTF-8 key-decode failures;
- two zlib stream failures and two zlib ADLER32 mismatches; and
- one key-summary boundary mismatch.

Because all 792 artifacts did not succeed, the run deliberately emitted no
logical-audit TSV and no L1 logical-baseline lock. The lock counts and logical
audit results are `mdictlib`-self-observed regression evidence, not independent
proof of correctness.

Each reviewed entry has a stable ID, title, information URL, review decision,
license status/evidence, reviewer, review time, and notes. Each artifact
records its inventory-relative source path, a collision-safe local relative
path, original and resolved URL, kind, byte count, payload SHA-256, expected
physical entries and their provenance, optional logical hashes and their
provenance, and any count-only bootstrap observation or observation error.

Private local-testing approval records `testingAllowed: true`, a reviewer,
timestamp, and non-empty rationale. It may retain an unverified license and no
license URL. `redistributionAllowed` is independent and remains false unless an
affirmative license and its URL establish permission to republish the bytes.
Pending or rejected candidates remain metadata; acquisition must not silently
treat them as approved.

## Discovery

Create a fresh, bounded metadata inventory without downloading any listed
payload or extracting an archive:

```sh
node scripts/corpus/inventory-mdict-index.mjs \
  --root https://mdx.mdict.org/ \
  --output .corpus/mdict-index.inventory.json
```

The crawler stays on the configured origin and beneath its root path. Defaults
bound page count, listed-file count, bytes per page, per-page time,
concurrency, and aggregate in-flight page bodies. The latter defaults to
67,108,864 bytes through `--max-in-flight-page-bytes`; run
`node scripts/corpus/inventory-mdict-index.mjs --help` for overrides. The output
retains every direct file type, not only MDX, so companion MDD, styles, scripts,
archives, and unknown extensions remain visible. It fetches only HTML directory
pages and never executes listed CSS/JavaScript or extracts archives.

If AALookup later produces a reviewed draft, normalize it into deterministic
candidate metadata with:

```sh
node scripts/corpus/import-aalookup-catalog.mjs \
  --input ../aalookup/.dev-data/server/dictionary-catalog.draft.json \
  --output .corpus/aalookup.candidates.json
```

This imports candidates only. Its source digest, sampling limitation, URL
roles, and file classifications are retained; it does not approve licenses,
create a reviewed lock, or download anything.

## Review And Acquisition

Never promote inventory rows mechanically. A reviewer must decide whether
private local automated testing is authorized, record the basis, and separately
record whether a verified license permits redistribution. The selector assigns
URL-hashed local names so decoded or case-insensitive basename collisions
cannot overwrite one another. Archives, CSS, and JavaScript may be useful
provenance but are not executed or auto-extracted by this workflow.

Select every direct MDX row in the tracked snapshot only after making that
explicit local-testing decision:

```sh
node scripts/corpus/select-inventory.mjs \
  --input corpus/mdict-org-2026-08-10.inventory.json \
  --type mdx \
  --output .corpus/mdict-org-mdx.selection.json \
  --reviewed-by "Lonelam (maintainer request)" \
  --reviewed-at 2026-08-10T04:38:54.272Z \
  --notes "Maintainer explicitly requested private repo-local validation of every direct MDX artifact in the bound snapshot; source availability is not redistribution permission." \
  --approve-local-testing
```

The command selects all 1,254 snapshot rows, not a success-only subset. It
sets `redistributionAllowed` to false and leaves the license unverified; source
availability is never converted into a redistribution grant. Inspect the
selection before acquisition, but regenerate it with `--notes` rather than
hand-editing it: bootstrap requires the selector's canonical stable JSON bytes.
Repeat with `--type mdd` and a separate output only when companion-resource
testing is also authorized.

For the MDX selection above, the canonical selection-file SHA-256 is
`69a59efa4f6876b542191d3d696915168d5b7672c45e8f130ac1d11263857627` and
the selected source-set SHA-256 is
`7a481fb209fca5661f04ff2d0d6fccf33c58c6f4876bddb671d0f734e923cfc1`.
These identify the reviewed 1,254-row denominator; they are not acquisition or
parser-success claims.

The selection binds the exact inventory file SHA-256, root, snapshot time,
selected type/count/advertised-byte total, and a digest of the complete
source-path/type/URL/size set. Bootstrap receives both the selection and that
inventory, re-hashes their bytes, regenerates the selected set, and rejects a
missing, added, or changed row. Its draft `selectionBinding` additionally pins
the complete selection-file digest, entry/artifact counts, advertised bytes,
and local-path-inclusive artifact-set digest.

The committed lock is the immutable input for repeat acquisition. Acquisition
must use bounded concurrency, timeouts, redirect limits, per-file and total
byte ceilings, exclusive temporary files, verified hard-link installation,
destination containment checks, and exact byte/SHA-256 verification. A first acquisition
cannot establish trust by itself: its size, resolved URL, digest, and entry
facts remain a proposed lock update until reviewed.

The exact final acquisition/count command was:

Plan for more than 50 GB of free space for this MDX-only snapshot, including
in-flight partials and tooling output. An independently selected MDD corpus
advertises another 47,594,522,494 bytes.

```sh
node scripts/corpus/lock-corpus.mjs \
  --selection .corpus/mdict-org-mdx.selection.json \
  --inventory corpus/mdict-org-2026-08-10.inventory.json \
  --output .corpus/mdict-org-mdx.lock.draft.json \
  --root .corpus \
  --concurrency 16 \
  --retries 6 \
  --timeout-ms 1800000 \
  --deadline-ms 21600000 \
  --observe-timeout-ms 60000 \
  --max-file-bytes 3000000000 \
  --max-total-bytes 50000000000
```

After validating the inventory, reviewed selection, limits, and every output
collision, bootstrap removes any previous draft before acquisition starts. An
interrupted or fatal rerun therefore cannot leave older evidence looking like
the result of the current code.

These explicit ceilings are intentionally tighter than the 64 GiB total and
8 GiB per-file defaults. The 50,000,000,000-byte total covers this snapshot's
40,084,630,153 advertised MDX bytes, and the 3,000,000,000-byte per-file limit
covers its 2,539,842,394-byte largest MDX file. Reconsider the limits for a
different snapshot; do not copy them blindly. `--timeout-ms` is a network
inactivity timeout reset by response progress, not a whole-transfer deadline.
The shown 30-minute inactivity bound overrides the 120,000-ms default.
`--deadline-ms` is an absolute per-attempt deadline; 21,600,000 ms is the
six-hour default. Production acquisition accepts only HTTPS URLs without
embedded credentials, query strings, or fragments, rejects non-public or
metadata-network targets (including hostnames resolving to any non-public
address), pins each connection to an already validated public DNS answer, and
follows only HTTPS redirects on the originally reviewed origin. A cross-origin
target must be reviewed as a new source URL rather than reached through a
redirect. This keeps credential- or signature-bearing strings out of the
acquisition URL fields in public locks and outcome reports.

Acquisition verifies the advertised size while streaming, records the resolved
URL and payload SHA-256, and continues across exhausted URL, size, deadline, or
network errors. Its `acquisitionOutcomes` retains source/review/advertised facts
and the error for every selected row. Stable `.part` metadata resumes with
`Range`/`If-Range` when the server supplies a validator; otherwise the tooling
safely restarts the transfer.

Unless `--skip-observe` is explicitly used, each acquired file then runs in a
separate `inspect --count-only` subprocess, bounded by
`--observe-timeout-ms`. The observer opens metadata/indexes and reports
`len()`; it does not perform the old first-five key/payload materialization
smoke test. Its provenance pins the observer mode, timeout, tool/version, and
the built observer binary's byte count and SHA-256, while the locked artifact's
containment, byte count, and SHA-256 are reverified before and after the
subprocess. An open/count failure is
retained as `observationError` and must not be generalized into a claim that
MDD `span_at` streaming or payload decoding is unsupported.

Existing schema-version-1 bootstrap journals with the exact legacy shape can
be reused without redownloading only after their recorded facts and local file
size/SHA-256 are reverified. They are then atomically upgraded with
`sourcePath`, `inventorySha256`, and `selectionSha256`; any other shape or
provenance mismatch fails closed. Journals remain local for resumable
re-observation.

The bootstrap draft intentionally has null `expectedEntries` and
`entryCountBasis`, so it is not yet a valid reviewed lock. Convert successful
open/count observations into explicitly self-observed regression baselines and
retain one outcome row for every selected artifact:

```sh
node scripts/corpus/promote-lock.mjs \
  --input .corpus/mdict-org-mdx.lock.draft.json \
  --output .corpus/mdict-org-mdx.lock.reviewed.json \
  --outcomes .corpus/mdict-org-mdx.acquisition-outcomes.json \
  --accept-self-observed
```

This explicit flag does not turn a self-observed count into independent parser
evidence. Promotion verifies the draft's bound selection digest, counts, byte
total, and exact artifact set; deleting a failed outcome or adding an
unreviewed success is rejected. The reviewed output promotes only artifacts
whose count-only observation succeeded. The outcome file retains every
selected row as `promoted`, `excluded`, or `acquisition-error`, including
acquisition/open-count errors, the complete `selectionBinding`, and the
canonical `sourceDraftBytes`, `sourceDraftSha256`,
`promotedLock: {bytes, sha256}`, and catalog facts. Verify the generated pair
independently of the draft with:

```sh
node scripts/corpus/promote-lock.mjs \
  --verify-pair \
  --output .corpus/mdict-org-mdx.lock.reviewed.json \
  --outcomes .corpus/mdict-org-mdx.acquisition-outcomes.json
```

This rejects any selection-denominator, catalog, result, or canonical
lock-identity mismatch. A bootstrap promotion pair must retain null logical
hash/provenance fields; those fields belong to the separately verified
exhaustive-evidence transition below. Review and track a promoted lock only
together with its exact verified outcome report; neither file is complete
provenance on its own. The final reviewed pair is tracked as
`corpus/catalog.lock.json` and
`corpus/mdict-org-2026-08-10.acquisition-outcomes.json`; re-run `--verify-pair`
against those exact paths after regeneration and before committing them.

The exact final sync command reverified the reviewed L0 bytes and generated the
deterministic version 2 Rust manifest:

```sh
node scripts/corpus/sync.mjs \
  --catalog .corpus/mdict-org-mdx.lock.reviewed.json \
  --root .corpus \
  --concurrency 16 \
  --retries 0 \
  --timeout-ms 1800000 \
  --deadline-ms 21600000 \
  --max-file-bytes 3000000000 \
  --max-total-bytes 50000000000
```

`sync.mjs` preflights the locked total, enforces per-file and aggregate limits,
refuses changed redirects/sizes/hashes, reuses only verified local files, and
writes `.corpus/mdictlib-corpus.tsv` atomically. The final manifest contains
792 rows, is 107,469 bytes, and has SHA-256
`f45f5e02ea5eaf3eecf032048c72db62ce191310978b0eef96258d39790daef1`.

## Rust Manifest And Exhaustive Validation

The acquired corpus root contains `mdictlib-corpus.tsv`. Both tab-separated
headers are accepted:

```text
# version 1
path    kind    bytes    sha256    entries

# version 2
path    kind    bytes    sha256    entries    key_sha256    payload_sha256
```

Paths are normalized POSIX-style relative paths and `kind` is `mdx` or `mdd`.
The file SHA-256 identifies the raw downloaded bytes. Version 2's
`key_sha256` and `payload_sha256` fields are independently optional; when
present, the exhaustive regression requires an exact match. Keep rows sorted
by path. See [`tests/corpus-manifest.example.tsv`](../tests/corpus-manifest.example.tsv).

The exact final full-validation command was:

```sh
node scripts/corpus/validate.mjs \
  --catalog .corpus/mdict-org-mdx.lock.reviewed.json \
  --root .corpus \
  --mode full \
  --outcomes-output .corpus/mdict-org-mdx.exhaustive-outcomes.json \
  --audit-output .corpus/mdict-org-mdx.logical-audit.tsv \
  --audit-concurrency 2 \
  --artifact-timeout-ms 21600000
```

Before invoking Cargo, full mode collision-checks and removes both requested
outputs, so a build failure cannot leave a stale outcome ledger or audit TSV.
Neither output may alias the catalog, generated manifest, an artifact, a
bootstrap journal, or a `.part`, `.part.json`, or `.part.lock` acquisition
file.

Full mode then builds the release/all-features `corpus_audit` example once,
queries its `--identity` protocol, and binds the executable byte count/SHA-256,
compile-time crate version, tool name, and protocol. It rechecks that exact
binary around the artifact runs and audits exactly one locked artifact per
isolated subprocess. Each audit verifies file identity before and after the run
and exhaustively exercises keys, raw lookups, duplicate ordering, ordinals,
MDX payloads or MDD span/stream/resource routes, and logical digests. The
defaults are two concurrent artifact processes and a 3,600,000-ms timeout per
artifact; use `--audit-concurrency` and `--artifact-timeout-ms` to change them.
Duplicate validation checks every member of a raw duplicate group once from
the group's first physical ordinal and uses logarithmic membership probes for
later rows, avoiding repeated whole-group scans. The audit example has five
active unit tests, including a 4,096-member noncontiguous-duplicate regression.

The atomic schema-version-2 exhaustive outcome JSON binds the exact catalog
byte count/SHA-256 plus a digest and relative-fact list for the complete locked
artifact denominator. Its exact top-level fields are `audit`, `catalog`,
`completeSuccess`, `denominator`, `execution`, `generatedAt`, `protocol`,
`results`, `runner`, `schemaVersion`, `summary`. The `runner` object records
`binaryBytes`, `binarySha256`, `protocol`, `tool`, and `version`; the production
protocol is `mdictlib-corpus-audit-v1` and the tool is
`mdictlib corpus_audit`. It records an outcome for every artifact, including
identity, runner, protocol, output-limit, baseline, and timeout failures,
without exposing absolute local paths. Replacing either the catalog or runner
invalidates the whole run and suppresses the TSV.

On complete success, the ledger's `audit` object pins the exact canonical TSV
byte count and SHA-256 and the requested five-column TSV is written atomically.
Otherwise `audit` is null, full mode exits nonzero after installing the
complete outcome ledger, and no TSV is created. Full mode then runs the ignored
`local_corpus` and `local_sample` suites only after exhaustive success. When
`--outcomes-output` is omitted, its default is
`.corpus/mdictlib-corpus-audit.outcomes.json`.

Use `--mode quick` for integrity plus the `local_corpus`/`local_sample` suites,
or `--mode verify` (equivalently `--verify-only`) to check locked bytes and the
exact generated manifest without invoking Rust. The audit output, outcomes,
concurrency, and artifact-timeout options are accepted only in full mode.

The command above produced the tracked 2026-08-10 ledger on macOS 26.6 /
Darwin 25.6 arm64 (T6020), rustc/cargo 1.97.1 targeting
`aarch64-apple-darwin`, and Node.js 26.5.1. Its audit runner was 822,368 bytes
with SHA-256
`957d958b23e6ecaf1347246b701d6c557290e36223982c3ef81618c90f3f0a0d`;
the 792-artifact denominator SHA-256 was
`c6155b5f49101898b1d8da3bae5a9ffa3a08c0c44dca8c622a0c15571084e17a`.
The ledger records 757 complete whole-artifact successes and 35 failures at the
first bad ordinal. One 213,587-entry real artifact using the coherent legacy v2
keyword-index layout completed the full audit: the parser accepts its exact
little-endian keyword-header ADLER32 and omitted summary terminators only after
the canonical big-endian checksum fails, while retaining exact count, size,
consumption, decoding, block-checksum, and boundary checks.

The 35 recorded failures comprise 17 GBK and ten UTF-8 record-decode errors,
two GBK and one UTF-8 key-decode errors, two zlib stream errors, two zlib
ADLER32 mismatches, and one summary-boundary mismatch. The 757 successful
artifacts cover 27,098,834,819 bytes and 78,368,836 fully traversed entries.
The 35 failed artifacts cover 10,278,437,411 bytes and declare 10,682,384
entries, but stopped at the first recorded error; those entries were not all
traversed. These are the exact source-data failure classes recorded by the
strict parser boundary; follow-up forensics identified no parser change
warranted for them.

Record resulting exact logical hashes only after every artifact succeeds and
after explicitly accepting that they are self-observed regression evidence:

```sh
node scripts/corpus/record-logical-baselines.mjs \
  --catalog .corpus/mdict-org-mdx.lock.reviewed.json \
  --outcomes .corpus/mdict-org-mdx.exhaustive-outcomes.json \
  --audit-tsv .corpus/mdict-org-mdx.logical-audit.tsv \
  --output .corpus/mdict-org-mdx.lock.logical.json \
  --accept-self-observed
node scripts/corpus/record-logical-baselines.mjs \
  --verify-chain \
  --catalog .corpus/mdict-org-mdx.lock.reviewed.json \
  --outcomes .corpus/mdict-org-mdx.exhaustive-outcomes.json \
  --audit-tsv .corpus/mdict-org-mdx.logical-audit.tsv \
  --output .corpus/mdict-org-mdx.lock.logical.json
node scripts/corpus/sync.mjs \
  --catalog .corpus/mdict-org-mdx.lock.logical.json \
  --root .corpus \
  --max-total-bytes 45000000000
node scripts/corpus/validate.mjs \
  --catalog .corpus/mdict-org-mdx.lock.logical.json \
  --root .corpus \
  --mode full
```

The baseline recorder requires a canonical, complete-success schema-version-2
ledger matching the same raw catalog bytes, exact artifact identities and
denominator, runner identity, and canonical TSV projection/digest. The
`--verify-chain` command independently re-derives the logical lock and requires
an exact canonical byte match, so fabricated logical hashes cannot be made
valid by changing only a catalog digest. Retain the pre-baseline reviewed lock
and its promotion outcomes alongside the exhaustive ledger, TSV, and logical
lock: `promote-lock --verify-pair` verifies the acquisition/count link, while
`record-logical-baselines --verify-chain` verifies the exhaustive logical link.
The recorder also binds the exact outcome-ledger digest, rejects aliased or
stale inputs, and still rejects missing, extra, duplicate, wrong-kind, or
wrong-count audit rows.

The 2026-08-10 run was not a complete success, so `audit` is null in the
tracked ledger and neither `.corpus/mdict-org-mdx.logical-audit.tsv` nor an L1
logical lock was created. Do not run the baseline-recording commands above
against this failed ledger.
Each promoted `logicalObservation` records the catalog, denominator, runner,
outcomes, and audit SHA-256 values together with the runner version and
`mdictlib-corpus-audit-v1` protocol in this canonical form:

```text
mdictlib isolated exhaustive audit (self-observed; not independent verification); catalog_sha256=<hex>; denominator_sha256=<hex>; runner_sha256=<hex>; runner_version=<version>; protocol=mdictlib-corpus-audit-v1; outcomes_sha256=<hex>; audit_sha256=<hex>
```

The final sync regenerates the manifest with both logical digests, and the
final full run then treats them as exact assertions. Review the updated lock
before tracking it; `logicalDigestBasis` must continue to disclose
`mdictlib-self-observed` unless independent evidence replaces it.

The individual ignored Rust suites can also be run directly:

```sh
export MDICT_CORPUS_DIR="$PWD/.corpus"
cargo test --locked --release --all-features --test local_corpus -- --ignored --nocapture
cargo test --locked --release --all-features --test local_sample -- --ignored --nocapture
cargo test --locked --release --all-features --test local_lookup_regression -- --ignored --nocapture
```

The direct `local_lookup_regression` test still performs an exhaustive
single-process pass and prints per-row logical hashes, but it is not equivalent
to the canonical full workflow: it does not provide per-artifact process
isolation, timeout enforcement, or a catalog/denominator-bound outcome report.
Copying self-observed values into a version 2 manifest establishes a future
exact regression baseline, not independent correctness evidence.

Keep the tracked inventory as the denominator rather than replacing it with a
success-only lock. Promotion outcomes retain every selected row, including
unavailable/changed acquisitions and unsupported version/format, encrypted or
passcode-required, corrupt, and truncated observations. Authorization-denied
rows remain in the inventory outside the selected set. Do not remove a
difficult file merely to make a pass rate appear higher. Encrypted files
without authorized credentials and rejected licenses remain inventory
evidence, not parser failures.

## CI Boundary

Normal CI exercises deterministic catalog transformations and bounded local
HTTP fixtures plus the synthetic Rust conformance suites. It does not contact
third-party dictionary sites or cache their payloads. Full reviewed-corpus
acquisition and exhaustive validation are manual or self-hosted operations
because their authorization, network, disk, and runtime requirements are
environment-specific.

Run the dependency-free tooling tests locally with:

```sh
node --test scripts/corpus/*.test.mjs scripts/corpus/test/*.test.mjs
```
