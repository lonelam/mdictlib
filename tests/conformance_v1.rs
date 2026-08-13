//! Whole-file conformance for the MDict version 1 wire grammar.
//!
//! Every fixture here is produced by the independent version 1 encoder in
//! `tests/support/v1.rs`, which never calls parser code. The behavioral
//! assertions come from `tests/support/behavior.rs` and are the same ones the
//! version 2 suite runs, so passing them is evidence that adding a wire
//! version did not fork the shared core.

mod support;

use mdictlib::{Error, KeyOrdinal, MddFile, MdxFile};

use support::behavior::{ExpectedEntries, assert_mdd_behavior, assert_mdx_behavior};
use support::v1::{V1BlockCoding, V1FixtureBuilder, repeating_payload};
use support::{FixtureEncoding, FixtureKind};

/// Codings usable with arbitrary payloads. `LzoBackReference` needs a payload
/// shaped for lookbehind matches, so it has dedicated tests instead.
const CODINGS: [V1BlockCoding; 2] = [V1BlockCoding::None, V1BlockCoding::Lzo];

fn lzo_available() -> bool {
    cfg!(feature = "lzo")
}

/// Skips LZO fixtures when the optional feature is off, after first proving
/// the reader reports a precise unsupported-compression error.
fn skip_when_lzo_is_unavailable(coding: V1BlockCoding) -> bool {
    !lzo_available() && matches!(coding, V1BlockCoding::Lzo | V1BlockCoding::LzoBackReference)
}

#[test]
fn v1_mdx_round_trips_every_block_coding() {
    let entries = [
        ("alpha", "first record"),
        ("beta", "second record"),
        ("gamma", "third record"),
        ("delta", "fourth record"),
    ];
    let expected = ExpectedEntries::text(&entries);

    for coding in CODINGS {
        if skip_when_lzo_is_unavailable(coding) {
            continue;
        }
        let fixture = V1FixtureBuilder::mdx(entries)
            .coding(coding)
            .key_blocks([2, 2])
            .record_blocks([12, 13, 12, 13])
            .build();
        let file = fixture.write("v1-mdx-coding");
        assert_mdx_behavior(file.path(), &expected);
    }
}

#[test]
fn v1_mdd_round_trips_every_block_coding_and_streaming_route() {
    let entries = [
        ("\\one.bin".to_owned(), vec![0u8, 1, 2, 3, 4]),
        ("\\two.bin".to_owned(), vec![9u8; 40]),
        ("\\three.bin".to_owned(), Vec::new()),
        ("\\four.bin".to_owned(), (0u8..=255).collect()),
    ];
    let borrowed = entries
        .iter()
        .map(|(key, bytes)| (key.as_str(), bytes.clone()))
        .collect::<Vec<_>>();
    let expected = ExpectedEntries::binary(&borrowed);

    for coding in CODINGS {
        if skip_when_lzo_is_unavailable(coding) {
            continue;
        }
        let fixture = V1FixtureBuilder::mdd(entries.clone())
            .coding(coding)
            .key_blocks([2, 2])
            .build();
        let file = fixture.write("v1-mdd-coding");
        assert_mdd_behavior(file.path(), &expected);
    }
}

#[test]
fn v1_mdx_supports_every_supported_encoding() {
    for (encoding, entries) in [
        (
            FixtureEncoding::Utf8,
            [("ascii", "plain"), ("caffè", "accented")],
        ),
        (
            FixtureEncoding::Utf16Le,
            [("wide", "record"), ("宽字", "记录")],
        ),
        (FixtureEncoding::Gbk, [("汉字", "记录"), ("测试", "文本")]),
        (FixtureEncoding::Big5, [("漢字", "記錄"), ("測試", "文字")]),
        (
            FixtureEncoding::Gb18030,
            [("四字节", "😀"), ("测试", "文本")],
        ),
    ] {
        let expected = ExpectedEntries::text(&entries);
        let fixture = V1FixtureBuilder::mdx(entries)
            .encoding(encoding)
            .coding(V1BlockCoding::None)
            .build();
        let file = fixture.write("v1-mdx-encoding");
        assert_mdx_behavior(file.path(), &expected);
    }
}

