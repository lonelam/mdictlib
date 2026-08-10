//! Version-agnostic behavioral assertions for the shared parsing core.
//!
//! Every function here drives only the public API, so the same assertions can
//! run against a synthetic version 1 file and a synthetic version 2 file. That
//! is the executable form of the claim that adding a wire version did not add
//! a version-dependent branch to lookup, iteration, ordinal access, record
//! decoding, or MDD streaming.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;

use mdictlib::{KeyOrdinal, MddFile, MdxFile};

/// What a fixture is expected to contain, independent of how it was encoded.
#[derive(Debug, Clone)]
pub struct ExpectedEntries {
    pub keys: Vec<String>,
    pub records: Vec<Vec<u8>>,
}

impl ExpectedEntries {
    pub fn text(entries: &[(&str, &str)]) -> Self {
        Self {
            keys: entries.iter().map(|(key, _)| (*key).to_owned()).collect(),
            records: entries
                .iter()
                .map(|(_, text)| text.as_bytes().to_vec())
                .collect(),
        }
    }

    pub fn binary(entries: &[(&str, Vec<u8>)]) -> Self {
        Self {
            keys: entries.iter().map(|(key, _)| (*key).to_owned()).collect(),
            records: entries.iter().map(|(_, bytes)| bytes.clone()).collect(),
        }
    }

    fn len(&self) -> u64 {
        u64::try_from(self.keys.len()).unwrap()
    }

    /// Groups ordinals by key, preserving ascending physical order, which is
    /// exactly what duplicate-aware lookup must return.
    fn ordinals_by_key(&self) -> BTreeMap<&str, Vec<u64>> {
        let mut grouped: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
        for (index, key) in self.keys.iter().enumerate() {
            grouped
                .entry(key.as_str())
                .or_default()
                .push(u64::try_from(index).unwrap());
        }
        grouped
    }
}

/// Runs every shared-core route an MDX file exposes.
pub fn assert_mdx_behavior(path: &Path, expected: &ExpectedEntries) {
    let dictionary = MdxFile::open(path).expect("fixture opens");
    assert_eq!(dictionary.len(), expected.len(), "declared entry count");
    assert_eq!(dictionary.is_empty(), expected.keys.is_empty());

    // Physical key iteration, in file order, with no gaps.
    let iterated = dictionary
        .keys()
        .collect::<Result<Vec<_>, _>>()
        .expect("key iteration succeeds");
    assert_eq!(iterated.len(), expected.keys.len(), "iterated key count");
    for (index, entry) in iterated.iter().enumerate() {
        assert_eq!(entry.ordinal().get(), u64::try_from(index).unwrap());
        assert_eq!(entry.key(), expected.keys[index], "key at ordinal {index}");
    }

    assert_ordinal_routes(&expected.keys, |ordinal| {
        dictionary
            .key_at(ordinal)
            .expect("key_at succeeds")
            .map(|entry| entry.key().to_owned())
    });

    // keys_at preserves caller order, repeats, and out-of-range None.
    let probes = ordinal_probe_set(expected.len());
    let batched = dictionary.keys_at(&probes).expect("keys_at succeeds");
    assert_eq!(batched.len(), probes.len());
    for (probe, resolved) in probes.iter().zip(batched.iter()) {
        let index = usize::try_from(probe.get()).unwrap();
        match expected.keys.get(index) {
            Some(key) => assert_eq!(
                resolved.as_ref().map(|entry| entry.key()),
                Some(key.as_str())
            ),
            None => assert!(resolved.is_none(), "out-of-range ordinal must be None"),
        }
    }

    // Raw lookup for every distinct key, with all duplicate ordinals in order.
    for (key, ordinals) in expected.ordinals_by_key() {
        let matches = dictionary
            .locate(key)
            .expect("locate succeeds")
            .unwrap_or_else(|| panic!("locate found no match for {key:?}"));
        let located = (0..matches.len())
            .map(|index| matches.get(index).unwrap().get())
            .collect::<Vec<_>>();
        assert_eq!(located, ordinals, "duplicate ordinals for {key:?}");
        assert_eq!(matches.first().get(), ordinals[0], "lowest ordinal wins");
    }

    // entry_at, entries, and lookup must agree on payloads.
    for (index, record) in expected.records.iter().enumerate() {
        let ordinal = KeyOrdinal::new(u64::try_from(index).unwrap());
        let entry = dictionary
            .entry_at(ordinal)
            .expect("entry_at succeeds")
            .expect("entry exists");
        assert_eq!(entry.ordinal(), ordinal);
        assert_eq!(entry.key(), expected.keys[index]);
        assert_eq!(
            entry.text().as_bytes(),
            record.as_slice(),
            "entry_at payload at ordinal {index}"
        );
    }

    let streamed = dictionary
        .entries()
        .collect::<Result<Vec<_>, _>>()
        .expect("entry iteration succeeds");
    assert_eq!(streamed.len(), expected.records.len());
    for (index, entry) in streamed.iter().enumerate() {
        assert_eq!(entry.ordinal().get(), u64::try_from(index).unwrap());
        assert_eq!(entry.text().as_bytes(), expected.records[index].as_slice());
    }

    for (key, ordinals) in expected.ordinals_by_key() {
        let entry = dictionary
            .lookup(key)
            .expect("lookup succeeds")
            .expect("lookup finds the key");
        assert_eq!(entry.ordinal().get(), ordinals[0]);
        let index = usize::try_from(ordinals[0]).unwrap();
        assert_eq!(entry.text().as_bytes(), expected.records[index].as_slice());
    }

    assert!(
        dictionary
            .locate("\u{1}absent key\u{1}")
            .expect("locate succeeds")
            .is_none(),
        "absent keys must not match"
    );
}

