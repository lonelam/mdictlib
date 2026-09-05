//! Safety properties of the version 1 grammar under hostile input.
//!
//! These tests assert the properties that make an untrusted-input parser
//! usable: no panic, no allocation past the configured ceilings, no way for a
//! declared size to bypass a limit, and no amplification when a deterministic
//! failure is retried.

mod support;

use std::panic;

use mdictlib::{ChecksumPolicy, Error, KeyOrdinal, Limits, MdxFile, OpenOptions};

use support::FixtureKind;
use support::v1::{V1BlockCoding, V1Fixture, V1FixtureBuilder};

fn sample() -> V1Fixture {
    V1FixtureBuilder::mdx([
        ("alpha", "first record"),
        ("beta", "second record"),
        ("gamma", "third record"),
        ("delta", "fourth record"),
    ])
    .coding(V1BlockCoding::None)
    .key_blocks([2, 2])
    .record_blocks([25, 25])
    .build()
}

#[test]
fn single_byte_mutations_anywhere_in_a_v1_file_never_panic() {
    let fixture = sample();
    for index in 0..fixture.bytes.len() {
        let mut mutated = fixture.clone();
        mutated.bytes[index] ^= 0xa5;
        let file = mutated.write("v1-mutation");
        let path = file.path().to_path_buf();
        let result = panic::catch_unwind(|| {
            if let Ok(dictionary) = MdxFile::open(&path) {
                for entry in dictionary.entries() {
                    let _ = entry;
                }
                let _ = dictionary.locate("alpha");
                let _ = dictionary.key_at(KeyOrdinal::new(0));
            }
        });
        assert!(result.is_ok(), "byte {index} caused a panic");
    }
}

#[test]
fn truncating_a_v1_file_at_every_offset_never_panics() {
    let fixture = sample();
    for keep in 0..fixture.bytes.len() {
        let file = fixture.write_truncated("v1-truncation", keep);
        let path = file.path().to_path_buf();
        let result = panic::catch_unwind(|| {
            if let Ok(dictionary) = MdxFile::open(&path) {
                for entry in dictionary.entries() {
                    let _ = entry;
                }
            }
        });
        assert!(result.is_ok(), "truncation at {keep} caused a panic");
    }
}

/// Every configured ceiling must be enforced against a declared version 1
/// size, and must be reported as the limit it is rather than as generic
/// corruption.
#[test]
fn each_v1_limit_reports_limit_exceeded() {
    let fixture = sample();
    let file = fixture.write("v1-limit-kind");

    for limits in [
        Limits::new().with_key_index_bytes(4),
        Limits::new().with_record_index_bytes(4),
        Limits::new().with_compressed_block_bytes(8),
        Limits::new().with_decompressed_block_bytes(8),
        Limits::new().with_block_metadata_bytes(8),
        Limits::new().with_working_memory_bytes(64),
    ] {
        let options = OpenOptions::new().with_limits(limits);
        let outcome = MdxFile::open_with_options(file.path(), &options).and_then(|dictionary| {
            for entry in dictionary.entries() {
                entry?;
            }
            Ok(())
        });
        match outcome {
            Err(Error::LimitExceeded { .. }) => {}
            Err(other) => panic!("expected LimitExceeded, got {other}"),
            Ok(()) => panic!("a tight limit was not enforced"),
        }
    }
}

#[test]
fn a_hostile_v1_header_cannot_force_a_large_allocation() {
    // u32::MAX in every keyword-header size field. With a small working-memory
    // ceiling the reader must refuse before reserving anything large.
    let mut fixture = sample();
    fixture.set_keyword_u32(2, u32::MAX);
    fixture.set_keyword_u32(3, u32::MAX);
    let file = fixture.write("v1-hostile-sizes");

    let options =
        OpenOptions::new().with_limits(Limits::new().with_working_memory_bytes(64 * 1024));
    let error = MdxFile::open_with_options(file.path(), &options).unwrap_err();
    assert!(
        matches!(
            error,
            Error::LimitExceeded { .. } | Error::Truncated { .. } | Error::InvalidData(_)
        ),
        "unexpected error: {error}"
    );
}

