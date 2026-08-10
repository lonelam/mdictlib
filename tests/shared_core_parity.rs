//! Proves the shared core behaves identically for both wire versions.
//!
//! Each test builds the *same logical dictionary* twice — once with the
//! independent version 1 encoder, once with the independent version 2 encoder
//! — and runs the *same* assertion function over both. A version-dependent
//! branch anywhere in lookup, iteration, ordinal access, record decoding, or
//! MDD streaming would show up here as a divergence.

mod support;

use mdictlib::{KeyOrdinal, MddFile, MdxFile};

use support::behavior::{ExpectedEntries, assert_mdd_behavior, assert_mdx_behavior};
use support::v1::{V1BlockCoding, V1FixtureBuilder};
use support::{FixtureBuilder, FixtureCompression};

/// The logical content every parity test uses: duplicates, an empty record,
/// non-ASCII text, and enough entries to span several blocks.
fn mdx_entries() -> Vec<(&'static str, &'static str)> {
    vec![
        ("alpha", "first record"),
        ("beta", ""),
        ("alpha", "duplicate record"),
        ("gamma", "third record"),
        ("délta", "accented record"),
        ("alpha", "third duplicate"),
    ]
}

fn mdd_entries() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("\\a.bin", vec![1u8, 2, 3, 4, 5]),
        ("\\b.bin", Vec::new()),
        ("\\a.bin", vec![9u8; 33]),
        ("\\c.bin", (0u8..=64).collect()),
    ]
}

#[test]
fn mdx_shared_core_routes_agree_across_wire_versions() {
    let entries = mdx_entries();
    let expected = ExpectedEntries::text(&entries);

    let v1 = V1FixtureBuilder::mdx(entries.clone())
        .coding(V1BlockCoding::None)
        .key_blocks([2, 2, 2])
        .build();
    let v1_file = v1.write("parity-v1-mdx");
    assert_mdx_behavior(v1_file.path(), &expected);

    let v2 = FixtureBuilder::mdx(entries)
        .compression(FixtureCompression::None)
        .key_blocks([2, 2, 2])
        .build();
    let v2_file = v2.write("parity-v2-mdx");
    assert_mdx_behavior(v2_file.path(), &expected);
}

#[test]
fn mdd_shared_core_routes_agree_across_wire_versions() {
    let entries = mdd_entries();
    let borrowed = entries
        .iter()
        .map(|(key, bytes)| (*key, bytes.clone()))
        .collect::<Vec<_>>();
    let expected = ExpectedEntries::binary(&borrowed);

    let v1 = V1FixtureBuilder::mdd(entries.clone())
        .coding(V1BlockCoding::None)
        .key_blocks([2, 2])
        .build();
    let v1_file = v1.write("parity-v1-mdd");
    assert_mdd_behavior(v1_file.path(), &expected);

    let v2 = FixtureBuilder::mdd(entries)
        .compression(FixtureCompression::None)
        .key_blocks([2, 2])
        .build();
    let v2_file = v2.write("parity-v2-mdd");
    assert_mdd_behavior(v2_file.path(), &expected);
}

#[test]
fn both_versions_report_the_same_public_metadata() {
    let entries = mdx_entries();

    let v1 = V1FixtureBuilder::mdx(entries.clone())
        .coding(V1BlockCoding::None)
        .build();
    let v1_file = v1.write("parity-v1-metadata");
    let v1_dictionary = MdxFile::open(v1_file.path()).unwrap();

    let v2 = FixtureBuilder::mdx(entries)
        .compression(FixtureCompression::None)
        .build();
    let v2_file = v2.write("parity-v2-metadata");
    let v2_dictionary = MdxFile::open(v2_file.path()).unwrap();

    assert_eq!(v1_dictionary.len(), v2_dictionary.len());
    assert_eq!(
        v1_dictionary.header().tag_name(),
        v2_dictionary.header().tag_name()
    );
    assert_eq!(
        v1_dictionary.header().strip_key(),
        v2_dictionary.header().strip_key()
    );
    assert_eq!(
        v1_dictionary.header().key_case_sensitive(),
        v2_dictionary.header().key_case_sensitive()
    );

    // The declared engine version is the one thing that legitimately differs,
    // and it is exposed as raw header text rather than as parser behavior.
    assert_eq!(v1_dictionary.header().generated_by_engine_version(), "1.2");
    assert_eq!(v2_dictionary.header().generated_by_engine_version(), "2.0");
}

#[test]
fn lazy_decoding_is_preserved_for_both_wire_versions() {
    // Opening must not decode key or record blocks. Corrupting a later block
    // and still opening successfully is the observable form of that claim.
    let entries = mdx_entries();

    let mut v1 = V1FixtureBuilder::mdx(entries.clone())
        .coding(V1BlockCoding::None)
        .key_blocks([2, 2, 2])
        .build();
    let range = v1.layout.key_blocks[2].clone();
    v1.corrupt_block_checksum(&range);
    let v1_file = v1.write("parity-v1-lazy");
    let v1_dictionary = MdxFile::open(v1_file.path()).expect("open stays lazy");
    assert!(v1_dictionary.key_at(KeyOrdinal::new(0)).unwrap().is_some());
    assert!(v1_dictionary.key_at(KeyOrdinal::new(5)).is_err());

    let mut v2 = FixtureBuilder::mdx(entries)
        .compression(FixtureCompression::None)
        .key_blocks([2, 2, 2])
        .build();
    let range = v2.layout.key_blocks[2].clone();
    v2.corrupt_block_checksum(&range);
    let v2_file = v2.write("parity-v2-lazy");
    let v2_dictionary = MdxFile::open(v2_file.path()).expect("open stays lazy");
    assert!(v2_dictionary.key_at(KeyOrdinal::new(0)).unwrap().is_some());
    assert!(v2_dictionary.key_at(KeyOrdinal::new(5)).is_err());
}

#[test]
fn memory_accounting_is_reported_for_both_wire_versions() {
    let entries = mdd_entries();

    let v1 = V1FixtureBuilder::mdd(entries.clone())
        .coding(V1BlockCoding::None)
        .build();
    let v1_file = v1.write("parity-v1-memory");
    let v1_usage = MddFile::open(v1_file.path())
        .unwrap()
        .memory_usage()
        .unwrap();

    let v2 = FixtureBuilder::mdd(entries)
        .compression(FixtureCompression::None)
        .build();
    let v2_file = v2.write("parity-v2-memory");
    let v2_usage = MddFile::open(v2_file.path())
        .unwrap()
        .memory_usage()
        .unwrap();

    for usage in [&v1_usage, &v2_usage] {
        assert!(usage.metadata_bytes() > 0, "metadata must be accounted");
        assert!(usage.peak_bytes() >= usage.current_bytes());
    }
}
