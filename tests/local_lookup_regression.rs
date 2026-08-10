#[path = "support/corpus.rs"]
mod corpus;

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use mdictlib::{KeyEntry, MatchBasis, MddFile, MdxFile};

use corpus::{Corpus, CorpusKind, Sha256, hex};

const RELEASE_MANIFEST_SHA256: &str =
    "f4a61bb746601fae3c46d0cf80f2c49426be8c3cd5414a42d2b0942a3f0672f9";
const AUDIT_OUTPUT_ENV: &str = "MDICT_CORPUS_AUDIT_TSV";
const AUDIT_HEADER: &str = "path\tkind\tentries\tkey_sha256\tpayload_sha256\n";

struct LogicalBaseline {
    kind: CorpusKind,
    entries: u64,
    key_sha256: &'static str,
    payload_sha256: &'static str,
}

const RELEASE_LOGICAL_BASELINES: &[LogicalBaseline] = &[
    LogicalBaseline {
        kind: CorpusKind::Mdd,
        entries: 182_430,
        key_sha256: "9bb651242cff051de4a8817c7ee092b161031b17d27dd3533ee66256f729e5ab",
        payload_sha256: "032c58f178654d79d80c87a140c7df9317462b78af68dd85a93b48227621895a",
    },
    LogicalBaseline {
        kind: CorpusKind::Mdx,
        entries: 52_749,
        key_sha256: "9a5bfb02a915cdc9d2266e6259c34a7c547ab76a18858312f71a4ba25ec3c532",
        payload_sha256: "bedd7a30d61c032a7b5ef7fd71a93190ec5169f57d55cb991d2af21d49ea761c",
    },
    LogicalBaseline {
        kind: CorpusKind::Mdd,
        entries: 160_806,
        key_sha256: "f9fcc723287971eaad9e01b8726ec10f9d78b33402b5a1bba1453ee3443ae3d2",
        payload_sha256: "3ba238b29b4f273d6fa8dba7ec909f9804eb5ce95089228920282b12c9256daf",
    },
    LogicalBaseline {
        kind: CorpusKind::Mdd,
        entries: 1_873,
        key_sha256: "35e72385055a44e1692c0a17145307db53c935fe8ffea3966977db3fd2ca0f90",
        payload_sha256: "de17a19c3e01e867634790212e3e0b13a4198c005f06b9f127b377b93ae4d981",
    },
    LogicalBaseline {
        kind: CorpusKind::Mdd,
        entries: 112_789,
        key_sha256: "406048fb58077ba3cec28077b8939ea6038b3d8c77bee05cb6e1408314274371",
        payload_sha256: "efb43555aba8c99107669db2386c4c7a5e4f0ad09577fe9fed70ad64eef26fa1",
    },
    LogicalBaseline {
        kind: CorpusKind::Mdd,
        entries: 48,
        key_sha256: "d82543a8685d33453dc5f1ebb75695ed7477611cac60a50c8466b8e061e395c7",
        payload_sha256: "a7ff8bcdf454f11bf996af138ebcfee1b56164a09081bf1820ad4bb09ec79169",
    },
    LogicalBaseline {
        kind: CorpusKind::Mdx,
        entries: 293_877,
        key_sha256: "1a31acb2dc0f9ba1332952005376f3f7726ef49d0b6b2f3ff11411c25bfab07c",
        payload_sha256: "21085551771ae7b371226a7df720133b266a6aaaa8fca7cab9529d927a8a56f0",
    },
];