#[test]
fn repeated_deterministic_v1_failures_do_not_amplify_memory() {
    // A cached failure must be replayed, not recomputed into a growing charge.
    let mut fixture = sample();
    let range = fixture.layout.key_blocks[1].clone();
    fixture.corrupt_block_checksum(&range);
    let file = fixture.write("v1-failure-amplification");
    let options = OpenOptions::new().with_checksum_policy(ChecksumPolicy::Verify);
    let dictionary = MdxFile::open_with_options(file.path(), &options).unwrap();

    let ordinal = KeyOrdinal::new(2);
    let first = dictionary.key_at(ordinal).unwrap_err().to_string();
    let baseline = dictionary.memory_usage().unwrap();

    for _ in 0..64 {
        let repeated = dictionary.key_at(ordinal).unwrap_err().to_string();
        assert_eq!(repeated, first, "failures must replay identically");
    }

    let after = dictionary.memory_usage().unwrap();
    assert_eq!(
        after.current_bytes(),
        baseline.current_bytes(),
        "retrying a deterministic failure must not accumulate charges"
    );
}

#[test]
fn v1_iterators_stay_exhausted_after_an_error() {
    let mut fixture = sample();
    let range = fixture.layout.record_blocks[1].clone();
    fixture.corrupt_block_checksum(&range);
    let file = fixture.write("v1-iterator-fusion");
    let options = OpenOptions::new().with_checksum_policy(ChecksumPolicy::Verify);
    let dictionary = MdxFile::open_with_options(file.path(), &options).unwrap();

    let mut entries = dictionary.entries();
    let mut errors = 0;
    for result in entries.by_ref() {
        if result.is_err() {
            errors += 1;
        }
    }
    assert_eq!(errors, 1);
    for _ in 0..8 {
        assert!(entries.next().is_none(), "a fused iterator must stay empty");
    }
}

#[test]
fn v1_open_does_not_decode_blocks() {
    // Every key and record block is corrupt, yet open must succeed: block
    // payloads are only touched lazily.
    let mut fixture = sample();
    for range in fixture.layout.key_blocks.clone() {
        fixture.corrupt_block_checksum(&range);
    }
    for range in fixture.layout.record_blocks.clone() {
        fixture.corrupt_block_checksum(&range);
    }
    let file = fixture.write("v1-lazy-open");
    let options = OpenOptions::new().with_checksum_policy(ChecksumPolicy::Verify);
    let dictionary =
        MdxFile::open_with_options(file.path(), &options).expect("open must not decode blocks");
    assert_eq!(dictionary.len(), 4);
    assert!(dictionary.key_at(KeyOrdinal::new(0)).is_err());
}

#[test]
fn default_checksum_policy_skips_block_mismatch() {
    let mut fixture = sample();
    let range = fixture.layout.key_blocks[0].clone();
    fixture.corrupt_block_checksum(&range);
    let file = fixture.write("v1-skip-checksum");
    let dictionary = MdxFile::open(file.path()).expect("checksum-only corruption is skipped");

    assert_eq!(
        dictionary
            .key_at(KeyOrdinal::new(0))
            .unwrap()
            .unwrap()
            .key(),
        "alpha"
    );
}

#[test]
fn v1_mdd_fixtures_survive_the_same_mutation_sweep() {
    let fixture = V1FixtureBuilder::mdd([
        ("\\one.bin".to_owned(), vec![1u8, 2, 3]),
        ("\\two.bin".to_owned(), vec![4u8, 5, 6, 7]),
    ])
    .coding(V1BlockCoding::None)
    .build();
    assert_eq!(fixture.kind, FixtureKind::Mdd);

    for index in 0..fixture.bytes.len() {
        let mut mutated = fixture.clone();
        mutated.bytes[index] ^= 0x5a;
        let file = mutated.write("v1-mdd-mutation");
        let path = file.path().to_path_buf();
        let result = panic::catch_unwind(|| {
            if let Ok(dictionary) = mdictlib::MddFile::open(&path) {
                for resource in dictionary.resources() {
                    let _ = resource;
                }
                if let Ok(Some(span)) = dictionary.span_at(KeyOrdinal::new(0)) {
                    let _ = span.copy_to(&mut std::io::sink());
                }
            }
        });
        assert!(result.is_ok(), "byte {index} caused a panic");
    }
}
