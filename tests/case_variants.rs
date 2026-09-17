mod support;

use std::fs::{self, FileTimes};
use std::mem::size_of;

use mdictlib::{
    ChecksumPolicy, Error, KeyIndexOptions, KeyIndexRejection, KeyOrdinal, Limits, MatchBasis,
    MatchMode, MdxFile, OpenOptions,
};
use support::v1::V1FixtureBuilder;
use support::{FixtureBuilder, TempDictionary};

const INCLUDE: MatchMode = MatchMode::IncludeCaseVariants;

#[test]
fn locators_never_read_record_bodies() {
    let mut fixture = FixtureBuilder::mdx([("Make", "upper"), ("make", "lower")]).build();
    fixture.bytes[fixture.layout.record_blocks[0].start] = 0xff;
    let source = fixture.write("case-lazy-records");
    assert_queries(&source, &[("make", MatchBasis::CaseVariants, &[1, 0])]);
    let dictionary = MdxFile::open(source.path()).unwrap();
    assert!(dictionary.entry_at(KeyOrdinal::new(0)).is_err());
}

#[test]
fn empty_dictionaries_return_no_case_matches() {
    let source = FixtureBuilder::mdx(Vec::<(&str, &str)>::new())
        .build()
        .write("case-empty");
    assert_queries(&source, &[]);
    assert_eq!(MatchMode::default(), MatchMode::PreferExact);
}

