# mdictlib

`mdictlib` is a safe, library-first Rust reader for MDict `.mdx` text
dictionaries and `.mdd` resource dictionaries.

## Design

- MDX and MDD use one shared, file-backed parsing core.
- Opening reads bounded metadata and block indexes; key and record blocks stay
  lazy.
- Every physical key has a `KeyOrdinal`, so duplicates retain distinct
  identities and payloads.
- Lookup is global and deterministic: all raw-exact matches win before any
  header-normalized fallback.
- Public iterators are opaque and fused; wire-format modules remain private.
- MDD spans are source-bound and can stream without exposing raw offsets or
  materializing a whole resource.
- Per-open `Limits` and aggregate working-memory accounting bound untrusted
  reads, decompression, allocation, locator construction, and materialization.
- Unsafe code is forbidden.

## Supported Scope

Both MDict major versions are read through the same public API, the same
shared core, and the same limits. The wire version is detected once, from the
header, and never influences lookup, iteration, ordinal access, record
decoding, or MDD streaming.

Common to both versions:

- UTF-8, UTF-16LE, GBK/GB2312, GB18030, and Big5 text decoding
- lazy iteration, physical-ordinal access, and duplicate-aware lookup
- the shared eight-byte block envelope with big-endian ADLER32
- optional pure-Rust LZO through the `lzo` feature

### MDict major version 2

- MDX and MDD sections
- uncompressed, zlib, and LZO blocks
- keyword-index encryption
- passcode-protected keyword-header encryption
- a narrowly identified legacy v2 keyword-index layout: an exact
  little-endian keyword-header ADLER32 plus omitted summary terminators, used
  only when the canonical big-endian checksum fails

### MDict major version 1

- MDX and MDD sections with 32-bit geometry, raw keyword metadata, one-byte
  summary lengths, and unterminated summaries
- uncompressed and LZO blocks

Validated against 453 authorized real v1.2 MDX artifacts (407 fully validated,
43,185,052 entries) and 14 authorized real v1.2 MDD artifacts (77,863 entries).
A dictionary's MDX and MDD need not share a wire version; each file's version is
resolved on its own.

**Not supported for version 1**, and refused with a precise structured error
rather than a guess:

- encrypted version 1 files — no authorized artifact declares encryption and no
  framing has been established
- ISO8859-1 text, which real version 1.2 dictionaries declare but whose MDict
  byte semantics are unresolved
- zlib version 1 blocks are decoded by the shared envelope but have never been
  observed in an authorized artifact, so creator compatibility is untested

A version 1 file is never retried under the version 2 grammar, or the reverse,
and no code path rewrites one version's bytes into the other's shape.

Independent full-file tests cover every listed encoding and compression path,
both encryption modes, multi-block boundaries, duplicate keys, corruption, and
hostile declarations, for both wire versions. Future-major layouts, writing,
HTML/style processing, resource extraction policy, multi-volume discovery,
prefix/fuzzy search, mmap, and persistent sidecars are out of scope.

## Using mdictlib

```toml
[dependencies]
mdictlib = "0.1.0"
```

Enable LZO when required by a dictionary:

```toml
[dependencies]
mdictlib = { version = "0.1.0", features = ["lzo"] }
```

## MDX

```rust,no_run
use mdictlib::{KeyOrdinal, MatchBasis, MdxFile};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dictionary = MdxFile::open("dictionary.mdx")?;

    if let Some(matches) = dictionary.locate("apple")? {
        println!("basis: {:?}, duplicates: {}", matches.basis(), matches.len());
        assert!(matches.basis() == MatchBasis::RawExact || !matches.is_empty());

        for ordinal in matches.iter() {
            let entry = dictionary.entry_at(ordinal)?.unwrap();
            println!("{}: {}", entry.ordinal().get(), entry.text());
        }
    }

    if let Some(entry) = dictionary.entry_at(KeyOrdinal::new(42))? {
        println!("physical entry 42 is {}", entry.key());
    }

    // Convenience lookup chooses the lowest matching physical ordinal.
    if let Some(entry) = dictionary.lookup("apple")? {
        println!("{}: {}", entry.key(), entry.text());
    }

    Ok(())
}
```

