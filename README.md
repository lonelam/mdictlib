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

- MDict major version 2 MDX and MDD sections
- UTF-8, UTF-16LE, GBK/GB2312, GB18030, and Big5 text decoding
- uncompressed and zlib blocks
- optional pure-Rust LZO through the `lzo` feature
- keyword-index encryption
- passcode-protected keyword-header encryption
- lazy iteration, physical-ordinal access, and duplicate-aware lookup

Independent full-file tests cover every listed encoding and compression path,
both encryption modes, multi-block boundaries, duplicate keys, corruption, and
hostile declarations. Version 1.x/future-major layouts, writing, HTML/style
processing, resource extraction policy, multi-volume discovery, prefix/fuzzy
search, mmap, and persistent sidecars are out of scope.

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

The committed workflow runs default/all-feature tests on Linux, macOS, and
Windows. Linux also runs formatting, Clippy, strict rustdoc, all seven bounded
AddressSanitizer/coverage-guided fuzz smoke targets, and offline package
verification. It pins cargo-fuzz 0.13.2 and nightly-2026-08-09.

## Reproducible Private-Corpus Checks

Private dictionary bytes are not stored here. Set `MDICT_CORPUS_DIR` to an
authorized corpus directory and put `mdictlib-corpus.tsv` at its root. Start
from [`tests/corpus-manifest.example.tsv`](tests/corpus-manifest.example.tsv).
Rows contain:

```text
path    kind    bytes    sha256    entries
```

Paths are normalized `/`-separated paths relative to the corpus root. `kind` is
`mdx` or `mdd`. Record byte counts, SHA-256 values, and entry counts
independently of the parser under test.

```sh
export MDICT_CORPUS_DIR=/absolute/path/to/authorized-corpus
cargo test --locked --release --all-features --test local_corpus -- --ignored --nocapture
cargo test --locked --release --all-features --test local_sample -- --ignored --nocapture
cargo test --locked --release --all-features --test local_lookup_regression -- --ignored --nocapture
```

Explicit runs fail with setup instructions if the environment, manifest, or a
declared asset is absent or wrong. The lookup regression checks every physical
row, raw lookup, duplicate ordering, ordinal identity, and payload/span route.
For the recorded release manifest, it also checks deidentified exact key and
record/resource hashes keyed by the manifest digest.

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