#[test]
fn indexed_case_selection_rejects_changed_keys_even_outside_the_requested_page() {
    let fixture = FixtureBuilder::mdx([("US", "country"), ("us", "pronoun"), ("Us", "other")])
        .key_blocks([1, 1, 1])
        .build();
    let source = fixture.write("case-source-mismatch");
    let dictionary = MdxFile::open(source.path()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keys.aaidx");
    let options = KeyIndexOptions::new();
    let build = dictionary
        .build_key_index_to_path(&path, &options, || false)
        .unwrap();
    drop(dictionary);
    let modified = fs::metadata(source.path()).unwrap().modified().unwrap();
    let changed = FixtureBuilder::mdx([("US", "country"), ("us", "pronoun"), ("uS", "other")])
        .key_blocks([1, 1, 1])
        .build();
    assert_eq!(changed.bytes.len(), fixture.bytes.len());
    // The normalized key and geometry stay equal, so only source-key proof can
    // reject this old index before returning even a one-row exact result.
    fs::write(source.path(), &changed.bytes).unwrap();
    fs::File::options()
        .write(true)
        .open(source.path())
        .unwrap()
        .set_times(FileTimes::new().set_modified(modified))
        .unwrap();
    let dictionary = MdxFile::open(source.path()).unwrap();
    let index = dictionary
        .open_key_index(path, &build.source_identity(), &options)
        .unwrap();
    for (offset, limit) in [(0, 1), (0, 0), (usize::MAX, 1)] {
        assert!(
            matches!(dictionary.locate_page_with_key_index_and_mode(&index, "us", offset, limit, INCLUDE),
            Err(Error::KeyIndexRejected(KeyIndexRejection::SourceKeyMismatch { ordinal })) if ordinal.get() == 2)
        );
    }
}

fn assert_queries(source: &TempDictionary, queries: &[(&str, MatchBasis, &[u64])]) {
    for checksum_policy in [ChecksumPolicy::Skip, ChecksumPolicy::Verify] {
        let dictionary = MdxFile::open(source.path()).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("keys.aaidx");
        let options = KeyIndexOptions::new()
            .with_checksum_policy(checksum_policy)
            .with_chunk_bytes(64);
        let build = dictionary
            .build_key_index_to_path(&path, &options, || false)
            .unwrap();
        let index = dictionary
            .open_key_index(&path, &build.source_identity(), &options)
            .unwrap();
        for &(query, basis, expected) in queries {
            let indexed = dictionary
                .locate_with_key_index_and_mode(&index, query, INCLUDE)
                .unwrap()
                .unwrap();
            assert_eq!(dictionary.memory_usage().unwrap().locator_bytes(), 0);
            assert_eq!(indexed.basis(), basis, "{query}");
            assert_eq!(
                indexed.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
                expected,
                "{query}"
            );

            for offset in (0..=expected.len() + 1).chain([usize::MAX]) {
                for limit in [0, 1, 2, expected.len() + 1, usize::MAX] {
                    let page = dictionary
                        .locate_page_with_key_index_and_mode(&index, query, offset, limit, INCLUDE)
                        .unwrap()
                        .unwrap();
                    let expected_page = expected
                        .iter()
                        .copied()
                        .skip(offset)
                        .take(limit)
                        .collect::<Vec<_>>();
                    assert_eq!(page.total(), expected.len(), "{query}");
                    assert_eq!(page.basis(), basis, "{query}");
                    assert_eq!(
                        page.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
                        expected_page,
                        "{query}, {offset}, {limit}"
                    );
                }
            }
        }
        assert!(
            dictionary
                .locate_with_key_index_and_mode(&index, "missing", INCLUDE)
                .unwrap()
                .is_none()
        );
        assert!(
            dictionary
                .locate_page_with_key_index_and_mode(&index, "missing", 0, 5, INCLUDE)
                .unwrap()
                .is_none()
        );
        assert_eq!(dictionary.memory_usage().unwrap().locator_bytes(), 0);

        for &(query, basis, expected) in queries {
            let direct = dictionary
                .locate_with_mode(query, INCLUDE)
                .unwrap()
                .unwrap();
            assert_eq!(direct.basis(), basis);
            assert_eq!(
                direct.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(direct.first().get(), expected[0]);
            let cloned = direct.clone();
            drop(direct);
            assert_eq!(
                cloned.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
                expected
            );
            for offset in (0..=expected.len() + 1).chain([usize::MAX]) {
                for limit in [0, 1, 2, expected.len() + 1, usize::MAX] {
                    let page = dictionary
                        .locate_page_with_mode(query, offset, limit, INCLUDE)
                        .unwrap()
                        .unwrap();
                    assert_eq!(page.total(), expected.len());
                    assert_eq!(page.basis(), basis);
                    assert_eq!(
                        page.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
                        expected
                            .iter()
                            .copied()
                            .skip(offset)
                            .take(limit)
                            .collect::<Vec<_>>()
                    );
                }
            }
            let original = dictionary.locate(query).unwrap().unwrap();
            let preferred = dictionary
                .locate_with_mode(query, MatchMode::PreferExact)
                .unwrap()
                .unwrap();
            assert_eq!(original.basis(), preferred.basis());
            assert_eq!(
                original.iter().collect::<Vec<_>>(),
                preferred.iter().collect::<Vec<_>>()
            );
            let indexed_preferred = dictionary
                .locate_with_key_index_and_mode(&index, query, MatchMode::PreferExact)
                .unwrap()
                .unwrap();
            assert_eq!(
                original.iter().collect::<Vec<_>>(),
                indexed_preferred.iter().collect::<Vec<_>>()
            );
        }
        assert!(
            dictionary
                .locate_with_mode("missing", INCLUDE)
                .unwrap()
                .is_none()
        );
        assert!(
            dictionary
                .locate_page_with_mode("missing", 0, 5, INCLUDE)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn exact_rows_precede_case_variants_across_blocks_in_both_wire_versions() {
    let entries = [
        ("MAKE", "upper"),
        ("make", "exact one"),
        ("Make", "title"),
        ("make", "exact two"),
    ];
    let v1 = V1FixtureBuilder::mdx(entries)
        .key_blocks([1, 2, 1])
        .build()
        .write("case-v1");
    let v2 = FixtureBuilder::mdx(entries)
        .key_blocks([1, 2, 1])
        .build()
        .write("case-v2");
    for source in [&v1, &v2] {
        assert_queries(
            source,
            &[
                ("make", MatchBasis::CaseVariants, &[1, 3, 0, 2]),
                ("MAKE", MatchBasis::CaseVariants, &[0, 1, 2, 3]),
                ("Make", MatchBasis::CaseVariants, &[2, 0, 1, 3]),
                ("mAkE", MatchBasis::CaseVariants, &[0, 1, 2, 3]),
            ],
        );
        let dictionary = MdxFile::open(source.path()).unwrap();
        assert_eq!(
            dictionary.lookup("make").unwrap().unwrap().ordinal().get(),
            1
        );
        assert_eq!(
            dictionary
                .locate("make")
                .unwrap()
                .unwrap()
                .iter()
                .map(KeyOrdinal::get)
                .collect::<Vec<_>>(),
            [1, 3]
        );
    }
}

#[test]
fn case_expansion_preserves_punctuation_and_existing_normalized_fallback() {
    let source = FixtureBuilder::mdx([
        ("TAKEOUT", "closed"),
        ("take-out", "exact"),
        ("Take-out", "case"),
        ("take out", "space"),
        ("take-out", "duplicate"),
        ("TAKE-OUT!", "punctuation"),
    ])
    .strip_key_attribute("StripKey", "Yes")
    .key_blocks([2, 2, 2])
    .build()
    .write("case-punctuation");
    assert_queries(
        &source,
        &[
            ("take-out", MatchBasis::CaseVariants, &[1, 4, 2]),
            ("tAkE-oUt", MatchBasis::CaseVariants, &[1, 2, 4]),
            ("take out", MatchBasis::RawExact, &[3]),
            ("takeout", MatchBasis::CaseVariants, &[0]),
            (
                "take.out",
                MatchBasis::HeaderNormalized,
                &[0, 1, 2, 3, 4, 5],
            ),
        ],
    );
}

#[test]
fn case_sensitive_headers_keep_the_existing_match_policy() {
    let source = FixtureBuilder::mdx([("US", "country"), ("us", "pronoun"), ("U-S", "hyphen")])
        .key_case_attribute("KeyCaseSensitive", "Yes")
        .strip_key_attribute("StripKey", "Yes")
        .build()
        .write("case-sensitive");
    assert_queries(
        &source,
        &[
            ("us", MatchBasis::RawExact, &[1]),
            ("US", MatchBasis::RawExact, &[0]),
            ("U.S", MatchBasis::HeaderNormalized, &[0, 2]),
        ],
    );
    assert!(
        MdxFile::open(source.path())
            .unwrap()
            .locate_with_mode("Us", INCLUDE)
            .unwrap()
            .is_none()
    );
}

#[test]
fn unicode_uses_scalar_lowercase_without_adding_full_casefold_equivalences() {
    let source = FixtureBuilder::mdx([
        ("ÉCOLE", "upper"),
        ("école", "lower"),
        ("İ", "expanded upper"),
        ("i\u{307}", "expanded lower"),
        ("Straße", "sharp s"),
        ("STRASSE", "ss"),
        ("Σ", "sigma"),
        ("σ", "lower sigma"),
        ("ς", "final sigma"),
    ])
    .build()
    .write("case-unicode");
    assert_queries(
        &source,
        &[
            ("école", MatchBasis::CaseVariants, &[1, 0]),
            ("İ", MatchBasis::CaseVariants, &[2, 3]),
            ("i\u{307}", MatchBasis::CaseVariants, &[3, 2]),
            ("Straße", MatchBasis::RawExact, &[4]),
            ("strasse", MatchBasis::CaseVariants, &[5]),
            ("σ", MatchBasis::CaseVariants, &[7, 6]),
            ("ς", MatchBasis::RawExact, &[8]),
        ],
    );
}

#[test]
fn persistent_case_pages_obey_memory_limits_for_large_duplicate_groups() {
    const ROWS: usize = 4096;
    let source =
        FixtureBuilder::mdx((0..ROWS).map(|n| (if n % 2 == 0 { "US" } else { "us" }, "entry")))
            .build()
            .write("case-bounded");
    let dictionary = MdxFile::open_with_options(
        source.path(),
        &OpenOptions::new().with_limits(Limits::new().with_locator_bytes(128)),
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keys.aaidx");
    let options = KeyIndexOptions::new();
    let build = dictionary
        .build_key_index_to_path(&path, &options, || false)
        .unwrap();
    let index = dictionary
        .open_key_index(path, &build.source_identity(), &options)
        .unwrap();
    assert!(matches!(
        dictionary.locate_with_key_index_and_mode(&index, "us", INCLUDE),
        Err(Error::LimitExceeded {
            limit: "key_match_page_bytes",
            value: 16384,
            max: 128
        })
    ));
    // Warm only the bounded parser/index caches, then account the returned page.
    drop(
        dictionary
            .locate_page_with_key_index_and_mode(&index, "us", 2047, 5, INCLUDE)
            .unwrap(),
    );
    let before = dictionary.memory_usage().unwrap().current_bytes();
    let page = dictionary
        .locate_page_with_key_index_and_mode(&index, "us", 2047, 5, INCLUDE)
        .unwrap()
        .unwrap();
    assert_eq!(page.total(), ROWS);
    assert_eq!(
        page.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
        [4095, 0, 2, 4, 6]
    );
    assert_eq!(
        dictionary.memory_usage().unwrap().current_bytes() - before,
        5 * size_of::<u32>()
    );
    drop(page);
    assert_eq!(dictionary.memory_usage().unwrap().current_bytes(), before);
    assert_eq!(dictionary.memory_usage().unwrap().locator_bytes(), 0);
}
