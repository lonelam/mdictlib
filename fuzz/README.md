# Fuzz Targets

Install the pinned runner and nightly toolchain used by CI:

```bash
rustup toolchain install nightly-2026-08-09 --profile minimal
cargo install cargo-fuzz --version 0.13.2 --locked
```

Fetch the committed dependency graph, then build every target with Address
Sanitizer and libFuzzer coverage instrumentation:

```bash
cargo fetch --locked --manifest-path fuzz/Cargo.toml
CARGO_NET_OFFLINE=true cargo +nightly-2026-08-09 fuzz build
```

Run a target from the repository root:

```bash
cargo +nightly-2026-08-09 fuzz run header_bytes
cargo +nightly-2026-08-09 fuzz run compression_block
cargo +nightly-2026-08-09 fuzz run whole_file -- \
  -max_len=1048576 -rss_limit_mb=1024
cargo +nightly-2026-08-09 fuzz run key_index
cargo +nightly-2026-08-09 fuzz run key_block
cargo +nightly-2026-08-09 fuzz run record_index
cargo +nightly-2026-08-09 fuzz run record_span
```

The targets cover:

- `header_bytes`: both top-level header tags from arbitrary in-memory bytes
- `compression_block`: none, zlib, and LZO block decoding with several bounded
  declared output lengths
- `whole_file`: arbitrary bytes opened as both MDX and MDD, followed by bounded
  key scans and locator calls when the declared entry count is small
- `key_index`: checksum-preserving mutations of a valid multi-block keyword
  index for both MDX and MDD, plus occasional block-envelope corruption
- `key_block`: checksum-preserving lazy key-block mutations for both container
  kinds, followed by bounded key, ordinal, and locator routes
- `record_index`: focused record-header and record-index mutations through the
  shared MDD record core, followed by bounded span mapping and streaming
- `record_span`: key record-offset and record-block mutations followed by
  bounded MDD `span_at`, `copy_to`, and materialization routes

The structured targets use the independent encoder in `fuzz/support/` to begin
with valid, uncompressed, multi-key-block and multi-record-block files. Payload
mutations repair ADLER32 when the goal is to reach deeper parsing; a small
fraction also corrupts block envelopes to retain negative coverage. Empty input
is an asserted valid-fixture control.

## Bounds And Limitations

- `whole_file` truncates each fuzzer input to 1 MiB. Public-facade targets use
  an explicit fuzz policy capped at 256 KiB of header XML, 1 MiB indexes and
  compressed blocks, 2 MiB decoded blocks and locator storage, 64 KiB
  materialized records, and 16 MiB aggregate working memory.
- All targets scan at most 16 rows, only build a locator for at most 16
  declared entries, and stream or materialize a mutated MDD span only when its
  logical length is at most 64 KiB. Keep a libFuzzer RSS limit in long-running
  jobs as a second process-level bound.
- Whole-file and structured public-facade targets use temporary files because
  the shipped reader API is path-backed. They are slower than the two narrow
  in-memory adapters.
- Structured fuzz fixtures exercise UTF-8 MDX keys, UTF-16LE MDD keys, and
  uncompressed section blocks. Compression algorithms are fuzzed by the focused
  block target; the main conformance suite separately opens complete zlib and
  LZO MDX/MDD files across every block class.
- This harness does not embed passcodes or private dictionary bytes. The main
  conformance suite independently generates and verifies encrypted keyword
  header/index combinations end to end.

Cargo-fuzz enables mdictlib's doc-hidden adapter through its checked
`cfg(fuzzing)`, while the fuzz crate enables the optional `lzo` feature. No
fuzz-only Cargo feature is published. Parser implementation modules remain
private; deep section coverage goes through the normal public file facades. CI
pins `cargo-fuzz` 0.13.2 and nightly-2026-08-09, runs every target for 32
coverage-guided iterations, and fails if the committed fuzz lockfile changes.