/// Runs every shared-core route an MDD file exposes, including streaming.
pub fn assert_mdd_behavior(path: &Path, expected: &ExpectedEntries) {
    let dictionary = MddFile::open(path).expect("fixture opens");
    assert_eq!(dictionary.len(), expected.len(), "declared entry count");
    assert_eq!(dictionary.is_empty(), expected.keys.is_empty());

    let iterated = dictionary
        .keys()
        .collect::<Result<Vec<_>, _>>()
        .expect("key iteration succeeds");
    assert_eq!(iterated.len(), expected.keys.len());
    for (index, entry) in iterated.iter().enumerate() {
        assert_eq!(entry.ordinal().get(), u64::try_from(index).unwrap());
        assert_eq!(entry.key(), expected.keys[index]);
    }

    assert_ordinal_routes(&expected.keys, |ordinal| {
        dictionary
            .key_at(ordinal)
            .expect("key_at succeeds")
            .map(|entry| entry.key().to_owned())
    });

    let probes = ordinal_probe_set(expected.len());
    let batched = dictionary.keys_at(&probes).expect("keys_at succeeds");
    for (probe, resolved) in probes.iter().zip(batched.iter()) {
        let index = usize::try_from(probe.get()).unwrap();
        match expected.keys.get(index) {
            Some(key) => assert_eq!(
                resolved.as_ref().map(|entry| entry.key()),
                Some(key.as_str())
            ),
            None => assert!(resolved.is_none()),
        }
    }

    for (key, ordinals) in expected.ordinals_by_key() {
        let matches = dictionary
            .locate(key)
            .expect("locate succeeds")
            .unwrap_or_else(|| panic!("locate found no match for {key:?}"));
        let located = (0..matches.len())
            .map(|index| matches.get(index).unwrap().get())
            .collect::<Vec<_>>();
        assert_eq!(located, ordinals, "duplicate ordinals for {key:?}");
    }

    // resource_at, span_at, read, and copy_to must all agree byte for byte.
    for (index, record) in expected.records.iter().enumerate() {
        let ordinal = KeyOrdinal::new(u64::try_from(index).unwrap());
        let resource = dictionary
            .resource_at(ordinal)
            .expect("resource_at succeeds")
            .expect("resource exists");
        assert_eq!(resource.ordinal(), ordinal);
        assert_eq!(resource.key(), expected.keys[index]);
        assert_eq!(resource.bytes(), record.as_slice(), "resource at {index}");

        let span = dictionary
            .span_at(ordinal)
            .expect("span_at succeeds")
            .expect("span exists");
        assert_eq!(span.ordinal(), ordinal);
        assert_eq!(span.len(), u64::try_from(record.len()).unwrap());
        assert_eq!(span.is_empty(), record.is_empty());
        assert_eq!(
            span.read().expect("span read succeeds").bytes(),
            record.as_slice()
        );

        let mut streamed = Vec::new();
        let written = span.copy_to(&mut streamed).expect("copy_to succeeds");
        assert_eq!(written, u64::try_from(record.len()).unwrap());
        assert_eq!(streamed, *record, "streamed bytes at ordinal {index}");
    }

    let iterated_resources = dictionary
        .resources()
        .collect::<Result<Vec<_>, _>>()
        .expect("resource iteration succeeds");
    assert_eq!(iterated_resources.len(), expected.records.len());
    for (index, resource) in iterated_resources.iter().enumerate() {
        assert_eq!(resource.bytes(), expected.records[index].as_slice());
    }

    for (key, ordinals) in expected.ordinals_by_key() {
        let index = usize::try_from(ordinals[0]).unwrap();
        let resource = dictionary
            .lookup(key)
            .expect("lookup succeeds")
            .expect("lookup finds the key");
        assert_eq!(resource.ordinal().get(), ordinals[0]);
        assert_eq!(resource.bytes(), expected.records[index].as_slice());

        let span = dictionary
            .lookup_span(key)
            .expect("lookup_span succeeds")
            .expect("lookup_span finds the key");
        assert_eq!(span.ordinal().get(), ordinals[0]);
        let mut streamed = Vec::new();
        span.copy_to(&mut streamed).expect("copy_to succeeds");
        assert_eq!(streamed, expected.records[index]);
    }
}

/// Exercises direct ordinal access across every entry plus the first
/// out-of-range ordinal.
fn assert_ordinal_routes(keys: &[String], resolve: impl Fn(KeyOrdinal) -> Option<String>) {
    for (index, key) in keys.iter().enumerate() {
        let ordinal = KeyOrdinal::new(u64::try_from(index).unwrap());
        assert_eq!(
            resolve(ordinal).as_deref(),
            Some(key.as_str()),
            "key_at ordinal {index}"
        );
    }
    let past_end = KeyOrdinal::new(u64::try_from(keys.len()).unwrap());
    assert!(resolve(past_end).is_none(), "past-the-end ordinal is None");
}

/// Builds a probe list covering reverse order, repeats, and out-of-range.
fn ordinal_probe_set(len: u64) -> Vec<KeyOrdinal> {
    let mut probes = Vec::new();
    for index in (0..len).rev() {
        probes.push(KeyOrdinal::new(index));
    }
    if len > 0 {
        probes.push(KeyOrdinal::new(0));
        probes.push(KeyOrdinal::new(0));
        probes.push(KeyOrdinal::new(len - 1));
    }
    probes.push(KeyOrdinal::new(len));
    probes.push(KeyOrdinal::new(len + 7));
    probes
}