#[test]
fn v1_summary_lengths_count_units_not_bytes_for_utf16() {
    // A UTF-16 fixture whose summaries are multi-byte proves the one-byte
    // length field counts encoding units: reading it as a byte count would
    // desynchronize the raw metadata immediately.
    let entries = [("宽宽宽宽", "wide"), ("窄", "narrow")];
    let expected = ExpectedEntries::text(&entries);
    let fixture = V1FixtureBuilder::mdx(entries)
        .encoding(FixtureEncoding::Utf16Le)
        .coding(V1BlockCoding::None)
        .build();
    let file = fixture.write("v1-utf16-summary-units");
    assert_mdx_behavior(file.path(), &expected);
}

#[test]
fn v1_handles_duplicates_equal_offsets_and_empty_records() {
    let entries = [
        ("dup", "first"),
        ("other", ""),
        ("dup", "second"),
        ("dup", ""),
    ];
    let expected = ExpectedEntries::text(&entries);
    let fixture = V1FixtureBuilder::mdx(entries)
        .coding(V1BlockCoding::None)
        .build();
    let file = fixture.write("v1-duplicates");
    assert_mdx_behavior(file.path(), &expected);

    let dictionary = MdxFile::open(file.path()).unwrap();
    let matches = dictionary.locate("dup").unwrap().unwrap();
    assert_eq!(matches.len(), 3);
    assert_eq!(
        (0..matches.len())
            .map(|index| matches.get(index).unwrap().get())
            .collect::<Vec<_>>(),
        vec![0, 2, 3]
    );
}

#[test]
fn v1_records_may_cross_decoded_record_block_boundaries() {
    let entries = [("first", "aaaaaaaaaa"), ("second", "bbbbbbbbbb")];
    let expected = ExpectedEntries::text(&entries);
    // Split the twenty record bytes so the first record straddles the seam.
    let fixture = V1FixtureBuilder::mdx(entries)
        .coding(V1BlockCoding::None)
        .record_blocks([6, 7, 7])
        .build();
    let file = fixture.write("v1-cross-block-records");
    assert_mdx_behavior(file.path(), &expected);
}

#[test]
fn v1_mdd_resources_may_cross_record_blocks_when_streamed() {
    let payload = (0u8..=200).collect::<Vec<_>>();
    let entries = [("\\big.bin".to_owned(), payload.clone())];
    let borrowed = [("\\big.bin", payload.clone())];
    let expected = ExpectedEntries::binary(&borrowed);
    let fixture = V1FixtureBuilder::mdd(entries)
        .coding(V1BlockCoding::None)
        .record_blocks([50, 50, 50, 51])
        .build();
    let file = fixture.write("v1-mdd-cross-block");
    assert_mdd_behavior(file.path(), &expected);

    let dictionary = MddFile::open(file.path()).unwrap();
    let span = dictionary.span_at(KeyOrdinal::new(0)).unwrap().unwrap();
    let mut streamed = Vec::new();
    assert_eq!(span.copy_to(&mut streamed).unwrap(), 201);
    assert_eq!(streamed, payload);
}

#[test]
fn v1_multiblock_files_expose_every_ordinal() {
    let entries = (0..24)
        .map(|index| (format!("key{index:03}"), format!("record {index}")))
        .collect::<Vec<_>>();
    let borrowed = entries
        .iter()
        .map(|(key, text)| (key.as_str(), text.as_str()))
        .collect::<Vec<_>>();
    let expected = ExpectedEntries::text(&borrowed);
    // Split the concatenated record bytes into four blocks whose seams fall
    // inside records, so multiblock traversal is genuinely exercised.
    let total: usize = borrowed.iter().map(|(_, text)| text.len()).sum();
    let chunk = total / 4;
    let record_blocks = vec![chunk, chunk, chunk, total - 3 * chunk];
    let fixture = V1FixtureBuilder::mdx(entries)
        .coding(V1BlockCoding::None)
        .key_blocks([5, 5, 5, 9])
        .record_blocks(record_blocks)
        .build();
    let file = fixture.write("v1-multiblock");
    assert_mdx_behavior(file.path(), &expected);
}