`locate()` searches every raw key first. Only a global raw miss enables the
header-controlled normalized index. `KeyMatches` preserves every matching
ordinal in physical order and reports `RawExact` or `HeaderNormalized`.

## MDD

Materialize a bounded resource:

```rust,no_run
use mdictlib::MddFile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resources = MddFile::open("dictionary.mdd")?;

    if let Some(resource) = resources.lookup("\\image.png")? {
        println!("{} bytes", resource.bytes().len());
    }

    Ok(())
}
```

Or resolve a source-bound span and stream it:

```rust,no_run
use mdictlib::MddFile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resources = MddFile::open("dictionary.mdd")?;

    if let Some(span) = resources.lookup_span("\\audio.mp3")? {
        let mut bytes = Vec::new();
        let written = span.copy_to(&mut bytes)?;
        assert_eq!(written, span.len());
    }

    Ok(())
}
```

Streaming is not subject to the whole-resource materialization ceiling;
individual decoded record blocks and aggregate parser work remain bounded.

## Physical Keys And Ordinals

`keys()` returns original keys in physical file order without reading record
payloads:

```rust,no_run
use mdictlib::{KeyOrdinal, MdxFile};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dictionary = MdxFile::open("dictionary.mdx")?;

    for key in dictionary.keys().take(10) {
        let key = key?;
        println!("{}: {}", key.ordinal().get(), key.key());
    }

    let keys = dictionary.keys_at(&[
        KeyOrdinal::new(7),
        KeyOrdinal::new(42),
        KeyOrdinal::new(7),
    ])?;
    println!("{keys:?}");

    Ok(())
}
```

Ordinals are stable only for the same unchanged dictionary file snapshot.

## Limits And Memory Diagnostics

```rust,no_run
use mdictlib::{Limits, MdxFile, OpenOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let limits = Limits::new()
        .with_materialized_record_bytes(16 * 1024 * 1024)
        .with_locator_bytes(256 * 1024 * 1024)
        .with_working_memory_bytes(512 * 1024 * 1024);
    let options = OpenOptions::new().with_limits(limits);
    let dictionary = MdxFile::open_with_options("dictionary.mdx", &options)?;

    let usage = dictionary.memory_usage()?;
    println!(
        "accounted current={} peak={} locator={}",
        usage.current_bytes(),
        usage.peak_bytes(),
        usage.locator_bytes(),
    );
    Ok(())
}
```

`MemoryUsage` values are conservative parser budget estimates, not allocator or
operating-system RSS. Payloads already returned to the caller are excluded.

## Encrypted Keyword Headers

```rust,no_run
use mdictlib::{MdxFile, OpenOptions, Passcode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let passcode = Passcode::new(
        "0123456789abcdef0123456789abcdef",
        "user@example.com",
    )?;
    let options = OpenOptions::new().with_passcode(passcode);
    let dictionary = MdxFile::open_with_options("encrypted.mdx", &options)?;
    println!("{} entries", dictionary.len());
    Ok(())
}
```

Passcode inputs are validated before cloning and `Debug` output is redacted.
Independent tests generate and open header-encrypted, index-encrypted, and
combined encrypted dictionaries end to end.

## Development Checks

```sh
cargo fmt --all -- --check
node --test scripts/corpus/*.test.mjs scripts/corpus/test/*.test.mjs
cargo test --locked --all-targets
cargo test --locked --all-targets --all-features
cargo test --locked --test conformance_v2 --no-default-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --doc --all-features
RUSTDOCFLAGS='-D warnings -D missing_docs -D rustdoc::broken_intra_doc_links' \
  cargo doc --locked --all-features --no-deps
cargo fetch --locked --manifest-path fuzz/Cargo.toml
CARGO_NET_OFFLINE=true cargo +nightly-2026-08-09 fuzz build
cargo package --locked --offline
```

