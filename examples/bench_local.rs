#[path = "../tests/support/corpus.rs"]
mod corpus;

use std::env;
use std::error::Error;
use std::hint::black_box;
use std::io::{self, Write};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use mdictlib::{KeyEntry, KeyOrdinal, MddFile, MdxFile, MemoryUsage};

use corpus::{Corpus, CorpusEntry, CorpusKind, Sha256, hex};

type AnyError = Box<dyn Error + Send + Sync>;

const WARM_RUNS_ENV: &str = "MDICT_BENCH_WARM_RUNS";
const THREADS_ENV: &str = "MDICT_BENCH_THREADS";
const FILTER_ENV: &str = "MDICT_BENCH_FILTER";

fn main() -> Result<(), AnyError> {
    let corpus = Corpus::load_from_env().map_err(io::Error::other)?;
    let warm_runs = env_usize(WARM_RUNS_ENV, 100)?;
    let default_threads = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4)
        .clamp(1, 8);
    let threads = env_usize(THREADS_ENV, default_threads)?;
    let filter = env::var(FILTER_ENV).ok();

    let selected = corpus
        .entries()
        .iter()
        .filter(|entry| {
            filter
                .as_ref()
                .is_none_or(|needle| entry.manifest_path().contains(needle))
        })
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(io::Error::other(format!("{FILTER_ENV} selected no manifest entries")).into());
    }

    println!("# mdictlib-corpus-benchmark-v1");
    println!("# crate_version={}", env!("CARGO_PKG_VERSION"));
    println!("# platform={}-{}", env::consts::OS, env::consts::ARCH);
    println!("# warm_runs={warm_runs}");
    println!("# concurrent_threads={threads}");
    println!("kind\tpath\tmetric\tvalue\tunit");

    for entry in selected {
        let path = corpus.path(entry);
        fact(entry, "file.bytes", entry.expected_bytes(), "bytes");
        fact(entry, "file.entries", entry.expected_entries(), "entries");
        text_fact(entry, "file.sha256", &entry.expected_sha256_hex(), "hex");
        match entry.kind() {
            CorpusKind::Mdx => benchmark_mdx(entry, &path, warm_runs, threads)?,
            CorpusKind::Mdd => benchmark_mdd(entry, &path, warm_runs, threads)?,
        }
    }

    Ok(())
}

fn benchmark_mdx(
    manifest: &CorpusEntry,
    path: &std::path::Path,
    warm_runs: usize,
    threads: usize,
) -> Result<(), AnyError> {
    let query = representative_mdx_query(path, manifest.expected_entries())?;

    if let Some(query) = query.as_deref() {
        let started = Instant::now();
        let cold = MdxFile::open(path)?;
        black_box(
            cold.lookup(query)?.ok_or_else(|| {
                io::Error::other(format!("cold raw lookup {query:?} returned None"))
            })?,
        );
        duration_fact(manifest, "lookup.cold_open_to_payload", started.elapsed());
    }

    let started = Instant::now();
    let dictionary = MdxFile::open(path)?;
    duration_fact(manifest, "open", started.elapsed());
    require_count(manifest, dictionary.len())?;

    if let Some(query) = query.as_deref() {
        let started = Instant::now();
        let matches = dictionary.locate(query)?;
        black_box(matches.as_ref().ok_or_else(|| {
            io::Error::other(format!("representative locate {query:?} returned None"))
        })?);
        duration_fact(manifest, "locator.build", started.elapsed());

        let started = Instant::now();
        let first = dictionary.lookup(query)?.ok_or_else(|| {
            io::Error::other(format!("representative raw lookup {query:?} returned None"))
        })?;
        black_box(first);
        duration_fact(manifest, "lookup.first_after_locator", started.elapsed());

        let warm = measure(warm_runs, || {
            let hit = dictionary.lookup(query)?.ok_or_else(|| {
                io::Error::other(format!("warm raw lookup {query:?} returned None"))
            })?;
            black_box(hit);
            Ok::<_, AnyError>(())
        })?;
        distribution_facts(manifest, "lookup.warm", &warm);
    }

    let started = Instant::now();
    let mut key_digest = Sha256::new();
    let mut key_count = 0u64;
    for key in dictionary.keys() {
        digest_key(&mut key_digest, &key?);
        key_count += 1;
    }
    require_count(manifest, key_count)?;
    duration_fact(manifest, "keys.full_scan", started.elapsed());
    text_fact(manifest, "keys.sha256", &hex(&key_digest.finish()), "hex");

    let started = Instant::now();
    let mut sequential_digest = Sha256::new();
    let mut sequential_count = 0u64;
    for entry in dictionary.entries() {
        let entry = entry?;
        digest_key(&mut sequential_digest, entry.key_entry());
        digest_bytes(&mut sequential_digest, entry.text().as_bytes());
        sequential_count += 1;
    }
    require_count(manifest, sequential_count)?;
    duration_fact(manifest, "records.sequential_scan", started.elapsed());
    let sequential_digest = sequential_digest.finish();
    text_fact(
        manifest,
        "records.sequential_sha256",
        &hex(&sequential_digest),
        "hex",
    );

    let started = Instant::now();
    let mut ordinal_digest = Sha256::new();
    for ordinal in 0..dictionary.len() {
        let entry = dictionary
            .entry_at(KeyOrdinal::new(ordinal))?
            .ok_or_else(|| io::Error::other(format!("entry_at({ordinal}) returned None")))?;
        digest_key(&mut ordinal_digest, entry.key_entry());
        digest_bytes(&mut ordinal_digest, entry.text().as_bytes());
    }
    duration_fact(manifest, "records.ordinal_roundtrip", started.elapsed());
    let ordinal_digest = ordinal_digest.finish();
    text_fact(
        manifest,
        "records.ordinal_sha256",
        &hex(&ordinal_digest),
        "hex",
    );
    if ordinal_digest != sequential_digest {
        return Err(io::Error::other("MDX sequential and ordinal record hashes differ").into());
    }

    if let Some(query) = query {
        let elapsed = concurrent_mdx_lookup(path, query, threads)?;
        duration_fact(manifest, "lookup.concurrent_first", elapsed);
        fact(manifest, "lookup.concurrent_threads", threads, "threads");
    }
    memory_facts(manifest, dictionary.memory_usage()?);
    Ok(())
}