#[test]
fn v1_empty_dictionaries_open_without_synthetic_blocks() {
    let mdx = V1FixtureBuilder::mdx(std::iter::empty::<(&str, &str)>()).build();
    let mdx_file = mdx.write("v1-empty-mdx");
    let dictionary = MdxFile::open(mdx_file.path()).unwrap();
    assert!(dictionary.is_empty());
    assert_eq!(dictionary.len(), 0);
    assert!(dictionary.keys().next().is_none());
    assert!(dictionary.entries().next().is_none());
    assert!(dictionary.key_at(KeyOrdinal::new(0)).unwrap().is_none());
    assert!(dictionary.locate("anything").unwrap().is_none());

    let mdd = V1FixtureBuilder::mdd(std::iter::empty::<(&str, Vec<u8>)>()).build();
    let mdd_file = mdd.write("v1-empty-mdd");
    let resources = MddFile::open(mdd_file.path()).unwrap();
    assert!(resources.is_empty());
    assert!(resources.resources().next().is_none());
    assert!(resources.span_at(KeyOrdinal::new(0)).unwrap().is_none());
}

#[test]
fn v1_refuses_iso8859_1_precisely() {
    // Eleven authorized real v1.2 artifacts declare this label. Its byte
    // semantics are unresolved, so the reader must refuse by name.
    let fixture = V1FixtureBuilder::mdx([("latin", "record")])
        .encoding_label("ISO8859-1")
        .build();
    let file = fixture.write("v1-iso8859-1");
    assert!(matches!(
        MdxFile::open(file.path()),
        Err(Error::Unsupported(
            "ISO8859-1 text encoding (MDict byte semantics unresolved)"
        ))
    ));
}

#[test]
fn v1_refuses_declared_encryption_instead_of_guessing_a_framing() {
    for bits in [1u8, 2, 3] {
        let fixture = V1FixtureBuilder::mdx([("key", "record")])
            .declare_encryption(bits)
            .build();
        let file = fixture.write("v1-declared-encryption");
        assert!(
            matches!(
                MdxFile::open(file.path()),
                Err(Error::Unsupported(
                    "encrypted MDict version 1 keyword sections"
                ))
            ),
            "encryption bits {bits} must be refused"
        );
    }
}

#[cfg(not(feature = "lzo"))]
#[test]
fn v1_lzo_blocks_report_a_precise_unsupported_error_without_the_feature() {
    let fixture = V1FixtureBuilder::mdx([("key", "record")])
        .coding(V1BlockCoding::Lzo)
        .build();
    let file = fixture.write("v1-lzo-disabled");
    let dictionary = MdxFile::open(file.path()).expect("open only parses metadata");
    let error = dictionary.keys().next().unwrap().unwrap_err();
    assert!(matches!(
        error,
        Error::Unsupported("LZO compressed blocks (enable the `lzo` feature)")
    ));
}

// ---------------------------------------------------------------------------
// Version fallthrough
// ---------------------------------------------------------------------------

#[test]
fn a_v1_header_with_a_v2_body_fails_without_retrying_the_other_grammar() {
    // The version 2 builder writes a version 2 body; declaring version 1 in
    // the header must produce a version 1 parse failure, never a silent
    // fallback that would let malformed files masquerade as valid.
    let fixture = support::FixtureBuilder::mdx([("alpha", "record"), ("beta", "record")])
        .engine_versions("1.2", "1.2")
        .build();
    let file = fixture.write("v2-body-declared-v1");
    let error = MdxFile::open(file.path()).unwrap_err();
    assert!(
        !matches!(error, Error::Unsupported(_)),
        "expected a structural refusal, got {error}"
    );
}

