mod support;

use std::fs::{self, FileTimes, OpenOptions};
use std::io::{Cursor, Seek, SeekFrom, Write};
use std::mem::size_of;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mdictlib::{
    Error, KEY_INDEX_FORMAT_REVISION, KEY_INDEX_NORMALIZATION_REVISION, KEY_INDEX_PARSER_REVISION,
    KEY_INDEX_REVISION, KeyIndexOptions, KeyIndexRejection, KeyIndexSourceIdentity, KeyOrdinal,
    Limits, MatchBasis, MdxFile, OpenOptions as MdictOpenOptions,
};
use support::v1::{V1BlockCoding, V1FixtureBuilder};
use support::{FixtureBuilder, independent_adler32};

const HEADER_CHECKSUM_BYTES: usize = 4;
const HEADER_BYTES: usize = 224;
const FIRST_DESCRIPTOR_OFFSET: usize = 88;
const DESCRIPTOR_BYTES: usize = 32;
const CHECKSUM_START_OFFSET: usize = 16;
const CHUNK_BYTES_OFFSET: usize = 40;

fn artifact_path(directory: &Path, name: &str) -> PathBuf {
    directory.join(format!("{name}.mdx-key-index"))
}

struct WriteSeekOnly(Cursor<Vec<u8>>);

impl Write for WriteSeekOnly {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl Seek for WriteSeekOnly {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.0.seek(position)
    }
}

fn build_index(
    dictionary: &MdxFile,
    directory: &Path,
    name: &str,
    options: &KeyIndexOptions,
) -> (PathBuf, KeyIndexSourceIdentity) {
    let path = artifact_path(directory, name);
    let report = dictionary
        .build_key_index_to_path(&path, options, || false)
        .unwrap();
    assert_eq!(report.bytes_written(), fs::metadata(&path).unwrap().len());
    (path, report.source_identity())
}