#[test]
fn audit_output_is_installed_as_one_complete_file() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = env::temp_dir().join(format!(
        "mdictlib-logical-audit-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let destination = root.join("audit.tsv");
    let contents = b"path\tkind\tentries\tkey_sha256\tpayload_sha256\n";

    write_audit_atomically(&destination, contents).unwrap();
    assert_eq!(fs::read(&destination).unwrap(), contents);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);

    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires the manifest-verified private corpus selected by MDICT_CORPUS_DIR"]
fn every_physical_key_round_trips_by_ordinal_and_raw_lookup() {
    let corpus = Corpus::load_from_env_or_panic();
    let release_baselines = (hex(corpus.manifest_sha256()) == RELEASE_MANIFEST_SHA256)
        .then_some(RELEASE_LOGICAL_BASELINES);

    print!("{AUDIT_HEADER}");
    let mut audit = String::from(AUDIT_HEADER);
    for (index, entry) in corpus.entries().iter().enumerate() {
        let path = corpus.path(entry);
        let (key_sha256, payload_sha256) = match entry.kind() {
            CorpusKind::Mdx => audit_mdx(&path, entry.expected_entries()),
            CorpusKind::Mdd => audit_mdd(&path, entry.expected_entries()),
        };
        let row = format!(
            "{}\t{}\t{}\t{}\t{}\n",
            entry.manifest_path(),
            entry.kind().as_str(),
            entry.expected_entries(),
            hex(&key_sha256),
            hex(&payload_sha256)
        );
        print!("{row}");
        audit.push_str(&row);
        let release_baseline = release_baselines.map(|baselines| {
            let baseline = baselines
                .get(index)
                .unwrap_or_else(|| panic!("release baseline is missing corpus row {index}"));
            assert_eq!(baseline.kind, entry.kind());
            assert_eq!(baseline.entries, entry.expected_entries());
            baseline
        });
        assert_logical_digest(
            "key_sha256",
            &key_sha256,
            entry.expected_key_sha256(),
            release_baseline.map(|baseline| baseline.key_sha256),
            entry.relative_path(),
        );
        assert_logical_digest(
            "payload_sha256",
            &payload_sha256,
            entry.expected_payload_sha256(),
            release_baseline.map(|baseline| baseline.payload_sha256),
            entry.relative_path(),
        );
    }
    if let Some(baselines) = release_baselines {
        assert_eq!(baselines.len(), corpus.entries().len());
    }
    if let Some(output) = env::var_os(AUDIT_OUTPUT_ENV) {
        write_audit_atomically(Path::new(&output), audit.as_bytes()).unwrap_or_else(|error| {
            panic!(
                "failed to write {AUDIT_OUTPUT_ENV}={} atomically: {error}",
                Path::new(&output).display()
            )
        });
    }
}

fn write_audit_atomically(destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("mdictlib-corpus-audit.tsv");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));

    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn assert_logical_digest(
    field: &str,
    actual: &[u8; 32],
    manifest_expected: Option<&[u8; 32]>,
    release_fallback: Option<&str>,
    relative_path: &std::path::Path,
) {
    let expected = manifest_expected
        .map(|expected| hex(expected))
        .or_else(|| release_fallback.map(str::to_owned));
    if let Some(expected) = expected {
        assert_eq!(
            hex(actual),
            expected,
            "{field} differs from the corpus baseline for {}",
            relative_path.display()
        );
    }
}

fn audit_mdx(path: &std::path::Path, expected_entries: u64) -> ([u8; 32], [u8; 32]) {
    let dictionary = MdxFile::open(path)
        .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display()));
    assert_eq!(dictionary.len(), expected_entries);

    let mut key_digest = Sha256::new();
    let mut payload_digest = Sha256::new();
    let mut count = 0u64;
    for key in dictionary.keys() {
        let key = key.unwrap_or_else(|error| {
            panic!("failed to iterate keys in {}: {error}", path.display())
        });
        assert_eq!(
            key.ordinal().get(),
            count,
            "non-contiguous physical ordinal in {}",
            path.display()
        );
        digest_key(&mut key_digest, &key);

        let ordinal_key = dictionary
            .key_at(key.ordinal())
            .unwrap_or_else(|error| {
                panic!(
                    "ordinal key lookup {} failed in {}: {error}",
                    key.ordinal().get(),
                    path.display()
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "ordinal key lookup {} returned None in {}",
                    key.ordinal().get(),
                    path.display()
                )
            });
        assert_eq!(
            ordinal_key,
            key,
            "ordinal key mismatch in {}",
            path.display()
        );

        let ordinal_entry = dictionary
            .entry_at(key.ordinal())
            .unwrap_or_else(|error| {
                panic!(
                    "ordinal entry lookup {} failed in {}: {error}",
                    key.ordinal().get(),
                    path.display()
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "ordinal entry lookup {} returned None in {}",
                    key.ordinal().get(),
                    path.display()
                )
            });
        assert_eq!(ordinal_entry.key_entry(), &key);
        digest_key(&mut payload_digest, ordinal_entry.key_entry());
        digest_bytes(&mut payload_digest, ordinal_entry.text().as_bytes());

        let matches = dictionary
            .locate(key.key())
            .unwrap_or_else(|error| {
                panic!(
                    "raw locate {:?} failed in {}: {error}",
                    key.key(),
                    path.display()
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "raw locate {:?} returned None in {}",
                    key.key(),
                    path.display()
                )
            });
        assert_eq!(matches.basis(), MatchBasis::RawExact);
        assert!(
            matches.iter().any(|ordinal| ordinal == key.ordinal()),
            "raw duplicate set omitted ordinal {} in {}",
            key.ordinal().get(),
            path.display()
        );

        let exact = dictionary
            .lookup(key.key())
            .unwrap_or_else(|error| {
                panic!(
                    "raw lookup {:?} failed in {}: {error}",
                    key.key(),
                    path.display()
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "raw lookup {:?} returned None in {}",
                    key.key(),
                    path.display()
                )
            });
        assert_eq!(
            exact.key(),
            key.key(),
            "raw lookup resolved a normalized alternative in {}",
            path.display()
        );
        count += 1;
    }
    assert_eq!(
        count,
        expected_entries,
        "key count mismatch in {}",
        path.display()
    );
    (key_digest.finish(), payload_digest.finish())
}

