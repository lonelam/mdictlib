#[path = "support/corpus.rs"]
mod corpus;

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use mdictlib::{MddFile, MdxFile};

use corpus::{Corpus, CorpusKind, MANIFEST_NAME, Sha256, hex};

#[test]
fn corpus_manifest_sha256_matches_standard_vector() {
    let mut digest = Sha256::new();
    digest.update(b"a");
    digest.update(b"bc");
    assert_eq!(
        hex(&digest.finish()),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );

    let mut boundary_digest = Sha256::new();
    boundary_digest.update(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
    assert_eq!(
        hex(&boundary_digest.finish()),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn corpus_manifest_accepts_verified_relative_file() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("mdictlib-corpus-manifest-{unique}"));
    fs::create_dir(&root).unwrap();
    let payload = b"manifest identity fixture";
    fs::write(root.join("sample.mdx"), payload).unwrap();
    let mut digest = Sha256::new();
    digest.update(payload);
    let manifest = format!(
        "path\tkind\tbytes\tsha256\tentries\nsample.mdx\tmdx\t{}\t{}\t7\n",
        payload.len(),
        hex(&digest.finish())
    );
    fs::write(root.join(MANIFEST_NAME), manifest).unwrap();

    let corpus = Corpus::load(&root).unwrap();
    assert_eq!(corpus.entries().len(), 1);
    assert_eq!(corpus.entries()[0].expected_entries(), 7);

    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires the manifest-verified private corpus selected by MDICT_CORPUS_DIR"]
fn every_manifest_dictionary_opens_and_matches_declared_count() {
    let corpus = Corpus::load_from_env_or_panic();

    for entry in corpus.entries() {
        let path = corpus.path(entry);
        match entry.kind() {
            CorpusKind::Mdx => {
                let dictionary = MdxFile::open(&path)
                    .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display()));
                assert_eq!(
                    dictionary.len(),
                    entry.expected_entries(),
                    "entry count differs from the manifest for {}",
                    entry.relative_path().display()
                );
                assert_eq!(
                    dictionary.is_empty(),
                    entry.expected_entries() == 0,
                    "empty-state mismatch for {}",
                    entry.relative_path().display()
                );
                if entry.expected_entries() != 0 {
                    let first = dictionary
                        .entries()
                        .next()
                        .transpose()
                        .unwrap_or_else(|error| {
                            panic!(
                                "failed to decode first MDX entry in {}: {error}",
                                path.display()
                            )
                        })
                        .unwrap_or_else(|| panic!("no MDX entries in {}", path.display()));
                    assert_eq!(first.ordinal().get(), 0);
                }
            }
            CorpusKind::Mdd => {
                let dictionary = MddFile::open(&path)
                    .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display()));
                assert_eq!(
                    dictionary.len(),
                    entry.expected_entries(),
                    "entry count differs from the manifest for {}",
                    entry.relative_path().display()
                );
                assert_eq!(
                    dictionary.is_empty(),
                    entry.expected_entries() == 0,
                    "empty-state mismatch for {}",
                    entry.relative_path().display()
                );
                if entry.expected_entries() != 0 {
                    let first = dictionary
                        .resources()
                        .next()
                        .transpose()
                        .unwrap_or_else(|error| {
                            panic!(
                                "failed to decode first MDD resource in {}: {error}",
                                path.display()
                            )
                        })
                        .unwrap_or_else(|| panic!("no MDD resources in {}", path.display()));
                    assert_eq!(first.ordinal().get(), 0);
                }
            }
        }
    }
}