fn assert_index_matches_locator(dictionary: &MdxFile, name: &str) {
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new()
        .with_build_memory_bytes(512)
        .with_chunk_bytes(64)
        .with_scratch_directory(directory.path());
    assert_eq!(dictionary.memory_usage().unwrap().locator_bytes(), 0);
    let (path, proof) = build_index(dictionary, directory.path(), name, &options);
    assert_eq!(dictionary.memory_usage().unwrap().locator_bytes(), 0);
    let index = dictionary.open_key_index(path, &proof, &options).unwrap();

    for query in ["target", "TARGET", "apple", "missing"] {
        let expected = dictionary.locate(query).unwrap();
        let actual = dictionary.locate_with_key_index(&index, query).unwrap();
        assert_eq!(
            actual.as_ref().map(|matches| matches.basis()),
            expected.as_ref().map(|matches| matches.basis()),
            "basis differs for {query:?}",
        );
        assert_eq!(
            actual
                .as_ref()
                .map(|matches| matches.iter().collect::<Vec<_>>()),
            expected
                .as_ref()
                .map(|matches| matches.iter().collect::<Vec<_>>()),
            "ordinals differ for {query:?}",
        );
    }

    let expected_prefix = dictionary
        .prefix_keys("app", 10)
        .unwrap()
        .into_iter()
        .map(|entry| (entry.ordinal(), entry.into_key()))
        .collect::<Vec<_>>();
    let actual_prefix = dictionary
        .prefix_keys_with_index(&index, "app", 10)
        .unwrap()
        .into_iter()
        .map(|entry| (entry.ordinal(), entry.into_key()))
        .collect::<Vec<_>>();
    assert_eq!(actual_prefix, expected_prefix);

    let mut expected_scan = Vec::new();
    dictionary
        .scan_normalized_keys(|ordinal, normalized| {
            expected_scan.push((ordinal, normalized.to_owned()));
            ControlFlow::Continue(())
        })
        .unwrap();
    let mut actual_scan = Vec::new();
    dictionary
        .scan_normalized_keys_with_index(&index, |ordinal, normalized| {
            actual_scan.push((ordinal, normalized.to_owned()));
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!(actual_scan, expected_scan);
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn section_offset(bytes: &[u8], section: usize) -> usize {
    usize::try_from(read_u64(
        bytes,
        FIRST_DESCRIPTOR_OFFSET + section * DESCRIPTOR_BYTES,
    ))
    .unwrap()
}

fn rewrite_section_chunk_checksum(bytes: &mut [u8], section: usize, relative_offset: usize) {
    let descriptor = FIRST_DESCRIPTOR_OFFSET + section * DESCRIPTOR_BYTES;
    let section_start = section_offset(bytes, section);
    let section_len = usize::try_from(read_u64(bytes, descriptor + 8)).unwrap();
    let checksum_start =
        usize::try_from(read_u64(bytes, descriptor + CHECKSUM_START_OFFSET)).unwrap();
    let chunk_bytes = usize::try_from(read_u32(bytes, CHUNK_BYTES_OFFSET)).unwrap();
    let chunk = relative_offset / chunk_bytes;
    let chunk_start = section_start + chunk * chunk_bytes;
    let chunk_end = (chunk_start + chunk_bytes).min(section_start + section_len);
    let checksum = independent_adler32(&bytes[chunk_start..chunk_end]);
    let header_len = usize::try_from(read_u64(bytes, 16)).unwrap();
    let checksum_offset = header_len + (checksum_start + chunk) * size_of::<u32>();
    bytes[checksum_offset..checksum_offset + 4].copy_from_slice(&checksum.to_le_bytes());
}

fn rewrite_header_checksum(bytes: &mut [u8]) {
    let header_len = usize::try_from(read_u64(bytes, 16)).unwrap();
    let checksum_at = header_len - HEADER_CHECKSUM_BYTES;
    let checksum = independent_adler32(&bytes[..checksum_at]);
    bytes[checksum_at..header_len].copy_from_slice(&checksum.to_le_bytes());
}

fn fnv1a(raw: &str) -> u32 {
    let mut hash = 2_166_136_261u32;
    for byte in raw.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

#[test]
fn revision_and_source_identity_are_stable_and_metadata_only() {
    assert_eq!(KEY_INDEX_FORMAT_REVISION, 2);
    assert_eq!(KEY_INDEX_PARSER_REVISION, 1);
    assert_eq!(KEY_INDEX_NORMALIZATION_REVISION, 1);
    assert_eq!(KEY_INDEX_REVISION, "f2-p1-n1");

    let fixture = FixtureBuilder::mdx([("alpha", "A"), ("beta", "B")]).build();
    let source = fixture.write("persistent-proof");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let first = dictionary.key_index_source_identity().unwrap();
    let second = dictionary.key_index_source_identity().unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first.source_bytes(),
        fs::metadata(source.path()).unwrap().len()
    );
    assert_eq!(first.key_count(), 2);
    assert_ne!(first.source_modified_unix_nanos(), 0);
}

#[test]
fn empty_dictionary_builds_a_valid_empty_index() {
    let fixture = FixtureBuilder::mdx(std::iter::empty::<(&str, &str)>()).build();
    let source = fixture.write("persistent-empty");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new().with_scratch_directory(directory.path());
    let (path, proof) = build_index(&dictionary, directory.path(), "empty", &options);
    let index = dictionary.open_key_index(path, &proof, &options).unwrap();
    assert!(index.is_empty());
    assert!(
        dictionary
            .locate_with_key_index(&index, "anything")
            .unwrap()
            .is_none()
    );
    assert!(
        dictionary
            .prefix_keys_with_index(&index, "a", 10)
            .unwrap()
            .is_empty()
    );
    let mut visited = false;
    dictionary
        .scan_normalized_keys_with_index(&index, |_, _| {
            visited = true;
            ControlFlow::Continue(())
        })
        .unwrap();
    assert!(!visited);
}

#[test]
fn readable_encrypted_sources_build_with_default_options() {
    let fixture = FixtureBuilder::mdx([("secret", "payload")])
        .encrypt_keyword_index()
        .build();
    let source = fixture.write("persistent-readable-encrypted-source");
    let dictionary = MdxFile::open(source.path()).unwrap();
    assert_eq!(dictionary.header().encryption_bits(), 2);

    let options = KeyIndexOptions::new();
    let mut sink = Cursor::new(Vec::new());
    let report = dictionary
        .build_key_index(&mut sink, &options, || false)
        .unwrap();
    assert_eq!(
        report.bytes_written(),
        u64::try_from(sink.get_ref().len()).unwrap()
    );

    let directory = tempfile::tempdir().unwrap();
    let (path, proof) = build_index(
        &dictionary,
        directory.path(),
        "readable-encrypted-default",
        &options,
    );
    let index = dictionary.open_key_index(path, &proof, &options).unwrap();
    let matches = dictionary
        .locate_with_key_index(&index, "SECRET")
        .unwrap()
        .expect("the encrypted source remains searchable through its default index");
    assert_eq!(matches.basis(), MatchBasis::HeaderNormalized);
    assert_eq!(matches.iter().collect::<Vec<_>>(), [KeyOrdinal::new(0)]);
}

#[test]
fn persistent_lookup_preserves_basis_duplicates_prefix_and_physical_scan() {
    let fixture = FixtureBuilder::mdx([
        ("TARGET!", "normalized zero"),
        ("target", "raw one"),
        ("Apple", "prefix two"),
        ("target", "raw three"),
        ("app-let", "prefix four"),
        ("Target?", "normalized five"),
    ])
    .strip_key_attribute("StripKey", "Yes")
    .key_blocks(vec![1, 2, 1, 2])
    .build();
    let source = fixture.write("persistent-equivalence");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new()
        .with_build_memory_bytes(512)
        .with_chunk_bytes(64)
        .with_scratch_directory(directory.path());
    let (path, proof) = build_index(&dictionary, directory.path(), "equivalence", &options);
    let before_open = dictionary.memory_usage().unwrap().current_bytes();
    let index = dictionary.open_key_index(path, &proof, &options).unwrap();
    let after_open = dictionary.memory_usage().unwrap().current_bytes();
    assert_eq!(
        after_open - before_open,
        4 * options.chunk_bytes() + 4 * size_of::<u32>()
    );
    assert_eq!(index.source_identity(), proof);
    assert_eq!(index.len(), dictionary.len());
    assert!(!index.is_empty());

    let exact = dictionary
        .locate_with_key_index(&index, "target")
        .unwrap()
        .unwrap();
    assert_eq!(exact.basis(), MatchBasis::RawExact);
    assert_eq!(
        exact.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
        [1, 3]
    );
    let with_matches = dictionary.memory_usage().unwrap().current_bytes();
    drop(exact);
    let without_matches = dictionary.memory_usage().unwrap().current_bytes();
    // Raw filtering reuses the four-row normalized equal-range allocation in
    // place, so its conservative reservation remains live with the result.
    assert_eq!(with_matches - without_matches, 4 * size_of::<u32>());

    let normalized = dictionary
        .locate_with_key_index(&index, "TARGET")
        .unwrap()
        .unwrap();
    assert_eq!(normalized.basis(), MatchBasis::HeaderNormalized);
    assert_eq!(
        normalized.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
        [0, 1, 3, 5]
    );
    assert!(
        dictionary
            .locate_with_key_index(&index, "missing")
            .unwrap()
            .is_none()
    );

    let prefix = dictionary
        .prefix_keys_with_index(&index, "APP", 10)
        .unwrap()
        .into_iter()
        .map(|entry| (entry.ordinal().get(), entry.into_key()))
        .collect::<Vec<_>>();
    assert_eq!(prefix, [(2, "Apple".to_owned()), (4, "app-let".to_owned())]);

    let mut scanned = Vec::new();
    dictionary
        .scan_normalized_keys_with_index(&index, |ordinal, normalized| {
            scanned.push((ordinal.get(), normalized.to_owned()));
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!(
        scanned,
        [
            (0, "target".to_owned()),
            (1, "target".to_owned()),
            (2, "apple".to_owned()),
            (3, "target".to_owned()),
            (4, "applet".to_owned()),
            (5, "target".to_owned()),
        ]
    );
}

#[test]
fn checksum_directory_uses_a_bounded_lazy_page_at_open() {
    const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SECTION_COUNT: usize = 4;
    const DESCRIPTOR_BYTES: usize = 32;
    const CHECKSUM_COUNT_OFFSET: usize = 24;

    let fixture = FixtureBuilder::mdx(std::iter::repeat_n((KEY, "value"), 1024)).build();
    let source = fixture.write("persistent-checksum-memory");
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new()
        .with_chunk_bytes(64)
        .with_scratch_directory(directory.path());
    let builder = MdxFile::open(source.path()).unwrap();
    let (path, proof) = build_index(&builder, directory.path(), "checksum-memory", &options);

    let artifact = fs::read(&path).unwrap();
    let checksum_count = (0..SECTION_COUNT)
        .map(|section| {
            usize::try_from(read_u64(
                &artifact,
                FIRST_DESCRIPTOR_OFFSET + section * DESCRIPTOR_BYTES + CHECKSUM_COUNT_OFFSET,
            ))
            .unwrap()
        })
        .sum::<usize>();
    let checksum_bytes = checksum_count * size_of::<u32>();
    let chunk_cache_bytes = SECTION_COUNT * options.chunk_bytes();
    let before = builder.memory_usage().unwrap().current_bytes();
    assert!(checksum_bytes > 4 * 1024);
    let index = builder.open_key_index(&path, &proof, &options).unwrap();
    let after = builder.memory_usage().unwrap().current_bytes();
    assert_eq!(after - before, 4 * 1024 + chunk_cache_bytes);
    drop(index);
    assert_eq!(builder.memory_usage().unwrap().current_bytes(), before);
}

#[test]
fn persistent_results_match_the_locator_across_both_wire_versions() {
    let entries = [
        ("TARGET!", "normalized zero"),
        ("target", "raw one"),
        ("Apple", "prefix two"),
        ("target", "raw three"),
        ("app-let", "prefix four"),
        ("Target?", "normalized five"),
    ];

    let v1 = V1FixtureBuilder::mdx(entries)
        .coding(V1BlockCoding::None)
        .strip_key_attribute("StripKey", "Yes")
        .key_blocks([1, 2, 1, 2])
        .build();
    let v1_source = v1.write("persistent-parity-v1");
    let v1_dictionary = MdxFile::open(v1_source.path()).unwrap();
    assert_index_matches_locator(&v1_dictionary, "parity-v1");

    let v2 = FixtureBuilder::mdx(entries)
        .strip_key_attribute("StripKey", "Yes")
        .key_blocks([1, 2, 1, 2])
        .build();
    let v2_source = v2.write("persistent-parity-v2");
    let v2_dictionary = MdxFile::open(v2_source.path()).unwrap();
    assert_index_matches_locator(&v2_dictionary, "parity-v2");
}

#[test]
fn a_raw_digest_collision_never_establishes_raw_equality() {
    // These unequal strings collide under the index's FNV-1a filter and both
    // normalize to "collision" when StripKey is enabled.
    let stored = "collision<]<#!!";
    let absent_query = "collision@*($!!";
    assert_ne!(stored, absent_query);
    assert_eq!(fnv1a(stored), fnv1a(absent_query));
    let fixture = FixtureBuilder::mdx([(stored, "payload")])
        .strip_key_attribute("StripKey", "Yes")
        .build();
    let source = fixture.write("persistent-digest-collision");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new().with_scratch_directory(directory.path());
    let (path, proof) = build_index(&dictionary, directory.path(), "digest-collision", &options);
    let index = dictionary.open_key_index(path, &proof, &options).unwrap();

    let matches = dictionary
        .locate_with_key_index(&index, absent_query)
        .unwrap()
        .unwrap();
    assert_eq!(matches.basis(), MatchBasis::HeaderNormalized);
    assert_eq!(matches.first(), KeyOrdinal::new(0));
}

#[test]
fn incompatible_revision_and_expected_identity_are_structured_rejections() {
    let fixture = FixtureBuilder::mdx([("alpha", "A")]).build();
    let source = fixture.write("persistent-rejection");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new().with_scratch_directory(directory.path());
    let (path, proof) = build_index(
        &dictionary,
        directory.path(),
        "structured-rejection",
        &options,
    );

    let wrong = KeyIndexSourceIdentity::new(
        proof.source_bytes(),
        proof.source_modified_unix_nanos() + 1,
        proof.key_count(),
    );
    assert!(matches!(
        dictionary.open_key_index(&path, &wrong, &options),
        Err(Error::KeyIndexRejected(
            KeyIndexRejection::SourceIdentityMismatch
        ))
    ));

    let original = fs::read(&path).unwrap();
    let format_path = artifact_path(directory.path(), "format-rejection");
    let mut bytes = original.clone();
    bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    fs::write(&format_path, bytes).unwrap();
    assert!(matches!(
        dictionary.open_key_index(&format_path, &proof, &options),
        Err(Error::KeyIndexRejected(
            KeyIndexRejection::UnsupportedFormatRevision { found: u32::MAX }
        ))
    ));

    let mut bytes = original;
    bytes[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
    rewrite_header_checksum(&mut bytes);
    fs::write(&path, bytes).unwrap();
    assert!(matches!(
        dictionary.open_key_index(&path, &proof, &options),
        Err(Error::KeyIndexRejected(
            KeyIndexRejection::IncompatibleParserRevision { found: u32::MAX }
        ))
    ));
}

#[test]
fn source_length_change_rejects_an_otherwise_valid_index_at_open() {
    let fixture = FixtureBuilder::mdx([("alpha", "A")]).build();
    let source = fixture.write("persistent-source-length-rejection");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new().with_scratch_directory(directory.path());
    let (path, identity) = build_index(
        &dictionary,
        directory.path(),
        "source-length-rejection",
        &options,
    );
    OpenOptions::new()
        .append(true)
        .open(source.path())
        .unwrap()
        .write_all(&[0])
        .unwrap();

    assert!(matches!(
        dictionary.open_key_index(path, &identity, &options),
        Err(Error::KeyIndexRejected(
            KeyIndexRejection::SourceLengthMismatch { .. }
        ))
    ));
}

#[test]
fn hostile_section_geometry_is_a_structured_rejection() {
    let fixture = FixtureBuilder::mdx([("alpha", "A")]).build();
    let source = fixture.write("persistent-hostile-geometry");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new().with_scratch_directory(directory.path());
    let (path, proof) = build_index(&dictionary, directory.path(), "hostile-geometry", &options);
    let mut bytes = fs::read(&path).unwrap();
    bytes[FIRST_DESCRIPTOR_OFFSET..FIRST_DESCRIPTOR_OFFSET + 8]
        .copy_from_slice(&(u64::MAX - 3).to_le_bytes());
    rewrite_header_checksum(&mut bytes);
    fs::write(&path, bytes).unwrap();

    assert!(matches!(
        dictionary.open_key_index(&path, &proof, &options),
        Err(Error::KeyIndexRejected(KeyIndexRejection::InvalidLayout(_)))
    ));
    assert_eq!(dictionary.lookup("alpha").unwrap().unwrap().text(), "A");
}

#[test]
fn section_corruption_is_detected_when_the_chunk_is_first_used() {
    let fixture = FixtureBuilder::mdx([("alpha", "A"), ("beta", "B")]).build();
    let source = fixture.write("persistent-corruption");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new()
        .with_chunk_bytes(64)
        .with_scratch_directory(directory.path());
    let (path, proof) = build_index(
        &dictionary,
        directory.path(),
        "section-corruption",
        &options,
    );
    let bytes = fs::read(&path).unwrap();
    let text_offset = read_u64(&bytes, FIRST_DESCRIPTOR_OFFSET);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.seek(SeekFrom::Start(text_offset)).unwrap();
    file.write_all(&[bytes[usize::try_from(text_offset).unwrap()] ^ 0x80])
        .unwrap();
    file.flush().unwrap();

    // Section payloads stay lazy: the compact header still opens.
    let index = dictionary.open_key_index(&path, &proof, &options).unwrap();
    assert!(matches!(
        dictionary.locate_with_key_index(&index, "alpha"),
        Err(Error::KeyIndexRejected(
            KeyIndexRejection::ChecksumMismatch {
                section: "text",
                ..
            }
        ))
    ));
}

#[test]
fn checksum_valid_out_of_range_ordinal_is_rejected_lazily() {
    const ORDER_SECTION: usize = 3;

    let fixture = FixtureBuilder::mdx([("alpha", "A")]).build();
    let source = fixture.write("persistent-invalid-order-ordinal");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new()
        .with_chunk_bytes(64)
        .with_scratch_directory(directory.path());
    let (path, identity) = build_index(
        &dictionary,
        directory.path(),
        "invalid-order-ordinal",
        &options,
    );
    let mut bytes = fs::read(&path).unwrap();
    let order_offset = section_offset(&bytes, ORDER_SECTION);
    bytes[order_offset..order_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    rewrite_section_chunk_checksum(&mut bytes, ORDER_SECTION, 0);
    fs::write(&path, bytes).unwrap();

    let index = dictionary
        .open_key_index(&path, &identity, &options)
        .unwrap();
    assert!(matches!(
        dictionary.locate_with_key_index(&index, "alpha"),
        Err(Error::KeyIndexRejected(KeyIndexRejection::InvalidLayout(
            "text ordinal is out of range"
        )))
    ));
}

#[test]
fn checksum_valid_inverted_and_out_of_range_bounds_are_rejected_lazily() {
    const TEXT_SECTION: usize = 0;
    const BOUNDS_SECTION: usize = 1;

    let fixture = FixtureBuilder::mdx([("alpha", "A")]).build();
    let source = fixture.write("persistent-invalid-bounds");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new()
        .with_chunk_bytes(64)
        .with_scratch_directory(directory.path());
    let (path, identity) = build_index(&dictionary, directory.path(), "valid-bounds", &options);
    let original = fs::read(&path).unwrap();
    let text_len = read_u64(
        &original,
        FIRST_DESCRIPTOR_OFFSET + TEXT_SECTION * DESCRIPTOR_BYTES + 8,
    );
    let bounds_offset = section_offset(&original, BOUNDS_SECTION);

    let inverted_path = artifact_path(directory.path(), "inverted-bounds");
    let mut inverted = original.clone();
    inverted[bounds_offset..bounds_offset + 8].copy_from_slice(&(text_len + 1).to_le_bytes());
    rewrite_section_chunk_checksum(&mut inverted, BOUNDS_SECTION, 0);
    fs::write(&inverted_path, inverted).unwrap();
    let inverted_index = dictionary
        .open_key_index(&inverted_path, &identity, &options)
        .unwrap();
    assert!(matches!(
        dictionary.locate_with_key_index(&inverted_index, "alpha"),
        Err(Error::KeyIndexRejected(KeyIndexRejection::InvalidLayout(
            "text bounds are inverted"
        )))
    ));

    let out_of_range_path = artifact_path(directory.path(), "out-of-range-bounds");
    let mut out_of_range = original;
    out_of_range[bounds_offset + 8..bounds_offset + 16]
        .copy_from_slice(&(text_len + 1).to_le_bytes());
    rewrite_section_chunk_checksum(&mut out_of_range, BOUNDS_SECTION, 8);
    fs::write(&out_of_range_path, out_of_range).unwrap();
    let out_of_range_index = dictionary
        .open_key_index(&out_of_range_path, &identity, &options)
        .unwrap();
    assert!(matches!(
        dictionary.locate_with_key_index(&out_of_range_index, "alpha"),
        Err(Error::KeyIndexRejected(KeyIndexRejection::InvalidLayout(
            "text bounds exceed text section"
        )))
    ));
}

#[test]
fn checksum_valid_invalid_utf8_is_rejected_lazily() {
    const TEXT_SECTION: usize = 0;

    let fixture = FixtureBuilder::mdx([("alpha", "A")]).build();
    let source = fixture.write("persistent-invalid-index-utf8");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new()
        .with_chunk_bytes(64)
        .with_scratch_directory(directory.path());
    let (path, identity) = build_index(
        &dictionary,
        directory.path(),
        "invalid-index-utf8",
        &options,
    );
    let mut bytes = fs::read(&path).unwrap();
    let text_offset = section_offset(&bytes, TEXT_SECTION);
    bytes[text_offset] = 0xff;
    rewrite_section_chunk_checksum(&mut bytes, TEXT_SECTION, 0);
    fs::write(&path, bytes).unwrap();

    let index = dictionary
        .open_key_index(&path, &identity, &options)
        .unwrap();
    assert!(matches!(
        dictionary.locate_with_key_index(&index, "alpha"),
        Err(Error::KeyIndexRejected(KeyIndexRejection::InvalidLayout(
            "normalized key is not UTF-8"
        )))
    ));
}

#[test]
fn cached_reads_remain_stable_and_a_new_open_detects_later_corruption() {
    let fixture = FixtureBuilder::mdx([("alpha", "A"), ("beta", "B")]).build();
    let source = fixture.write("persistent-later-corruption");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new()
        .with_chunk_bytes(64)
        .with_scratch_directory(directory.path());
    let (path, proof) = build_index(&dictionary, directory.path(), "later-corruption", &options);
    let index = dictionary.open_key_index(&path, &proof, &options).unwrap();
    assert!(
        dictionary
            .locate_with_key_index(&index, "alpha")
            .unwrap()
            .is_some()
    );

    let bytes = fs::read(&path).unwrap();
    let text_offset = read_u64(&bytes, FIRST_DESCRIPTOR_OFFSET);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.seek(SeekFrom::Start(text_offset)).unwrap();
    file.write_all(&[bytes[usize::try_from(text_offset).unwrap()] ^ 0x80])
        .unwrap();
    file.flush().unwrap();

    let mut scanned = Vec::new();
    dictionary
        .scan_normalized_keys_with_index(&index, |_, normalized| {
            scanned.push(normalized.to_owned());
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!(scanned, ["alpha", "beta"]);

    let reopened = dictionary.open_key_index(&path, &proof, &options).unwrap();
    assert!(matches!(
        dictionary.locate_with_key_index(&reopened, "alpha"),
        Err(Error::KeyIndexRejected(
            KeyIndexRejection::ChecksumMismatch {
                section: "text",
                ..
            }
        ))
    ));
}

#[test]
fn writer_builds_directly_into_a_seekable_destination() {
    let fixture = FixtureBuilder::mdx([("alpha", "A"), ("beta", "B")]).build();
    let source = fixture.write("persistent-build-digest");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let options = KeyIndexOptions::new();
    // This destination deliberately has no `Read` implementation. A successful
    // build therefore proves the final sidecar is never read back.
    let mut sink = WriteSeekOnly(Cursor::new(Vec::new()));
    let report = dictionary
        .build_key_index(&mut sink, &options, || false)
        .unwrap();
    assert_eq!(
        report.bytes_written(),
        u64::try_from(sink.0.get_ref().len()).unwrap()
    );
    assert_eq!(sink.0.position(), report.bytes_written());
}

#[test]
fn checksum_metadata_ceiling_rejects_before_creating_the_destination() {
    let fixture = FixtureBuilder::mdx([("alpha", "A")]).build();
    let source = fixture.write("persistent-metadata-ceiling");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    // One-row bounds/raw/order each require one checksum. This leaves no
    // checksum slot for the non-empty normalized-text section.
    let metadata_limit = HEADER_BYTES + 3 * size_of::<u32>();
    let options = KeyIndexOptions::new()
        .with_max_metadata_bytes(metadata_limit)
        .with_chunk_bytes(64)
        .with_scratch_directory(directory.path());
    let destination = artifact_path(directory.path(), "metadata-ceiling");

    assert!(matches!(
        dictionary.build_key_index_to_path(&destination, &options, || false),
        Err(Error::LimitExceeded {
            limit: "key_index_metadata_bytes",
            value,
            max,
        }) if value == u64::try_from(metadata_limit + size_of::<u32>()).unwrap()
            && max == u64::try_from(metadata_limit).unwrap()
    ));
    assert!(!destination.exists());
}

#[test]
fn pathological_duplicate_matches_obey_the_locator_byte_ceiling() {
    const DUPLICATES: usize = 4_096;
    let fixture = FixtureBuilder::mdx(std::iter::repeat_n(("same", "value"), DUPLICATES)).build();
    let source = fixture.write("persistent-hostile-duplicates");
    let open_options = MdictOpenOptions::new().with_limits(Limits::new().with_locator_bytes(128));
    let dictionary = MdxFile::open_with_options(source.path(), &open_options).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new()
        .with_chunk_bytes(64)
        .with_scratch_directory(directory.path());
    let (path, proof) = build_index(
        &dictionary,
        directory.path(),
        "hostile-duplicates",
        &options,
    );
    let index = dictionary.open_key_index(path, &proof, &options).unwrap();

    assert!(matches!(
        dictionary.locate_with_key_index(&index, "same"),
        Err(Error::LimitExceeded {
            limit: "key_index_match_bytes",
            value: 16_384,
            max: 128,
        })
    ));

    let before_page = dictionary.memory_usage().unwrap().current_bytes();
    let page = dictionary
        .locate_page_with_key_index(&index, "same", 2_047, 5)
        .unwrap()
        .unwrap();
    assert_eq!(page.basis(), MatchBasis::RawExact);
    assert_eq!(page.total(), DUPLICATES);
    assert_eq!(
        page.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
        [2_047, 2_048, 2_049, 2_050, 2_051]
    );
    assert_eq!(
        dictionary.memory_usage().unwrap().current_bytes() - before_page,
        5 * size_of::<u32>()
    );
    drop(page);
    assert_eq!(
        dictionary.memory_usage().unwrap().current_bytes(),
        before_page
    );

    let beyond = dictionary
        .locate_page_with_key_index(&index, "same", DUPLICATES, 5)
        .unwrap()
        .unwrap();
    assert_eq!(beyond.total(), DUPLICATES);
    assert!(beyond.is_empty());
}

#[test]
fn persistent_scan_rejects_same_layout_source_key_mutation() {
    let fixture = FixtureBuilder::mdx([
        ("alpha", "A"),
        ("bravo", "B"),
        ("delta", "D"),
        ("omega", "O"),
    ])
    .key_blocks([3, 1])
    .build();
    let source = fixture.write("persistent-scan-source-mutation");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new().with_scratch_directory(directory.path());
    let (path, proof) = build_index(
        &dictionary,
        directory.path(),
        "scan-source-mutation",
        &options,
    );
    let index = dictionary.open_key_index(path, &proof, &options).unwrap();

    // Preserve file length, parsed block geometry, and the block's first/last
    // summaries, but replace its middle key after the index was opened.
    // Building visited block one last, so block zero is read back from source.
    let mut mutated = fixture.clone();
    let first_key_block = mutated.layout.key_blocks[0].clone();
    mutated.set_uncompressed_payload_bytes(&first_key_block, 22, b"crane");
    fs::write(source.path(), &mutated.bytes).unwrap();
    let original_modified = UNIX_EPOCH
        + Duration::from_nanos(u64::try_from(proof.source_modified_unix_nanos()).unwrap());
    OpenOptions::new()
        .write(true)
        .open(source.path())
        .unwrap()
        .set_times(FileTimes::new().set_modified(original_modified))
        .unwrap();

    let mut visits = 0usize;
    let scan = dictionary.scan_normalized_keys_with_index(&index, |_, _| {
        visits += 1;
        ControlFlow::Continue(())
    });
    assert!(
        matches!(
            &scan,
            Err(Error::KeyIndexRejected(
                KeyIndexRejection::SourceKeyMismatch { ordinal }
            )) if *ordinal == KeyOrdinal::new(1)
        ),
        "unexpected scan result: {scan:?}"
    );
    assert_eq!(visits, 1);
}

#[test]
fn truncation_and_source_rewrite_do_not_make_the_dictionary_unreadable() {
    let fixture = FixtureBuilder::mdx([("alpha", "A"), ("beta", "B")]).build();
    let source = fixture.write("persistent-stale-source");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let options = KeyIndexOptions::new().with_scratch_directory(directory.path());
    let (path, proof) = build_index(&dictionary, directory.path(), "stale-source", &options);

    let original_len = fs::metadata(&path).unwrap().len();
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(original_len - 1)
        .unwrap();
    assert!(matches!(
        dictionary.open_key_index(&path, &proof, &options),
        Err(Error::KeyIndexRejected(
            KeyIndexRejection::FileLengthMismatch { .. }
        ))
    ));
    assert_eq!(dictionary.lookup("alpha").unwrap().unwrap().text(), "A");

    let rebuilt = artifact_path(directory.path(), "source-rewrite");
    let report = dictionary
        .build_key_index_to_path(&rebuilt, &options, || false)
        .unwrap();
    let index = dictionary
        .open_key_index(&rebuilt, &report.source_identity(), &options)
        .unwrap();
    let mut source_bytes = fs::read(source.path()).unwrap();
    let last = source_bytes.last_mut().unwrap();
    *last ^= 0x01;
    fs::write(source.path(), source_bytes).unwrap();
    OpenOptions::new()
        .write(true)
        .open(source.path())
        .unwrap()
        .set_times(FileTimes::new().set_modified(SystemTime::now() + Duration::from_secs(2)))
        .unwrap();
    assert!(matches!(
        dictionary.locate_with_key_index(&index, "alpha"),
        Err(Error::KeyIndexRejected(
            KeyIndexRejection::SourceModifiedMismatch { .. }
        ))
    ));
}

#[test]
fn build_detects_source_mutation_and_honors_cancellation() {
    let fixture = FixtureBuilder::mdx([("alpha", "A"), ("beta", "B")]).build();
    let source = fixture.write("persistent-build-mutation");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let options = KeyIndexOptions::new();
    let mut sink = Cursor::new(Vec::new());
    assert!(matches!(
        dictionary.build_key_index(&mut sink, &options, || true),
        Err(Error::Cancelled { .. })
    ));
    assert!(sink.get_ref().is_empty());

    let mut checkpoints = 0usize;
    let result = dictionary.build_key_index(&mut sink, &options, || {
        checkpoints += 1;
        if checkpoints == 2 {
            OpenOptions::new()
                .append(true)
                .open(source.path())
                .unwrap()
                .write_all(&[0])
                .unwrap();
        }
        false
    });
    assert!(matches!(result, Err(Error::SourceChanged { .. })));
}
