mod support;

use std::sync::{Arc, Barrier};

use mdictlib::{Error, KeyOrdinal, Limits, MddFile, MdxFile, OpenOptions, Passcode};

use support::{
    FixtureBuilder, FixtureCompression, FixtureEncoding, FixturePasscode, independent_ripemd128,
};

#[test]
fn independent_ripemd128_matches_published_vectors() {
    assert_eq!(
        hex(&independent_ripemd128(b"")),
        "cdf26213a150dc3ecb610f18f6b38b46"
    );
    assert_eq!(
        hex(&independent_ripemd128(b"a")),
        "86be7afa339d0fc7cfc785e72f578d33"
    );
    assert_eq!(
        hex(&independent_ripemd128(b"abc")),
        "c14a12199c66e4ba84636b0f69144c77"
    );
}

#[test]
fn full_files_decode_every_supported_mdx_text_encoding() {
    let cases = [
        (FixtureEncoding::Utf8, "UTF-8", "café😀", "définition 😀"),
        (FixtureEncoding::Utf16Le, "UTF-16LE", "词典😀", "解释😀"),
        (FixtureEncoding::Gbk, "GBK", "汉语", "解释"),
        (FixtureEncoding::Gb18030, "GB18030", "😀词", "😀定义"),
        (FixtureEncoding::Big5, "BIG5", "詞典", "解釋"),
    ];

    for (encoding, label, key, text) in cases {
        let second_key = format!("{key}二");
        let second_text = format!("{text}二");
        let fixture = FixtureBuilder::mdx([
            (key.to_owned(), text.to_owned()),
            (second_key.clone(), second_text.clone()),
        ])
        .encoding(encoding)
        .key_blocks(vec![1, 1])
        .build();
        let dictionary_file = fixture.write(&format!("encoding-{label}"));
        let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

        assert_eq!(dictionary.header().encoding_label(), Some(label));
        assert_eq!(
            dictionary
                .key_at(KeyOrdinal::new(0))
                .unwrap()
                .unwrap()
                .key(),
            key
        );
        assert_eq!(
            dictionary
                .entry_at(KeyOrdinal::new(0))
                .unwrap()
                .unwrap()
                .text(),
            text
        );
        let second = dictionary.lookup(&second_key).unwrap().unwrap();
        assert_eq!(second.key(), second_key);
        assert_eq!(second.text(), second_text);
    }
}

#[test]
fn zlib_full_file_decodes_key_index_key_blocks_and_record_blocks() {
    let fixture = FixtureBuilder::mdx([
        ("alpha", "A repeated repeated repeated record"),
        ("beta", "B repeated repeated repeated record"),
        ("omega", "O repeated repeated repeated record"),
    ])
    .key_blocks(vec![1, 1, 1])
    .compression(FixtureCompression::Zlib)
    .build();
    assert_eq!(
        &fixture.bytes[fixture.layout.key_index_block.start..][..4],
        &[2, 0, 0, 0]
    );
    assert!(
        fixture
            .layout
            .key_blocks
            .iter()
            .all(|range| { fixture.bytes[range.start..range.start + 4] == [2, 0, 0, 0] })
    );
    assert!(
        fixture
            .layout
            .record_blocks
            .iter()
            .all(|range| { fixture.bytes[range.start..range.start + 4] == [2, 0, 0, 0] })
    );

    let dictionary_file = fixture.write("zlib-all-sections");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();
    assert_eq!(dictionary.keys().count(), 3);
    assert_eq!(
        dictionary.lookup("beta").unwrap().unwrap().text(),
        "B repeated repeated repeated record"
    );
}

