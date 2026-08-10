mod support;

use std::io::Write;

use mdictlib::{Error, KeyOrdinal, MddFile, MdxFile};

use support::FixtureBuilder;

#[test]
fn unsupported_major_version_wins_over_its_legacy_encoding_label() {
    let fixture = FixtureBuilder::mdx([("legacy", "record")])
        .engine_versions("1.2", "1.2")
        .encoding_label("ISO8859-1")
        .build();
    let dictionary_file = fixture.write("legacy-version-before-encoding");
    assert!(matches!(
        MdxFile::open(dictionary_file.path()),
        Err(Error::Unsupported(
            "MDict format major version other than 2"
        ))
    ));
}

#[test]
fn valid_empty_mdx_and_mdd_open_without_synthetic_blocks() {
    let mdx_fixture = FixtureBuilder::mdx(std::iter::empty::<(&str, &str)>()).build();
    let mdx_file = mdx_fixture.write("empty-mdx");
    let mdx = MdxFile::open(mdx_file.path()).unwrap();
    assert!(mdx.is_empty());
    assert!(mdx.keys().next().is_none());
    assert!(mdx.entries().next().is_none());
    assert!(mdx.locate("anything").unwrap().is_none());

    let mdd_fixture = FixtureBuilder::mdd(std::iter::empty::<(&str, Vec<u8>)>()).build();
    let mdd_file = mdd_fixture.write("empty-mdd");
    let mdd = MddFile::open(mdd_file.path()).unwrap();
    assert!(mdd.is_empty());
    assert!(mdd.keys().next().is_none());
    assert!(mdd.resources().next().is_none());
    assert!(mdd.locate("anything").unwrap().is_none());
}

#[test]
fn valid_multiblock_mdx_round_trips_ordinals_duplicates_and_record_boundaries() {
    let fixture = FixtureBuilder::mdx([
        ("alpha", "AA"),
        ("duplicate", "111"),
        ("duplicate", "2222"),
        ("empty", ""),
        ("following", "F"),
        ("omega", "OO"),
    ])
    .key_blocks(vec![2, 2, 2])
    // Concatenated records are 12 bytes. These boundaries split records 0
    // and 2, while entries 3 and 4 deliberately share offset 9.
    .record_blocks(vec![1, 5, 2, 4])
    .build();
    let dictionary_file = fixture.write("valid-multiblock-mdx");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    assert_eq!(dictionary.len(), 6);
    assert_eq!(dictionary.header().encoding_label(), Some("UTF-8"));

    let keys = dictionary
        .keys()
        .map(|result| {
            let key = result.unwrap();
            (key.ordinal().get(), key.into_key())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            (0, "alpha".to_owned()),
            (1, "duplicate".to_owned()),
            (2, "duplicate".to_owned()),
            (3, "empty".to_owned()),
            (4, "following".to_owned()),
            (5, "omega".to_owned()),
        ]
    );

    let batched = dictionary
        .keys_at(&[
            KeyOrdinal::new(5),
            KeyOrdinal::new(1),
            KeyOrdinal::new(1),
            KeyOrdinal::new(6),
        ])
        .unwrap()
        .into_iter()
        .map(|key| key.map(|key| (key.ordinal().get(), key.into_key())))
        .collect::<Vec<_>>();
    assert_eq!(
        batched,
        [
            Some((5, "omega".to_owned())),
            Some((1, "duplicate".to_owned())),
            Some((1, "duplicate".to_owned())),
            None,
        ]
    );

    let entries = dictionary
        .entries()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.ordinal().get(),
                entry.key().to_owned(),
                entry.text().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        entries,
        [
            (0, "alpha".to_owned(), "AA".to_owned()),
            (1, "duplicate".to_owned(), "111".to_owned()),
            (2, "duplicate".to_owned(), "2222".to_owned()),
            (3, "empty".to_owned(), String::new()),
            (4, "following".to_owned(), "F".to_owned()),
            (5, "omega".to_owned(), "OO".to_owned()),
        ]
    );

    // The first duplicate ends in the next key block; the second duplicate
    // crosses two record-block boundaries. Equal starts preserve an empty row.
    assert_eq!(
        dictionary
            .entry_at(KeyOrdinal::new(1))
            .unwrap()
            .unwrap()
            .text(),
        "111"
    );
    assert_eq!(
        dictionary
            .entry_at(KeyOrdinal::new(2))
            .unwrap()
            .unwrap()
            .text(),
        "2222"
    );
    assert_eq!(
        dictionary
            .entry_at(KeyOrdinal::new(3))
            .unwrap()
            .unwrap()
            .text(),
        ""
    );
}