#[test]
fn a_v2_header_with_a_v1_body_fails_without_retrying_the_other_grammar() {
    let fixture = V1FixtureBuilder::mdx([("alpha", "record"), ("beta", "record")])
        .engine_versions("2.0", "2.0")
        .coding(V1BlockCoding::None)
        .build();
    let file = fixture.write("v1-body-declared-v2");
    let error = MdxFile::open(file.path()).unwrap_err();
    assert!(
        !matches!(error, Error::Unsupported(_)),
        "expected a structural refusal, got {error}"
    );
}

#[test]
fn a_v1_generated_version_cannot_require_a_v2_reader() {
    let fixture = V1FixtureBuilder::mdx([("alpha", "record")])
        .engine_versions("1.2", "2.0")
        .coding(V1BlockCoding::None)
        .build();
    let file = fixture.write("v1-generated-v2-required");
    assert!(matches!(
        MdxFile::open(file.path()),
        Err(Error::InvalidFormat(
            "GeneratedByEngineVersion 1 conflicts with RequiredEngineVersion"
        ))
    ));
}

// ---------------------------------------------------------------------------
// Malformed version 1 files
// ---------------------------------------------------------------------------

fn sample_mdx() -> support::v1::V1Fixture {
    V1FixtureBuilder::mdx([
        ("alpha", "first"),
        ("beta", "second"),
        ("gamma", "third"),
        ("delta", "fourth"),
    ])
    .coding(V1BlockCoding::None)
    .key_blocks([2, 2])
    .record_blocks([11, 11])
    .build()
}

/// Opens a fixture and drains every lazy route, so failures that only surface
/// during lazy decoding are caught too.
fn open_and_drain(fixture: &support::v1::V1Fixture, name: &str) -> Result<(), Error> {
    let file = fixture.write(name);
    match fixture.kind {
        FixtureKind::Mdx => {
            let dictionary = MdxFile::open(file.path())?;
            for entry in dictionary.entries() {
                entry?;
            }
            dictionary.locate("alpha")?;
        }
        FixtureKind::Mdd => {
            let dictionary = MddFile::open(file.path())?;
            for resource in dictionary.resources() {
                resource?;
            }
        }
    }
    Ok(())
}

#[test]
fn v1_keyword_header_field_corruption_fails_closed() {
    // Each of the four u32 keyword-header fields, perturbed independently.
    for (field, value) in [
        (0u32, 99u32), // block count exceeding reality
        (1, 99),       // entry count disagreeing with the metadata
        (2, 4),        // key-info length too small for the rows
        (3, 7),        // key-block length disagreeing with the sum
    ] {
        let mut fixture = sample_mdx();
        fixture.set_keyword_u32(usize::try_from(field).unwrap(), value);
        let error = open_and_drain(&fixture, "v1-keyword-field")
            .expect_err(&format!("keyword field {field}={value} must be refused"));
        assert!(
            !matches!(error, Error::Unsupported(_)),
            "field {field}: expected a structural refusal, got {error}"
        );
    }
}

#[test]
fn v1_record_header_field_corruption_fails_closed() {
    for (field, value) in [
        (0u32, 99u32), // block count disagreeing with the index length
        (1, 99),       // entry count disagreeing with the key index
        (2, 9),        // index length that is not block_count * 8
        (3, 5),        // blocks length disagreeing with the summed sizes
    ] {
        let mut fixture = sample_mdx();
        fixture.set_record_u32(usize::try_from(field).unwrap(), value);
        let error = open_and_drain(&fixture, "v1-record-field")
            .expect_err(&format!("record field {field}={value} must be refused"));
        assert!(
            !matches!(error, Error::Unsupported(_)),
            "field {field}: expected a structural refusal, got {error}"
        );
    }
}