The Node corpus-tooling check is repository-checkout-only. `scripts/corpus/`
and the tracked corpus metadata are intentionally excluded from the published
Rust package.

The committed workflow runs default/all-feature tests on Linux, macOS, and
Windows. Linux also runs formatting, Clippy, strict rustdoc, all seven bounded
AddressSanitizer/coverage-guided fuzz smoke targets, and offline package
verification. It pins cargo-fuzz 0.13.2 and nightly-2026-08-09.

## Corpus Discovery And Local Storage

Dictionary payloads are not stored in this Git repository or Git LFS. A public
download URL does not grant redistribution rights, and the audited direct-file
inventory is too large for a normal source checkout. The repository instead
tracks reviewable metadata and bounded tooling; authorized downloads live under
the ignored `.corpus/` directory.

Discovery is not validation. The referenced AALookup generator intentionally
samples a "reasonable set" in a bounded model-driven browsing run and says it
need not visit every file; the referenced checkout currently has no generated
draft. A deterministic metadata-only audit of `https://mdx.mdict.org/` on
2026-08-10 found 1,254 direct MDX links advertising 40,084,630,153 bytes. Its
sorted directory-listing fingerprint is
`cfa8cdc0e3b1579280398a295e45b7b56fb7c5ee856aa138492cbc72e6eac77d`;
that fingerprint does not authenticate any downloaded payload. In this
context, "all direct MDX files" means only those links in that named source
snapshot—not all dictionaries on the Internet or files hidden in archives.
The corrected row-bounded metadata snapshot is tracked as
[`corpus/mdict-org-2026-08-10.inventory.json`](https://github.com/lonelam/mdictlib/blob/master/corpus/mdict-org-2026-08-10.inventory.json);
the crawl started at `2026-08-10T04:06:35.081Z`, and the file contains no
dictionary payloads.

The complete review, inventory, acquisition, and validation workflow is in
[`corpus/README.md`](https://github.com/lonelam/mdictlib/blob/master/corpus/README.md).
It requires unapproved entries to remain visible and unsupported, encrypted,
corrupt, unavailable, and successfully validated artifacts to be classified
instead of silently treating only parser successes as the corpus.

The repository tooling binds a canonical reviewed selection to the exact
inventory bytes and complete selected source-path/type/URL/size set.
Acquisition accepts only reviewed, credential/query-free HTTPS targets with
connections pinned to validated public DNS answers and same-origin redirects.
Its binary-identity-pinned bootstrap observation is an isolated metadata-open
and entry-count check, not payload validation. Promotion binds the reviewed
lock and complete selection outcome report as a verifiable pair. Canonical full
validation later audits one locked artifact per bounded subprocess and writes
an exact catalog-, denominator-, and runner-bound outcome set; a logical-audit
TSV is produced only if every locked artifact succeeds. Recording self-observed
logical baselines requires that matching successful ledger and TSV; a separate
chain check exactly re-derives the logical lock from the pre-baseline lock and
those two evidence files.

The 2026-08-10 acquisition downloaded all 1,254 selected MDX objects, exactly
40,084,630,153 bytes, with no acquisition error. The reviewed bootstrap lock
contains the 792 files that completed metadata open/count: 37,377,272,230 bytes
and 89,051,220 declared physical entries. Its complete outcome report retains
the other 462 files: 453 non-v2 files, six v2 files rejected while decoding key
summaries, and three truncated record sections. The exact tracked evidence is
the [reviewed lock](https://github.com/lonelam/mdictlib/blob/master/corpus/catalog.lock.json)
and [acquisition outcomes](https://github.com/lonelam/mdictlib/blob/master/corpus/mdict-org-2026-08-10.acquisition-outcomes.json).

The isolated exhaustive run completed 757 whole artifacts, covering
27,098,834,819 bytes and 78,368,836 entries. The other 35 artifacts stopped at
their first recorded error; they contain 10,278,437,411 bytes and declare
10,682,384 entries, which were therefore not all traversed. The failures were
17 GBK and ten UTF-8 record-decode errors, two GBK and one UTF-8 key-decode
errors, two zlib stream errors, two zlib ADLER32 mismatches, and one key-summary
boundary mismatch. The tracked
[exhaustive outcome ledger](https://github.com/lonelam/mdictlib/blob/master/corpus/mdict-org-2026-08-10.exhaustive-outcomes.json)
is complete for the 792-artifact denominator. Because the run was not a
complete success, it produced no logical-audit TSV or logical-baseline lock.
One coherent real dictionary using the narrowly supported legacy v2
keyword-index layout completed all 213,587 entries.
These are source-data failures at the strict parser boundary; follow-up
forensics identified no parser change warranted. The results are
`mdictlib`-self-observed regression evidence, not independent proof of parser
correctness.

## Reproducible Authorized-Corpus Checks

Set `MDICT_CORPUS_DIR` to an authorized corpus directory and put
`mdictlib-corpus.tsv` at its root. Start from
[`tests/corpus-manifest.example.tsv`](tests/corpus-manifest.example.tsv).
Version 1 rows contain:

```text
path    kind    bytes    sha256    entries
```

Version 2 adds two independently optional logical baselines:

```text
path    kind    bytes    sha256    entries    key_sha256    payload_sha256
```

Paths are normalized `/`-separated paths relative to the corpus root. `kind` is
`mdx` or `mdd`. Record byte counts, SHA-256 values, and entry counts
independently of the parser under test where possible. `key_sha256` and
`payload_sha256` are exact hashes emitted by the exhaustive regression route;
when present, subsequent runs must match them. A self-observed entry count or
logical hash is a reproducibility snapshot, not independent proof of parser
correctness, and its basis must remain documented in the reviewed catalog.

```sh
export MDICT_CORPUS_DIR=/absolute/path/to/authorized-corpus
cargo test --locked --release --all-features --test local_corpus -- --ignored --nocapture
cargo test --locked --release --all-features --test local_sample -- --ignored --nocapture
cargo test --locked --release --all-features --test local_lookup_regression -- --ignored --nocapture
```

Explicit runs fail with setup instructions if the environment, manifest, or a
declared asset is absent or wrong. The lookup regression checks every physical
row, raw lookup, duplicate ordering, ordinal identity, and payload/span route.
It checks every per-row logical hash supplied by a version 2 manifest. For the
recorded version 1 release manifest, it also retains the deidentified exact key
and record/resource fallback hashes keyed by that manifest digest.

Those direct tests remain useful for an authorized corpus, but the canonical
repository-checkout full workflow linked above additionally provides
per-artifact process isolation, timeouts, executable identity checks, and a
lock-denominator-bound outcome report.

CI tests the corpus tooling against bounded local fixtures and keeps the Rust
corpus suites ignored by default. Full remote acquisition and exhaustive
validation are deliberate manual or self-hosted operations because they need
authorization review and substantial network, disk, and runtime budgets.

## Measurements

The release harness consumes the same verified manifest:

```sh
export MDICT_CORPUS_DIR=/absolute/path/to/authorized-corpus
cargo run --locked --release --all-features --example bench_local > benchmark.tsv
```

It reports open-to-first-lookup, metadata open, locator construction, warm
lookup distributions, full key scans, sequential and ordinal MDX payloads, MDD
streaming and materialization, concurrent first lookup, deterministic hashes,
and accounted memory. Optional controls are `MDICT_BENCH_FILTER`,
`MDICT_BENCH_WARM_RUNS`, and `MDICT_BENCH_THREADS`.

The checked-in `0.1.0` release evidence covers seven private v2 files,
3.61 GB, and 804,572 physical entries, including exhaustive raw lookup and
ordinal/payload round trips. Exact commands, hashes, timings, memory accounting,
and externally measured peak RSS are recorded in the repository at
`.codex/benchmarks/2026-08-10-macos-arm64.md`.

## License

MIT. See [LICENSE](LICENSE).