fn audit_mdd(path: &std::path::Path, expected_entries: u64) -> ([u8; 32], [u8; 32]) {
    let dictionary = MddFile::open(path)
        .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display()));
    assert_eq!(dictionary.len(), expected_entries);

    let mut key_digest = Sha256::new();
    let mut payload_digest = Sha256::new();
    let mut count = 0u64;
    for key in dictionary.keys() {
        let key = key.unwrap_or_else(|error| {
            panic!("failed to iterate keys in {}: {error}", path.display())
        });
        assert_eq!(
            key.ordinal().get(),
            count,
            "non-contiguous physical ordinal in {}",
            path.display()
        );
        digest_key(&mut key_digest, &key);

        let ordinal_key = dictionary
            .key_at(key.ordinal())
            .unwrap_or_else(|error| {
                panic!(
                    "ordinal key lookup {} failed in {}: {error}",
                    key.ordinal().get(),
                    path.display()
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "ordinal key lookup {} returned None in {}",
                    key.ordinal().get(),
                    path.display()
                )
            });
        assert_eq!(
            ordinal_key,
            key,
            "ordinal key mismatch in {}",
            path.display()
        );

        let ordinal_span = dictionary
            .span_at(key.ordinal())
            .unwrap_or_else(|error| {
                panic!(
                    "ordinal span lookup {} failed in {}: {error}",
                    key.ordinal().get(),
                    path.display()
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "ordinal span lookup {} returned None in {}",
                    key.ordinal().get(),
                    path.display()
                )
            });
        assert_eq!(ordinal_span.key_entry(), &key);
        digest_key(&mut payload_digest, ordinal_span.key_entry());
        payload_digest.update(&ordinal_span.len().to_be_bytes());
        let copied = ordinal_span
            .copy_to(&mut DigestWriter(&mut payload_digest))
            .unwrap_or_else(|error| {
                panic!(
                    "ordinal resource stream {} failed in {}: {error}",
                    key.ordinal().get(),
                    path.display()
                )
            });
        assert_eq!(copied, ordinal_span.len());

        let matches = dictionary
            .locate(key.key())
            .unwrap_or_else(|error| {
                panic!(
                    "raw locate {:?} failed in {}: {error}",
                    key.key(),
                    path.display()
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "raw locate {:?} returned None in {}",
                    key.key(),
                    path.display()
                )
            });
        assert_eq!(matches.basis(), MatchBasis::RawExact);
        assert!(
            matches.iter().any(|ordinal| ordinal == key.ordinal()),
            "raw duplicate set omitted ordinal {} in {}",
            key.ordinal().get(),
            path.display()
        );

        let exact = dictionary
            .lookup_span(key.key())
            .unwrap_or_else(|error| {
                panic!(
                    "raw lookup {:?} failed in {}: {error}",
                    key.key(),
                    path.display()
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "raw lookup {:?} returned None in {}",
                    key.key(),
                    path.display()
                )
            });
        assert_eq!(
            exact.key(),
            key.key(),
            "raw lookup resolved a normalized alternative in {}",
            path.display()
        );
        count += 1;
    }
    assert_eq!(
        count,
        expected_entries,
        "key count mismatch in {}",
        path.display()
    );
    (key_digest.finish(), payload_digest.finish())
}

fn digest_key(digest: &mut Sha256, key: &KeyEntry) {
    digest.update(&key.ordinal().get().to_be_bytes());
    digest_bytes(digest, key.key().as_bytes());
}

fn digest_bytes(digest: &mut Sha256, bytes: &[u8]) {
    digest.update(&u64::try_from(bytes.len()).unwrap().to_be_bytes());
    digest.update(bytes);
}

struct DigestWriter<'a>(&'a mut Sha256);

impl Write for DigestWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