#[test]
fn v1_rejects_a_v2_style_sixteen_byte_record_index() {
    // A version 2 index row is sixteen bytes. Accepting one here would mean the
    // record grammar was not actually version-specific.
    let mut fixture = sample_mdx();
    let blocks = fixture.layout.record_blocks.len();
    fixture.set_record_u32(2, u32::try_from(blocks * 16).unwrap());
    let error = open_and_drain(&fixture, "v1-v2-index-row").unwrap_err();
    assert!(matches!(error, Error::InvalidData(_)));
}

#[test]
fn v1_rejects_trailing_keyword_metadata_bytes() {
    // One authorized real artifact fails exactly this way. It stays refused;
    // no fallback may be invented to accept it.
    let fixture = V1FixtureBuilder::mdx([("alpha", "first"), ("beta", "second")])
        .coding(V1BlockCoding::None)
        .key_info_trailing_bytes([0u8; 262])
        .build();
    let error = open_and_drain(&fixture, "v1-trailing-key-info").unwrap_err();
    match error {
        Error::InvalidData(message) => {
            assert!(
                message.contains("trailing"),
                "unexpected message: {message}"
            );
        }
        other => panic!("expected InvalidData, got {other}"),
    }
}

#[test]
fn v1_rejects_trailing_record_index_bytes() {
    let fixture = V1FixtureBuilder::mdx([("alpha", "first"), ("beta", "second")])
        .coding(V1BlockCoding::None)
        .record_index_trailing_bytes([0u8; 8])
        .build();
    let error = open_and_drain(&fixture, "v1-trailing-record-index").unwrap_err();
    assert!(matches!(error, Error::InvalidData(_)));
}

#[test]
fn v1_rejects_malformed_summary_lengths() {
    for lengths in [(200u8, 5u8), (0, 200), (200, 200)] {
        let fixture = V1FixtureBuilder::mdx([("alpha", "first"), ("beta", "second")])
            .coding(V1BlockCoding::None)
            .summary_length_overrides([lengths])
            .build();
        let error = open_and_drain(&fixture, "v1-summary-length")
            .expect_err(&format!("summary lengths {lengths:?} must be refused"));
        assert!(!matches!(error, Error::Unsupported(_)));
    }
}

#[test]
fn v1_rejects_a_v2_style_terminated_summary() {
    // Version 1 summaries have no terminator. Declaring one more unit than the
    // summary occupies desynchronizes the raw metadata, which must be caught.
    let fixture = V1FixtureBuilder::mdx([("alpha", "first"), ("beta", "second")])
        .coding(V1BlockCoding::None)
        .summary_length_overrides([(6u8, 4u8)])
        .build();
    let error = open_and_drain(&fixture, "v1-terminated-summary").unwrap_err();
    assert!(!matches!(error, Error::Unsupported(_)));
}

#[test]
fn v1_rejects_wrong_block_sizes() {
    // Compressed and decompressed sizes live at the end of each metadata row:
    // 4 (entries) + 1 + 5 (first "alpha") + 1 + 4 ("beta") = 15 bytes in.
    let mut fixture = V1FixtureBuilder::mdx([("alpha", "first"), ("beta", "second")])
        .coding(V1BlockCoding::None)
        .build();
    let mut wrong_compressed = fixture.clone();
    wrong_compressed.set_key_info_u32(15, 9_999);
    let error = open_and_drain(&wrong_compressed, "v1-wrong-comp-size").unwrap_err();
    assert!(!matches!(error, Error::Unsupported(_)));

    fixture.set_key_info_u32(19, 9_999);
    let error = open_and_drain(&fixture, "v1-wrong-decomp-size").unwrap_err();
    assert!(!matches!(error, Error::Unsupported(_)));
}