fn benchmark_mdd(
    manifest: &CorpusEntry,
    path: &std::path::Path,
    warm_runs: usize,
    threads: usize,
) -> Result<(), AnyError> {
    let representative = representative_mdd_key(path, manifest.expected_entries())?;

    if let Some((_, query)) = representative.as_ref() {
        let started = Instant::now();
        let cold = MddFile::open(path)?;
        black_box(cold.lookup_span(query)?.ok_or_else(|| {
            io::Error::other(format!("cold raw span lookup {query:?} returned None"))
        })?);
        duration_fact(manifest, "lookup.cold_open_to_span", started.elapsed());
    }

    let started = Instant::now();
    let dictionary = MddFile::open(path)?;
    duration_fact(manifest, "open", started.elapsed());
    require_count(manifest, dictionary.len())?;

    if let Some((_, query)) = representative.as_ref() {
        let started = Instant::now();
        let matches = dictionary.locate(query)?;
        black_box(matches.as_ref().ok_or_else(|| {
            io::Error::other(format!("representative locate {query:?} returned None"))
        })?);
        duration_fact(manifest, "locator.build", started.elapsed());

        let started = Instant::now();
        let first = dictionary.lookup_span(query)?.ok_or_else(|| {
            io::Error::other(format!("representative raw lookup {query:?} returned None"))
        })?;
        black_box(first);
        duration_fact(manifest, "lookup.first_after_locator", started.elapsed());

        let warm = measure(warm_runs, || {
            let hit = dictionary.lookup_span(query)?.ok_or_else(|| {
                io::Error::other(format!("warm raw lookup {query:?} returned None"))
            })?;
            black_box(hit);
            Ok::<_, AnyError>(())
        })?;
        distribution_facts(manifest, "lookup.warm", &warm);
    }

    let started = Instant::now();
    let mut key_digest = Sha256::new();
    let mut key_count = 0u64;
    for key in dictionary.keys() {
        digest_key(&mut key_digest, &key?);
        key_count += 1;
    }
    require_count(manifest, key_count)?;
    duration_fact(manifest, "keys.full_scan", started.elapsed());
    text_fact(manifest, "keys.sha256", &hex(&key_digest.finish()), "hex");

    let started = Instant::now();
    let mut stream_digest = DigestWriter::new();
    for ordinal in 0..dictionary.len() {
        let span = dictionary
            .span_at(KeyOrdinal::new(ordinal))?
            .ok_or_else(|| io::Error::other(format!("span_at({ordinal}) returned None")))?;
        digest_key(stream_digest.digest_mut(), span.key_entry());
        stream_digest.digest_mut().update(&span.len().to_be_bytes());
        let copied = span.copy_to(&mut stream_digest)?;
        if copied != span.len() {
            return Err(io::Error::other(format!(
                "span_at({ordinal}) copied {copied} bytes; expected {}",
                span.len()
            ))
            .into());
        }
    }
    duration_fact(manifest, "resources.streaming_scan", started.elapsed());
    let streaming_digest = stream_digest.finish();
    text_fact(
        manifest,
        "resources.streaming_sha256",
        &hex(&streaming_digest),
        "hex",
    );

    let started = Instant::now();
    let mut materialized_digest = Sha256::new();
    for resource in dictionary.resources() {
        let resource = resource?;
        digest_key(&mut materialized_digest, resource.key_entry());
        materialized_digest.update(
            &u64::try_from(resource.bytes().len())
                .map_err(|_| io::Error::other("resource length exceeds u64"))?
                .to_be_bytes(),
        );
        materialized_digest.update(resource.bytes());
    }
    duration_fact(manifest, "resources.materialized_scan", started.elapsed());
    let materialized_digest = materialized_digest.finish();
    text_fact(
        manifest,
        "resources.materialized_sha256",
        &hex(&materialized_digest),
        "hex",
    );
    if materialized_digest != streaming_digest {
        return Err(io::Error::other("MDD streaming and materialized hashes differ").into());
    }

    if let Some((ordinal, query)) = representative {
        measure_mdd_resource_modes(manifest, path, ordinal)?;
        let elapsed = concurrent_mdd_lookup(path, query, threads)?;
        duration_fact(manifest, "lookup.concurrent_first", elapsed);
        fact(manifest, "lookup.concurrent_threads", threads, "threads");
    }
    memory_facts(manifest, dictionary.memory_usage()?);
    Ok(())
}