#[test]
fn valid_multiblock_mdd_round_trips_spans_streaming_and_binary_resources() {
    let fixture = FixtureBuilder::mdd([
        ("\\alpha.bin", vec![0x00, 0x01]),
        ("\\duplicate.bin", vec![0x11, 0x12, 0x13]),
        ("\\duplicate.bin", vec![0x21, 0x22, 0x23, 0x24]),
        ("\\empty.bin", Vec::new()),
        ("\\following.bin", vec![0x31]),
        ("\\omega.bin", vec![0x41, 0x42]),
    ])
    .key_blocks(vec![2, 2, 2])
    .record_blocks(vec![1, 5, 2, 4])
    .build();
    let dictionary_file = fixture.write("valid-multiblock-mdd");
    let dictionary = MddFile::open(dictionary_file.path()).unwrap();

    assert_eq!(dictionary.len(), 6);
    assert_eq!(
        dictionary
            .keys()
            .map(|key| key.unwrap().into_key())
            .collect::<Vec<_>>(),
        [
            "\\alpha.bin",
            "\\duplicate.bin",
            "\\duplicate.bin",
            "\\empty.bin",
            "\\following.bin",
            "\\omega.bin",
        ]
    );

    let span = dictionary.span_at(KeyOrdinal::new(2)).unwrap().unwrap();
    assert_eq!(span.ordinal(), KeyOrdinal::new(2));
    assert_eq!(span.key(), "\\duplicate.bin");
    assert_eq!(span.len(), 4);

    let mut streamed = Vec::new();
    assert_eq!(span.copy_to(&mut streamed).unwrap(), 4);
    assert_eq!(streamed, [0x21, 0x22, 0x23, 0x24]);
    assert_eq!(span.read().unwrap().bytes(), streamed);

    let empty = dictionary.span_at(KeyOrdinal::new(3)).unwrap().unwrap();
    assert!(empty.is_empty());
    assert!(empty.read().unwrap().bytes().is_empty());

    let resources = dictionary
        .resources()
        .map(|resource| {
            let resource = resource.unwrap();
            (
                resource.ordinal().get(),
                resource.key().to_owned(),
                resource.bytes().to_vec(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(resources.len(), 6);
    assert_eq!(resources[1].2, [0x11, 0x12, 0x13]);
    assert_eq!(resources[2].2, [0x21, 0x22, 0x23, 0x24]);
    assert!(resources[3].2.is_empty());
}

#[test]
fn key_block_corruption_is_lazy_and_key_and_entry_iterators_terminate_after_error() {
    let mut fixture = FixtureBuilder::mdx([("alpha", "A"), ("beta", "B"), ("omega", "O")])
        .key_blocks(vec![2, 1])
        .build();
    let corrupt_block = fixture.layout.key_blocks[1].clone();
    fixture.corrupt_block_checksum(&corrupt_block);
    let dictionary_file = fixture.write("lazy-corrupt-key-block");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let mut keys = dictionary.keys();
    assert_eq!(keys.next().unwrap().unwrap().key(), "alpha");
    assert_eq!(keys.next().unwrap().unwrap().key(), "beta");
    assert!(keys.next().unwrap().is_err());
    assert!(keys.next().is_none());
    assert!(keys.next().is_none());

    let mut entries = dictionary.entries();
    assert_eq!(entries.next().unwrap().unwrap().text(), "A");
    assert!(entries.next().unwrap().is_err());
    assert!(entries.next().is_none());
    assert!(entries.next().is_none());

    let mut fixture = FixtureBuilder::mdd([
        ("\\alpha.bin", b"A".to_vec()),
        ("\\omega.bin", b"O".to_vec()),
    ])
    .key_blocks(vec![1, 1])
    .record_blocks(vec![1, 1])
    .build();
    let corrupt_block = fixture.layout.record_blocks[1].clone();
    fixture.corrupt_block_checksum(&corrupt_block);
    let resource_file = fixture.write("lazy-corrupt-resource-block");
    let resources = MddFile::open(resource_file.path()).unwrap();
    let mut iter = resources.resources();
    assert_eq!(iter.next().unwrap().unwrap().bytes(), b"A");
    assert!(iter.next().unwrap().is_err());
    assert!(iter.next().is_none());
    assert!(iter.next().is_none());
}

#[test]
fn deterministic_lazy_failures_are_cached_after_the_first_attempt() {
    let mut fixture = FixtureBuilder::mdx([("alpha", "A")]).build();
    let key_block = fixture.layout.key_blocks[0].clone();
    fixture.corrupt_block_checksum(&key_block);
    let dictionary_file = fixture.write("cached-corrupt-key-block");

    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();
    assert!(matches!(
        dictionary.key_at(KeyOrdinal::new(0)).unwrap_err(),
        Error::ChecksumMismatch { .. }
    ));
    assert!(matches!(
        dictionary.key_at(KeyOrdinal::new(0)).unwrap_err(),
        Error::ChecksumMismatch { .. }
    ));

    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();
    assert!(matches!(
        dictionary.locate("alpha").unwrap_err(),
        Error::ChecksumMismatch { .. }
    ));
    assert!(matches!(
        dictionary.locate("alpha").unwrap_err(),
        Error::ChecksumMismatch { .. }
    ));

    let mut fixture = FixtureBuilder::mdx([("alpha", "A")]).build();
    let record_block = fixture.layout.record_blocks[0].clone();
    fixture.corrupt_block_checksum(&record_block);
    let dictionary_file = fixture.write("cached-corrupt-record-block");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();
    assert!(matches!(
        dictionary.entry_at(KeyOrdinal::new(0)).unwrap_err(),
        Error::ChecksumMismatch { .. }
    ));
    assert!(matches!(
        dictionary.entry_at(KeyOrdinal::new(0)).unwrap_err(),
        Error::ChecksumMismatch { .. }
    ));
}

#[test]
fn malformed_key_terminator_is_detected_lazily_and_fuses_iteration() {
    let mut fixture = FixtureBuilder::mdx([("unterminated", "payload")]).build();
    let key_block = fixture.layout.key_blocks[0].clone();
    let payload_len = key_block.end - key_block.start - 8;
    fixture.set_uncompressed_payload_byte(&key_block, payload_len - 1, b'!');
    let dictionary_file = fixture.write("unterminated-key");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let mut keys = dictionary.keys();
    assert!(keys.next().unwrap().is_err());
    assert!(keys.next().is_none());
    assert!(keys.next().is_none());
}

#[test]
fn plausible_but_wrong_decoded_key_count_is_detected_lazily_and_fuses_iteration() {
    let mut fixture = FixtureBuilder::mdx([("sufficiently-long-key", "payload")]).build();
    let key_index = fixture.layout.key_index_block.clone();
    // The first key-index field is the block's declared entry count. Keep all
    // eager section counts mutually consistent at two while leaving one
    // physical key row in the lazy key block.
    fixture.set_uncompressed_payload_byte(&key_index, 7, 2);
    fixture.set_keyword_u64(1, 2);
    fixture.set_record_u64(1, 2);
    let dictionary_file = fixture.write("lazy-key-count-mismatch");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let mut keys = dictionary.keys();
    assert!(keys.next().unwrap().is_err());
    assert!(keys.next().is_none());
    assert!(keys.next().is_none());
}

#[test]
fn extra_decoded_key_rows_are_rejected_before_capacity_can_grow() {
    let mut fixture = FixtureBuilder::mdx([("alpha", "A"), ("omega", "O")]).build();
    let key_index = fixture.layout.key_index_block.clone();
    // Declare one row consistently in the eager headers/index while retaining
    // two checksum-valid physical rows in the lazy key block. The parser must
    // reject the extra row before Vec::push can exceed the declared capacity.
    fixture.set_uncompressed_payload_byte(&key_index, 7, 1);
    fixture.set_keyword_u64(1, 1);
    fixture.set_record_u64(1, 1);
    let dictionary_file = fixture.write("extra-lazy-key-row");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let mut keys = dictionary.keys();
    assert!(keys.next().unwrap().is_err());
    assert!(keys.next().is_none());
}

#[test]
fn record_block_corruption_is_lazy_and_does_not_hide_earlier_records() {
    let mut fixture = FixtureBuilder::mdx([("alpha", "A"), ("omega", "O")])
        .key_blocks(vec![1, 1])
        .record_blocks(vec![1, 1])
        .build();
    let corrupt_block = fixture.layout.record_blocks[1].clone();
    fixture.corrupt_block_checksum(&corrupt_block);
    let dictionary_file = fixture.write("lazy-corrupt-record-block");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    assert_eq!(
        dictionary
            .entry_at(KeyOrdinal::new(0))
            .unwrap()
            .unwrap()
            .text(),
        "A"
    );
    assert!(dictionary.entry_at(KeyOrdinal::new(1)).is_err());

    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();
    let mut entries = dictionary.entries();
    assert_eq!(entries.next().unwrap().unwrap().text(), "A");
    assert!(entries.next().unwrap().is_err());
    assert!(entries.next().is_none());
    assert!(entries.next().is_none());
}

#[test]
fn structural_count_length_and_checksum_corruptions_fail_closed() {
    let base = || {
        FixtureBuilder::mdx([("alpha", "A"), ("omega", "O")])
            .key_blocks(vec![1, 1])
            .record_blocks(vec![1, 1])
            .build()
    };

    let mut keyword_checksum = base();
    let checksum_byte = keyword_checksum.layout.keyword_header_offset + 40;
    keyword_checksum.bytes[checksum_byte] ^= 0x40;
    let file = keyword_checksum.write("bad-keyword-header-checksum");
    assert!(MdxFile::open(file.path()).is_err());

    let mut header_checksum = base();
    header_checksum.bytes[header_checksum.layout.header_checksum_offset] ^= 0x40;
    let file = header_checksum.write("bad-header-checksum");
    assert!(MdxFile::open(file.path()).is_err());

    let mut key_index_checksum = base();
    let key_index = key_index_checksum.layout.key_index_block.clone();
    key_index_checksum.corrupt_block_checksum(&key_index);
    let file = key_index_checksum.write("bad-key-index-checksum");
    assert!(MdxFile::open(file.path()).is_err());

    let mut key_entry_count = base();
    key_entry_count.set_keyword_u64(1, 3);
    let file = key_entry_count.write("bad-key-entry-count");
    assert!(MdxFile::open(file.path()).is_err());

    let mut key_blocks_len = base();
    let actual = key_blocks_len
        .layout
        .key_blocks
        .iter()
        .map(|range| range.end - range.start)
        .sum::<usize>();
    key_blocks_len.set_keyword_u64(4, u64::try_from(actual + 1).unwrap());
    let file = key_blocks_len.write("bad-key-blocks-length");
    assert!(MdxFile::open(file.path()).is_err());

    let mut record_blocks_len = base();
    let actual = record_blocks_len
        .layout
        .record_blocks
        .iter()
        .map(|range| range.end - range.start)
        .sum::<usize>();
    record_blocks_len.set_record_u64(3, u64::try_from(actual + 1).unwrap());
    let file = record_blocks_len.write("bad-record-blocks-length");
    assert!(MdxFile::open(file.path()).is_err());

    let mut short_record_index = base();
    short_record_index.set_record_u64(2, 8);
    let file = short_record_index.write("short-record-index");
    assert!(MdxFile::open(file.path()).is_err());

    let trailing_key_index = FixtureBuilder::mdx([("alpha", "A")])
        .key_index_trailing_bytes(vec![0xde, 0xad])
        .build();
    let file = trailing_key_index.write("trailing-key-index");
    assert!(MdxFile::open(file.path()).is_err());
}

#[test]
fn inverted_record_offsets_are_rejected_during_lazy_key_block_validation() {
    let fixture = FixtureBuilder::mdx([("alpha", "A"), ("omega", "O")])
        .record_starts(vec![2, 1])
        .build();
    let dictionary_file = fixture.write("inverted-record-offsets");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let mut keys = dictionary.keys();
    assert!(keys.next().unwrap().is_err());
    assert!(keys.next().is_none());
    assert!(dictionary.entry_at(KeyOrdinal::new(0)).is_err());
}

#[test]
fn cross_block_record_offset_decrease_is_rejected_by_direct_ordinal_access() {
    let fixture = FixtureBuilder::mdx([("alpha", "AAAAA"), ("middle", "B"), ("omega", "C")])
        .key_blocks(vec![1, 2])
        .record_starts(vec![5, 1, 6])
        .build();
    let dictionary_file = fixture.write("cross-block-inverted-record-offsets");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    assert!(dictionary.key_at(KeyOrdinal::new(2)).is_err());
    assert!(dictionary.entry_at(KeyOrdinal::new(2)).is_err());
    assert!(dictionary.locate("omega").is_err());

    let fixture = FixtureBuilder::mdd([
        ("\\alpha.bin", vec![0; 5]),
        ("\\middle.bin", vec![1]),
        ("\\omega.bin", vec![2]),
    ])
    .key_blocks(vec![1, 2])
    .record_starts(vec![5, 1, 6])
    .build();
    let dictionary_file = fixture.write("cross-block-inverted-resource-offsets");
    let resources = MddFile::open(dictionary_file.path()).unwrap();

    assert!(resources.key_at(KeyOrdinal::new(2)).is_err());
    assert!(resources.span_at(KeyOrdinal::new(2)).is_err());
}

#[test]
fn rejects_trailing_record_index_bytes() {
    let fixture = FixtureBuilder::mdx([("alpha", "A")])
        .record_index_trailing_bytes(vec![0xde, 0xad, 0xbe, 0xef])
        .build();
    let dictionary_file = fixture.write("trailing-record-index");

    assert!(MdxFile::open(dictionary_file.path()).is_err());
}

#[test]
fn rejects_file_truncated_inside_the_final_lazy_record_block() {
    let mut fixture = FixtureBuilder::mdx([("alpha", "payload")]).build();
    fixture.bytes.pop();
    let dictionary_file = fixture.write("truncated-final-record-block");

    assert!(MdxFile::open(dictionary_file.path()).is_err());
}

#[test]
fn streamed_mdd_span_propagates_destination_errors() {
    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("synthetic destination failure"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let fixture = FixtureBuilder::mdd([("\\resource.bin", vec![1, 2, 3])]).build();
    let dictionary_file = fixture.write("failing-mdd-writer");
    let dictionary = MddFile::open(dictionary_file.path()).unwrap();
    let span = dictionary.span_at(KeyOrdinal::new(0)).unwrap().unwrap();

    assert!(span.copy_to(&mut FailingWriter).is_err());
}