#[test]
fn v1_rejects_checksum_mismatches_in_key_and_record_blocks() {
    let mut key_corrupt = sample_mdx();
    let range = key_corrupt.layout.key_blocks[0].clone();
    key_corrupt.corrupt_block_checksum(&range);
    let error = open_and_drain(&key_corrupt, "v1-key-checksum").unwrap_err();
    assert!(matches!(error, Error::ChecksumMismatch { .. }));

    let mut record_corrupt = sample_mdx();
    let range = record_corrupt.layout.record_blocks[0].clone();
    record_corrupt.corrupt_block_checksum(&range);
    let error = open_and_drain(&record_corrupt, "v1-record-checksum").unwrap_err();
    assert!(matches!(error, Error::ChecksumMismatch { .. }));
}

#[test]
fn v1_rejects_unknown_and_malformed_block_codings() {
    let mut unknown_tag = sample_mdx();
    let range = unknown_tag.layout.key_blocks[0].clone();
    unknown_tag.corrupt_block_tag(&range, [9, 0, 0, 0]);
    let error = open_and_drain(&unknown_tag, "v1-unknown-tag").unwrap_err();
    assert!(matches!(error, Error::InvalidData(_)));

    // A block that claims LZO but holds uncompressed bytes must not decode.
    let mut wrong_tag = sample_mdx();
    let range = wrong_tag.layout.key_blocks[0].clone();
    wrong_tag.corrupt_block_tag(&range, [1, 0, 0, 0]);
    let error = open_and_drain(&wrong_tag, "v1-malformed-lzo").unwrap_err();
    assert!(!matches!(error, Error::Unsupported(_)) || !lzo_available());
}

#[test]
fn v1_rejects_invalid_record_offsets_in_key_rows() {
    // Rewrite the first row's u32 record offset to a value past the record
    // stream. Payload bytes 0..4 of the first uncompressed key block.
    let mut fixture = sample_mdx();
    let range = fixture.layout.key_blocks[0].clone();
    fixture.set_uncompressed_payload_bytes(&range, 0, &u32::MAX.to_be_bytes());
    let error = open_and_drain(&fixture, "v1-bad-record-offset").unwrap_err();
    assert!(matches!(error, Error::InvalidData(_)));
}

#[test]
fn v1_rejects_decreasing_record_offsets_inside_a_key_block() {
    let mut fixture = sample_mdx();
    let range = fixture.layout.key_blocks[0].clone();
    // First row's offset is 0; set it above the second row's offset.
    fixture.set_uncompressed_payload_bytes(&range, 0, &9u32.to_be_bytes());
    let error = open_and_drain(&fixture, "v1-decreasing-offsets").unwrap_err();
    assert!(matches!(error, Error::InvalidData(_)));
}

#[test]
fn v1_rejects_truncation_at_every_section_boundary() {
    let fixture = sample_mdx();
    let boundaries = [
        fixture.layout.keyword_header_offset + 8,
        fixture.layout.key_info.start + 4,
        fixture.layout.key_blocks[0].start + 4,
        fixture.layout.record_header_offset + 8,
        fixture.layout.record_index.start + 4,
        fixture.layout.record_blocks[0].start + 4,
        fixture.bytes.len() - 1,
    ];
    for keep in boundaries {
        let file = fixture.write_truncated("v1-truncated", keep);
        let outcome = MdxFile::open(file.path()).and_then(|dictionary| {
            for entry in dictionary.entries() {
                entry?;
            }
            Ok(())
        });
        let error = outcome.unwrap_err_or_else_panic(keep);
        assert!(
            !matches!(error, Error::Unsupported(_)),
            "truncation at {keep}: expected a structural refusal, got {error}"
        );
    }
}

/// Small helper so truncation failures name the offset that survived.
trait UnwrapErrOrPanic {
    fn unwrap_err_or_else_panic(self, keep: usize) -> Error;
}

impl UnwrapErrOrPanic for Result<(), Error> {
    fn unwrap_err_or_else_panic(self, keep: usize) -> Error {
        match self {
            Ok(()) => panic!("truncation at {keep} was accepted"),
            Err(error) => error,
        }
    }
}