fn representative_mdx_query(
    path: &std::path::Path,
    count: u64,
) -> Result<Option<String>, AnyError> {
    if count == 0 {
        return Ok(None);
    }
    let dictionary = MdxFile::open(path)?;
    let key = dictionary
        .key_at(KeyOrdinal::new(count / 2))?
        .ok_or_else(|| io::Error::other("representative MDX ordinal returned None"))?;
    Ok(Some(key.into_key()))
}

fn representative_mdd_key(
    path: &std::path::Path,
    count: u64,
) -> Result<Option<(KeyOrdinal, String)>, AnyError> {
    if count == 0 {
        return Ok(None);
    }
    let dictionary = MddFile::open(path)?;
    let ordinal = KeyOrdinal::new(count / 2);
    let key = dictionary
        .key_at(ordinal)?
        .ok_or_else(|| io::Error::other("representative MDD ordinal returned None"))?;
    Ok(Some((ordinal, key.into_key())))
}

fn measure_mdd_resource_modes(
    manifest: &CorpusEntry,
    path: &std::path::Path,
    ordinal: KeyOrdinal,
) -> Result<(), AnyError> {
    let streaming_file = MddFile::open(path)?;
    let streaming_span = streaming_file
        .span_at(ordinal)?
        .ok_or_else(|| io::Error::other("representative streaming span returned None"))?;
    let mut streaming_digest = DigestWriter::new();
    let started = Instant::now();
    let copied = streaming_span.copy_to(&mut streaming_digest)?;
    duration_fact(manifest, "resource.sample_stream", started.elapsed());
    fact(manifest, "resource.sample_bytes", copied, "bytes");
    let streaming_digest = streaming_digest.finish();
    text_fact(
        manifest,
        "resource.sample_stream_sha256",
        &hex(&streaming_digest),
        "hex",
    );

    let materializing_file = MddFile::open(path)?;
    let materializing_span = materializing_file
        .span_at(ordinal)?
        .ok_or_else(|| io::Error::other("representative materializing span returned None"))?;
    let started = Instant::now();
    let resource = materializing_span.read()?;
    duration_fact(manifest, "resource.sample_materialize", started.elapsed());
    let mut digest = Sha256::new();
    digest.update(resource.bytes());
    let materialized_digest = digest.finish();
    text_fact(
        manifest,
        "resource.sample_materialize_sha256",
        &hex(&materialized_digest),
        "hex",
    );
    if materialized_digest != streaming_digest {
        return Err(io::Error::other(
            "streamed and materialized representative resource hashes differ",
        )
        .into());
    }
    Ok(())
}

fn concurrent_mdx_lookup(
    path: &std::path::Path,
    query: String,
    threads: usize,
) -> Result<Duration, AnyError> {
    let dictionary = Arc::new(MdxFile::open(path)?);
    let barrier = Arc::new(Barrier::new(threads + 1));
    let mut handles = Vec::with_capacity(threads);
    for _ in 0..threads {
        let dictionary = Arc::clone(&dictionary);
        let barrier = Arc::clone(&barrier);
        let query = query.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            dictionary.lookup(&query).map(|entry| entry.is_some())
        }));
    }

    let started = Instant::now();
    barrier.wait();
    for handle in handles {
        let found = handle
            .join()
            .map_err(|_| io::Error::other("concurrent MDX lookup thread panicked"))??;
        if !found {
            return Err(io::Error::other("concurrent MDX lookup returned None").into());
        }
    }
    Ok(started.elapsed())
}

