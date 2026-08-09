#[path = "support/corpus.rs"]
mod corpus;

use std::io;

use mdictlib::{KeyEntry, KeyOrdinal, MddFile, MdxFile};

use corpus::{Corpus, CorpusKind};

#[test]
#[ignore = "requires the manifest-verified private corpus selected by MDICT_CORPUS_DIR"]
fn representative_ordinals_batch_and_payload_paths_work() {
    let corpus = Corpus::load_from_env_or_panic();

    for entry in corpus.entries() {
        if entry.expected_entries() == 0 {
            continue;
        }
        let path = corpus.path(entry);
        match entry.kind() {
            CorpusKind::Mdx => audit_mdx(&path, entry.expected_entries()),
            CorpusKind::Mdd => audit_mdd(&path, entry.expected_entries()),
        }
    }
}

fn sample_ordinals(len: u64) -> Vec<KeyOrdinal> {
    let mut ordinals = vec![KeyOrdinal::new(0), KeyOrdinal::new(len / 2)];
    if len > 1 {
        ordinals.push(KeyOrdinal::new(len - 1));
    }
    ordinals.dedup();
    ordinals
}

fn audit_mdx(path: &std::path::Path, expected_entries: u64) {
    let dictionary = MdxFile::open(path).unwrap();
    assert_eq!(dictionary.len(), expected_entries);
    let ordinals = sample_ordinals(dictionary.len());
    assert_batch_contract_mdx(&dictionary, &ordinals);

    for ordinal in ordinals {
        let key = dictionary.key_at(ordinal).unwrap().unwrap();
        let entry = dictionary.entry_at(ordinal).unwrap().unwrap();
        assert_eq!(entry.key_entry(), &key);
    }
}

fn assert_batch_contract_mdx(dictionary: &MdxFile, ordinals: &[KeyOrdinal]) {
    let mut requests = ordinals.iter().rev().copied().collect::<Vec<_>>();
    requests.push(ordinals[0]);
    requests.push(KeyOrdinal::new(dictionary.len()));
    let actual = dictionary.keys_at(&requests).unwrap();
    let expected = requests
        .iter()
        .map(|ordinal| dictionary.key_at(*ordinal).unwrap())
        .collect::<Vec<Option<KeyEntry>>>();
    assert_eq!(actual, expected);
}

fn audit_mdd(path: &std::path::Path, expected_entries: u64) {
    let dictionary = MddFile::open(path).unwrap();
    assert_eq!(dictionary.len(), expected_entries);
    let ordinals = sample_ordinals(dictionary.len());
    assert_batch_contract_mdd(&dictionary, &ordinals);

    for ordinal in ordinals {
        let key = dictionary.key_at(ordinal).unwrap().unwrap();
        let span = dictionary.span_at(ordinal).unwrap().unwrap();
        assert_eq!(span.key_entry(), &key);
        let copied = span.copy_to(&mut io::sink()).unwrap();
        assert_eq!(copied, span.len());
    }
}

fn assert_batch_contract_mdd(dictionary: &MddFile, ordinals: &[KeyOrdinal]) {
    let mut requests = ordinals.iter().rev().copied().collect::<Vec<_>>();
    requests.push(ordinals[0]);
    requests.push(KeyOrdinal::new(dictionary.len()));
    let actual = dictionary.keys_at(&requests).unwrap();
    let expected = requests
        .iter()
        .map(|ordinal| dictionary.key_at(*ordinal).unwrap())
        .collect::<Vec<Option<KeyEntry>>>();
    assert_eq!(actual, expected);
}