#[test]
fn mixed_none_and_zlib_blocks_share_one_reader_path() {
    let fixture = FixtureBuilder::mdx([("alpha", "first"), ("omega", "second")])
        .key_blocks(vec![1, 1])
        .mixed_compression(
            FixtureCompression::Zlib,
            FixtureCompression::None,
            FixtureCompression::Zlib,
        )
        .build();
    let dictionary_file = fixture.write("mixed-none-zlib");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    assert_eq!(
        dictionary
            .entries()
            .map(|entry| entry.unwrap().text().to_owned())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
}

#[test]
fn zlib_mdd_preserves_utf16_keys_and_opaque_binary_resources() {
    let fixture = FixtureBuilder::mdd([
        ("\\图像.bin", vec![0x00, 0xff, 0x10, 0x80, 0x00]),
        ("\\音频.bin", vec![0x7f; 512]),
    ])
    .key_blocks(vec![1, 1])
    .compression(FixtureCompression::Zlib)
    .build();
    let dictionary_file = fixture.write("zlib-mdd-binary");
    let dictionary = MddFile::open(dictionary_file.path()).unwrap();

    let first = dictionary.lookup("\\图像.bin").unwrap().unwrap();
    assert_eq!(first.bytes(), [0x00, 0xff, 0x10, 0x80, 0x00]);
    let second = dictionary.lookup_span("\\音频.bin").unwrap().unwrap();
    assert_eq!(second.len(), 512);
    assert_eq!(second.read().unwrap().bytes(), vec![0x7f; 512]);
}

#[cfg(feature = "lzo")]
#[test]
fn literal_only_lzo_full_file_decodes_all_block_classes() {
    let key = format!("long-key-{}", "k".repeat(300));
    let text = format!("long-record-{}", "r".repeat(700));
    let fixture = FixtureBuilder::mdx([(key.clone(), text.clone())])
        .compression(FixtureCompression::Lzo)
        .build();
    assert_eq!(
        &fixture.bytes[fixture.layout.key_index_block.start..][..4],
        &[1, 0, 0, 0]
    );
    let dictionary_file = fixture.write("lzo-literal-full-file");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let entry = dictionary.lookup(&key).unwrap().unwrap();
    assert_eq!(entry.key(), key);
    assert_eq!(entry.text(), text);
}

#[cfg(feature = "lzo")]
#[test]
fn literal_only_lzo_mdd_full_file_decodes_all_block_classes() {
    let key = format!("\\resource-{}", "k".repeat(300));
    let bytes = (0..700)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let fixture = FixtureBuilder::mdd([(key.clone(), bytes.clone())])
        .compression(FixtureCompression::Lzo)
        .build();
    assert_eq!(
        &fixture.bytes[fixture.layout.key_index_block.start..][..4],
        &[1, 0, 0, 0]
    );
    assert!(
        fixture
            .layout
            .key_blocks
            .iter()
            .all(|range| fixture.bytes[range.start..range.start + 4] == [1, 0, 0, 0])
    );
    assert!(
        fixture
            .layout
            .record_blocks
            .iter()
            .all(|range| fixture.bytes[range.start..range.start + 4] == [1, 0, 0, 0])
    );

    let dictionary_file = fixture.write("lzo-literal-mdd-full-file");
    let dictionary = MddFile::open(dictionary_file.path()).unwrap();
    let span = dictionary.lookup_span(&key).unwrap().unwrap();
    let mut streamed = Vec::new();
    assert_eq!(span.copy_to(&mut streamed).unwrap(), bytes.len() as u64);
    assert_eq!(streamed, bytes);
    assert_eq!(span.read().unwrap().bytes(), bytes);
}

#[cfg(not(feature = "lzo"))]
#[test]
fn lzo_full_file_requires_the_optional_feature() {
    let fixture = FixtureBuilder::mdx([("alpha", "record")])
        .compression(FixtureCompression::Lzo)
        .build();
    let dictionary_file = fixture.write("lzo-feature-required");

    assert!(matches!(
        MdxFile::open(dictionary_file.path()),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn encrypted_keyword_index_opens_without_passcode_and_remains_lazy() {
    let fixture = FixtureBuilder::mdx([
        ("alpha", "first encrypted-index record"),
        ("omega", "second encrypted-index record"),
    ])
    .key_blocks(vec![1, 1])
    .compression(FixtureCompression::Zlib)
    .encrypt_keyword_index()
    .build();
    let dictionary_file = fixture.write("encrypted-keyword-index");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    assert_eq!(dictionary.header().encryption_bits(), 2);
    assert!(dictionary.header().has_encrypted_keyword_index());
    assert_eq!(
        dictionary.lookup("omega").unwrap().unwrap().text(),
        "second encrypted-index record"
    );
}

#[test]
fn encrypted_keyword_header_requires_and_validates_passcode_end_to_end() {
    let fixture_passcode = FixturePasscode::new(
        "0123456789abcdef0123456789abcdef",
        "fixture-user@example.com",
    );
    let fixture = FixtureBuilder::mdx([("alpha", "secret header record")])
        .encrypt_keyword_header(fixture_passcode.clone())
        .build();
    let dictionary_file = fixture.write("encrypted-keyword-header");

    assert!(matches!(
        MdxFile::open(dictionary_file.path()),
        Err(Error::MissingPasscode)
    ));

    let wrong = OpenOptions::new().with_passcode(
        Passcode::new(
            "fedcba9876543210fedcba9876543210",
            &fixture_passcode.user_id,
        )
        .unwrap(),
    );
    assert!(MdxFile::open_with_options(dictionary_file.path(), &wrong).is_err());

    let correct = OpenOptions::new().with_passcode(
        Passcode::new(&fixture_passcode.reg_code_hex, &fixture_passcode.user_id).unwrap(),
    );
    let dictionary = MdxFile::open_with_options(dictionary_file.path(), &correct).unwrap();
    assert_eq!(dictionary.header().encryption_bits(), 1);
    assert!(dictionary.header().has_encrypted_keyword_header());
    assert_eq!(
        dictionary.lookup("alpha").unwrap().unwrap().text(),
        "secret header record"
    );
}

#[test]
fn combined_keyword_header_and_index_encryption_composes_with_zlib() {
    let fixture_passcode = FixturePasscode::new(
        "00112233445566778899aabbccddeeff",
        "combined-encryption-user",
    );
    let fixture = FixtureBuilder::mdx([("combined", "encrypted and compressed")])
        .compression(FixtureCompression::Zlib)
        .encrypt_keyword_index()
        .encrypt_keyword_header(fixture_passcode.clone())
        .build();
    let dictionary_file = fixture.write("combined-encryption-zlib");
    let options = OpenOptions::new().with_passcode(
        Passcode::new(&fixture_passcode.reg_code_hex, &fixture_passcode.user_id).unwrap(),
    );
    let dictionary = MdxFile::open_with_options(dictionary_file.path(), &options).unwrap();

    assert_eq!(dictionary.header().encryption_bits(), 3);
    assert_eq!(
        dictionary.lookup("combined").unwrap().unwrap().text(),
        "encrypted and compressed"
    );
}

#[test]
fn concurrent_first_locate_is_deterministic_for_all_callers() {
    const WORKING_MEMORY_BYTES: usize = 128 * 1024;

    let mut entries = Vec::new();
    let mut expected_ordinals = Vec::new();
    for ordinal in 0..256u64 {
        let key = if ordinal % 31 == 0 {
            expected_ordinals.push(ordinal);
            "shared".to_owned()
        } else {
            format!("key-{ordinal:03}-{}", "x".repeat(150))
        };
        entries.push((key, format!("record-{ordinal}")));
    }
    let locator_text_bytes = entries
        .iter()
        .map(|(key, _)| key.len().checked_mul(2).unwrap())
        .sum::<usize>();
    assert!(locator_text_bytes < WORKING_MEMORY_BYTES);
    assert!(locator_text_bytes * 2 > WORKING_MEMORY_BYTES);

    let fixture = FixtureBuilder::mdx(entries)
        .key_blocks(vec![16; 16])
        .compression(FixtureCompression::Zlib)
        .build();
    let dictionary_file = fixture.write("concurrent-first-locate");
    // The fixture's retained locator is about 90 KiB. This aggregate budget
    // accommodates one locator plus the open metadata and one decoded key
    // block, but cannot accommodate two locator builds at once. All first
    // callers succeeding therefore exercise the locator's single-flight
    // construction as well as deterministic publication.
    let options = OpenOptions::new()
        .with_limits(Limits::new().with_working_memory_bytes(WORKING_MEMORY_BYTES));
    let dictionary =
        Arc::new(MdxFile::open_with_options(dictionary_file.path(), &options).unwrap());
    let barrier = Arc::new(Barrier::new(17));

    let workers = (0..16)
        .map(|_| {
            let dictionary = Arc::clone(&dictionary);
            let barrier = Arc::clone(&barrier);
            let expected_ordinals = expected_ordinals.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..8 {
                    let matches = dictionary.locate("shared").unwrap().unwrap();
                    assert_eq!(
                        matches.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
                        expected_ordinals
                    );
                    assert_eq!(
                        dictionary
                            .entry_at(matches.first())
                            .unwrap()
                            .unwrap()
                            .text(),
                        "record-0"
                    );
                }
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn public_per_open_limits_reach_each_major_parser_boundary() {
    let fixture = FixtureBuilder::mdx([("alpha", "record payload")]).build();
    let dictionary_file = fixture.write("public-per-open-limits");

    let open_with = |limits| {
        MdxFile::open_with_options(
            dictionary_file.path(),
            &OpenOptions::new().with_limits(limits),
        )
    };

    assert_limit(
        open_with(Limits::new().with_header_xml_bytes(1)).unwrap_err(),
        "header_xml_bytes",
    );
    assert_limit(
        open_with(Limits::new().with_key_index_bytes(1)).unwrap_err(),
        "key_index_decompressed_bytes",
    );
    assert_limit(
        open_with(Limits::new().with_compressed_block_bytes(1)).unwrap_err(),
        "compressed_block_bytes",
    );

    let dictionary = open_with(Limits::new().with_materialized_record_bytes(1)).unwrap();
    assert_limit(
        dictionary.entry_at(KeyOrdinal::new(0)).unwrap_err(),
        "materialized_record_bytes",
    );

    let dictionary = open_with(Limits::new().with_locator_entries(0)).unwrap();
    assert_limit(dictionary.locate("alpha").unwrap_err(), "locator_entries");

    let dictionary = open_with(Limits::new().with_locator_bytes(1)).unwrap();
    assert_limit(dictionary.locate("alpha").unwrap_err(), "locator_bytes");

    assert_limit(
        open_with(Limits::new().with_working_memory_bytes(0)).unwrap_err(),
        "working_memory_bytes",
    );
}

#[test]
fn oversized_sparse_declarations_hit_limits_without_large_reads_or_allocations() {
    const MIB: u64 = 1024 * 1024;

    let base = || FixtureBuilder::mdx([("alpha", "record")]).build();

    let mut header = base();
    header.bytes[..4].copy_from_slice(&(16 * MIB as u32 + 1).to_be_bytes());
    let file = header.write_sparse("sparse-oversized-header", 32 * MIB);
    assert_limit(MdxFile::open(file.path()).unwrap_err(), "header_xml_bytes");

    let mut key_index = base();
    key_index.set_keyword_u64(3, 256 * MIB + 1);
    let file = key_index.write_sparse("sparse-oversized-key-index", 300 * MIB);
    assert_limit(
        MdxFile::open(file.path()).unwrap_err(),
        "key_index_compressed_bytes",
    );

    let mut decoded_key_index = base();
    decoded_key_index.set_keyword_u64(2, 64 * MIB + 1);
    let file = decoded_key_index.write_sparse("sparse-oversized-decoded-key-index", 80 * MIB);
    assert_limit(
        MdxFile::open(file.path()).unwrap_err(),
        "key_index_decompressed_bytes",
    );

    // For the one-entry "alpha" fixture, the key-index block metadata starts
    // with count (8), then two 8-byte summary fields. Compressed and decoded
    // key-block sizes therefore begin at payload offsets 24 and 32.
    let mut key_block = base();
    let key_index_range = key_block.layout.key_index_block.clone();
    key_block.set_uncompressed_payload_bytes(&key_index_range, 24, &(256 * MIB + 1).to_be_bytes());
    key_block.set_keyword_u64(4, 256 * MIB + 1);
    let file = key_block.write_sparse("sparse-oversized-key-block", 300 * MIB);
    assert_limit(
        MdxFile::open(file.path()).unwrap_err(),
        "keyword_block_compressed_bytes",
    );

    let mut decoded_key_block = base();
    let key_index_range = decoded_key_block.layout.key_index_block.clone();
    decoded_key_block.set_uncompressed_payload_bytes(
        &key_index_range,
        32,
        &(512 * MIB + 1).to_be_bytes(),
    );
    let file = decoded_key_block.write_sparse("sparse-oversized-decoded-key-block", 16 * MIB);
    assert_limit(
        MdxFile::open(file.path()).unwrap_err(),
        "keyword_block_decompressed_bytes",
    );

    let mut record_index = base();
    let blocks = 64 * MIB / 16 + 1;
    record_index.set_record_u64(0, blocks);
    record_index.set_record_u64(1, blocks);
    record_index.set_record_u64(2, blocks * 16);
    let file = record_index.write_sparse("sparse-oversized-record-index", 80 * MIB);
    assert_limit(
        MdxFile::open(file.path()).unwrap_err(),
        "record_index_bytes",
    );

    let mut record_block = base();
    let record_index_offset = record_block.layout.record_index.start;
    record_block.bytes[record_index_offset..record_index_offset + 8]
        .copy_from_slice(&(256 * MIB + 1).to_be_bytes());
    record_block.set_record_u64(3, 256 * MIB + 1);
    let file = record_block.write_sparse("sparse-oversized-record-block", 300 * MIB);
    assert_limit(
        MdxFile::open(file.path()).unwrap_err(),
        "record_block_compressed_bytes",
    );

    let mut decoded_record_block = base();
    let record_index_offset = decoded_record_block.layout.record_index.start + 8;
    decoded_record_block.bytes[record_index_offset..record_index_offset + 8]
        .copy_from_slice(&(512 * MIB + 1).to_be_bytes());
    let file = decoded_record_block.write_sparse("sparse-oversized-decoded-record-block", 16 * MIB);
    assert_limit(
        MdxFile::open(file.path()).unwrap_err(),
        "record_block_decompressed_bytes",
    );
}

fn assert_limit(error: Error, expected_limit: &str) {
    match error {
        Error::LimitExceeded { limit, .. } => assert_eq!(limit, expected_limit),
        other => panic!("expected {expected_limit} limit error, got {other}"),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