fn concurrent_mdd_lookup(
    path: &std::path::Path,
    query: String,
    threads: usize,
) -> Result<Duration, AnyError> {
    let dictionary = Arc::new(MddFile::open(path)?);
    let barrier = Arc::new(Barrier::new(threads + 1));
    let mut handles = Vec::with_capacity(threads);
    for _ in 0..threads {
        let dictionary = Arc::clone(&dictionary);
        let barrier = Arc::clone(&barrier);
        let query = query.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            dictionary.lookup_span(&query).map(|span| span.is_some())
        }));
    }

    let started = Instant::now();
    barrier.wait();
    for handle in handles {
        let found = handle
            .join()
            .map_err(|_| io::Error::other("concurrent MDD lookup thread panicked"))??;
        if !found {
            return Err(io::Error::other("concurrent MDD lookup returned None").into());
        }
    }
    Ok(started.elapsed())
}

fn measure(
    iterations: usize,
    mut operation: impl FnMut() -> Result<(), AnyError>,
) -> Result<Vec<Duration>, AnyError> {
    let mut durations = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        operation()?;
        durations.push(started.elapsed());
    }
    Ok(durations)
}

fn distribution_facts(manifest: &CorpusEntry, prefix: &str, values: &[Duration]) {
    let mut nanos = values.iter().map(Duration::as_nanos).collect::<Vec<_>>();
    nanos.sort_unstable();
    fact(manifest, &format!("{prefix}.count"), nanos.len(), "runs");
    fact(
        manifest,
        &format!("{prefix}.p50"),
        percentile(&nanos, 50),
        "ns",
    );
    fact(
        manifest,
        &format!("{prefix}.p95"),
        percentile(&nanos, 95),
        "ns",
    );
    fact(
        manifest,
        &format!("{prefix}.p99"),
        percentile(&nanos, 99),
        "ns",
    );
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    sorted[(sorted.len() - 1) * percentile / 100]
}

fn digest_key(digest: &mut Sha256, key: &KeyEntry) {
    digest.update(&key.ordinal().get().to_be_bytes());
    digest_bytes(digest, key.key().as_bytes());
}

fn digest_bytes(digest: &mut Sha256, bytes: &[u8]) {
    digest.update(&(bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

struct DigestWriter {
    digest: Sha256,
}

impl DigestWriter {
    const fn new() -> Self {
        Self {
            digest: Sha256::new(),
        }
    }

    fn digest_mut(&mut self) -> &mut Sha256 {
        &mut self.digest
    }

    fn finish(self) -> [u8; 32] {
        self.digest.finish()
    }
}

impl Write for DigestWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.digest.update(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn require_count(manifest: &CorpusEntry, actual: u64) -> Result<(), AnyError> {
    if actual != manifest.expected_entries() {
        return Err(io::Error::other(format!(
            "{} contains {actual} entries; manifest declares {}",
            manifest.relative_path().display(),
            manifest.expected_entries()
        ))
        .into());
    }
    Ok(())
}

fn env_usize(name: &str, default: usize) -> Result<usize, AnyError> {
    let Some(value) = env::var_os(name) else {
        return Ok(default);
    };
    let value = value
        .into_string()
        .map_err(|_| io::Error::other(format!("{name} is not valid UTF-8")))?;
    let parsed = value
        .parse::<usize>()
        .map_err(|_| io::Error::other(format!("{name} must be a positive integer")))?;
    if parsed == 0 {
        return Err(io::Error::other(format!("{name} must be greater than zero")).into());
    }
    Ok(parsed)
}

fn duration_fact(manifest: &CorpusEntry, metric: &str, duration: Duration) {
    fact(manifest, metric, duration.as_nanos(), "ns");
}

fn fact(manifest: &CorpusEntry, metric: &str, value: impl std::fmt::Display, unit: &str) {
    println!(
        "{}\t{}\t{metric}\t{value}\t{unit}",
        manifest.kind().as_str(),
        manifest.manifest_path()
    );
}

fn text_fact(manifest: &CorpusEntry, metric: &str, value: &str, unit: &str) {
    fact(manifest, metric, sanitize_field(value), unit);
}

fn memory_facts(manifest: &CorpusEntry, usage: MemoryUsage) {
    fact(manifest, "memory.current", usage.current_bytes(), "bytes");
    fact(manifest, "memory.peak", usage.peak_bytes(), "bytes");
    fact(manifest, "memory.metadata", usage.metadata_bytes(), "bytes");
    fact(manifest, "memory.locator", usage.locator_bytes(), "bytes");
    fact(
        manifest,
        "memory.key_cache",
        usage.key_cache_bytes(),
        "bytes",
    );
    fact(
        manifest,
        "memory.record_cache",
        usage.record_cache_bytes(),
        "bytes",
    );
}

fn sanitize_field(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}
