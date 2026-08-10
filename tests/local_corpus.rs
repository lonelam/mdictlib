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
    assert_eq!(corpus.entries()[0].expected_key_sha256(), None);
    assert_eq!(corpus.entries()[0].expected_payload_sha256(), None);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn corpus_manifest_v2_accepts_independently_optional_logical_digests() {
    let root = temporary_corpus_root("v2-logical-digests");
    fs::create_dir(&root).unwrap();
    let mdx_payload = b"mdx identity fixture";
    let mdd_payload = b"mdd identity fixture";
    fs::write(root.join("keys.mdx"), mdx_payload).unwrap();
    fs::write(root.join("payload.mdd"), mdd_payload).unwrap();

    let mdx_sha256 = digest(mdx_payload);
    let mdd_sha256 = digest(mdd_payload);
    let key_sha256 = [0x11; 32];
    let payload_sha256 = [0xee; 32];
    let manifest = format!(
        "path\tkind\tbytes\tsha256\tentries\tkey_sha256\tpayload_sha256\n\
         keys.mdx\tmdx\t{}\t{}\t3\t{}\t\n\
         payload.mdd\tmdd\t{}\t{}\t5\t\t{}\n",
        mdx_payload.len(),
        hex(&mdx_sha256),
        hex(&key_sha256),
        mdd_payload.len(),
        hex(&mdd_sha256),
        hex(&payload_sha256),
    );
    fs::write(root.join(MANIFEST_NAME), manifest).unwrap();

    let corpus = Corpus::load(&root).unwrap();
    assert_eq!(corpus.entries().len(), 2);
    assert_eq!(corpus.entries()[0].expected_key_sha256(), Some(&key_sha256));
    assert_eq!(corpus.entries()[0].expected_payload_sha256(), None);
    assert_eq!(corpus.entries()[1].expected_key_sha256(), None);
    assert_eq!(
        corpus.entries()[1].expected_payload_sha256(),
        Some(&payload_sha256)
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn corpus_manifest_v2_rejects_invalid_logical_digest() {
    let root = temporary_corpus_root("v2-invalid-digest");
    fs::create_dir(&root).unwrap();
    let payload = b"manifest identity fixture";
    fs::write(root.join("sample.mdx"), payload).unwrap();
    let manifest = format!(
        "path\tkind\tbytes\tsha256\tentries\tkey_sha256\tpayload_sha256\n\
         sample.mdx\tmdx\t{}\t{}\t7\tnot-a-digest\t\n",
        payload.len(),
        hex(&digest(payload)),
    );
    fs::write(root.join(MANIFEST_NAME), manifest).unwrap();

    let error = Corpus::load(&root).unwrap_err();
    assert!(error.contains("key_sha256 must contain exactly 64 hexadecimal digits"));

    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn corpus_uses_the_verified_canonical_path_after_a_symlink_swap() {
    use std::os::unix::fs::symlink;

    let root = temporary_corpus_root("canonical-path");
    let verified = root.join("verified");
    let replacement = root.join("replacement");
    fs::create_dir_all(&verified).unwrap();
    fs::create_dir_all(&replacement).unwrap();
    let verified_payload = b"verified dictionary bytes";
    fs::write(verified.join("sample.mdx"), verified_payload).unwrap();
    fs::write(replacement.join("sample.mdx"), b"different bytes").unwrap();
    symlink(&verified, root.join("selected")).unwrap();
    let manifest = format!(
        "path\tkind\tbytes\tsha256\tentries\nselected/sample.mdx\tmdx\t{}\t{}\t1\n",
        verified_payload.len(),
        hex(&digest(verified_payload)),
    );
    fs::write(root.join(MANIFEST_NAME), manifest).unwrap();

    let corpus = Corpus::load(&root).unwrap();
    let entry = &corpus.entries()[0];
    assert_eq!(
        corpus.path(entry),
        verified.join("sample.mdx").canonicalize().unwrap()
    );

    fs::remove_file(root.join("selected")).unwrap();
    symlink(&replacement, root.join("selected")).unwrap();
    assert_eq!(fs::read(corpus.path(entry)).unwrap(), verified_payload);

    fs::remove_dir_all(root).unwrap();
}

fn temporary_corpus_root(label: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("mdictlib-corpus-manifest-{label}-{unique}"))
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(bytes);
    digest.finish()
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