#[test]
fn v1_hostile_u32_declarations_are_refused_without_allocating() {
    // Every version 1 field is a u32, so u32::MAX is the largest value a file
    // can declare. Each must be refused by a limit or a geometry check rather
    // than reaching an allocation.
    for field in 0..4usize {
        let mut fixture = sample_mdx();
        fixture.set_keyword_u32(field, u32::MAX);
        let error = open_and_drain(&fixture, "v1-hostile-keyword").unwrap_err();
        assert!(
            matches!(
                error,
                Error::InvalidData(_) | Error::LimitExceeded { .. } | Error::Truncated { .. }
            ),
            "keyword field {field}: unexpected {error}"
        );

        let mut fixture = sample_mdx();
        fixture.set_record_u32(field, u32::MAX);
        let error = open_and_drain(&fixture, "v1-hostile-record").unwrap_err();
        assert!(
            matches!(
                error,
                Error::InvalidData(_) | Error::LimitExceeded { .. } | Error::Truncated { .. }
            ),
            "record field {field}: unexpected {error}"
        );
    }
}

#[test]
fn v1_iteration_fuses_after_the_first_lazy_error() {
    let mut fixture = sample_mdx();
    let range = fixture.layout.record_blocks[1].clone();
    fixture.corrupt_block_checksum(&range);
    let file = fixture.write("v1-fused-iteration");
    let dictionary = MdxFile::open(file.path()).unwrap();

    let mut entries = dictionary.entries();
    let mut errors = 0;
    let mut seen = 0;
    for result in entries.by_ref() {
        seen += 1;
        if result.is_err() {
            errors += 1;
        }
    }
    assert_eq!(errors, 1, "an iterator must yield at most one error");
    assert!(seen > 1, "records before the corrupt block still decode");
    assert!(entries.next().is_none(), "iterator stays exhausted");
}

#[test]
fn v1_deterministic_failures_replay_identically() {
    let mut fixture = sample_mdx();
    let range = fixture.layout.key_blocks[1].clone();
    fixture.corrupt_block_checksum(&range);
    let file = fixture.write("v1-cached-failure");
    let dictionary = MdxFile::open(file.path()).unwrap();

    let ordinal = KeyOrdinal::new(2);
    let first = dictionary.key_at(ordinal).unwrap_err().to_string();
    let second = dictionary.key_at(ordinal).unwrap_err().to_string();
    assert_eq!(first, second, "cached failures must replay identically");
}

#[test]
fn v1_back_reference_lzo_actually_exercises_the_copy_path() {
    if !lzo_available() {
        return;
    }
    // The record payload is shaped so its LZO stream is mostly lookbehind
    // copies; key blocks keep a literal-only stream because key rows carry
    // offsets that do not repeat.
    let payload = String::from_utf8(repeating_payload(b"abcd", 40)).unwrap();
    let entries = [("repeated", payload.as_str())];
    let expected = ExpectedEntries::text(&entries);
    let fixture = V1FixtureBuilder::mdx(entries)
        .mixed_coding(V1BlockCoding::Lzo, V1BlockCoding::LzoBackReference)
        .build();
    let file = fixture.write("v1-lzo-back-reference");
    assert_mdx_behavior(file.path(), &expected);

    // The same shape must also stream correctly through an MDD span.
    let bytes = repeating_payload(b"\x01\x02\x03\x04", 60);
    let mdd_entries = [("\\repeated.bin".to_owned(), bytes.clone())];
    let borrowed = [("\\repeated.bin", bytes)];
    let mdd = V1FixtureBuilder::mdd(mdd_entries)
        .mixed_coding(V1BlockCoding::Lzo, V1BlockCoding::LzoBackReference)
        .build();
    let mdd_file = mdd.write("v1-mdd-lzo-back-reference");
    assert_mdd_behavior(mdd_file.path(), &ExpectedEntries::binary(&borrowed));
}
