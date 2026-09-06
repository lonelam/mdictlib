mod support;

use std::ops::ControlFlow;

use mdictlib::{KeyEntry, KeyOrdinal, MatchBasis, MddFile, MdxFile};

use support::FixtureBuilder;

#[test]
fn raw_exact_lookup_wins_globally_before_an_earlier_normalized_match() {
    let fixture = FixtureBuilder::mdx([
        ("TARGET", "normalized fallback"),
        ("Target", "another fallback"),
        ("target", "raw exact"),
    ])
    .key_blocks(vec![1, 1, 1])
    .build();
    let dictionary_file = fixture.write("raw-before-normalized");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let matches = dictionary.locate("target").unwrap().unwrap();
    assert_eq!(matches.basis(), MatchBasis::RawExact);
    assert_eq!(matches.len(), 1);
    assert!(!matches.is_empty());
    assert_eq!(matches.first(), KeyOrdinal::new(2));
    assert_eq!(matches.get(0), Some(KeyOrdinal::new(2)));
    assert_eq!(matches.get(1), None);
    assert_eq!(matches.iter().collect::<Vec<_>>(), [KeyOrdinal::new(2)]);

    let entry = dictionary.lookup("target").unwrap().unwrap();
    assert_eq!(entry.ordinal(), KeyOrdinal::new(2));
    assert_eq!(entry.text(), "raw exact");
}

#[test]
fn locate_preserves_all_raw_duplicates_in_physical_order_across_many_blocks() {
    let fixture = FixtureBuilder::mdx([
        ("duplicate", "zero"),
        ("duplicate", "one"),
        ("duplicate", "two"),
        ("duplicate", "three"),
        ("duplicate", "four"),
    ])
    .key_blocks(vec![1, 1, 1, 1, 1])
    .build();
    let dictionary_file = fixture.write("raw-duplicates-many-blocks");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let matches = dictionary.locate("duplicate").unwrap().unwrap();
    assert_eq!(matches.basis(), MatchBasis::RawExact);
    assert_eq!(
        matches.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
    );
    assert_eq!(
        dictionary.lookup("duplicate").unwrap().unwrap().text(),
        "zero"
    );
}

#[test]
fn locate_page_preserves_global_raw_precedence_total_and_order() {
    let fixture = FixtureBuilder::mdx([
        ("PAGE!", "normalized zero"),
        ("page", "raw one"),
        ("Page?", "normalized two"),
        ("page", "raw three"),
        ("page", "raw four"),
    ])
    .strip_key_attribute("StripKey", "Yes")
    .key_blocks([1, 2, 2])
    .build();
    let dictionary_file = fixture.write("paged-raw-precedence");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let first = dictionary.locate_page("page", 0, 2).unwrap().unwrap();
    assert_eq!(first.basis(), MatchBasis::RawExact);
    assert_eq!(first.total(), 3);
    assert_eq!(
        first.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
        [1, 3]
    );

    let second = dictionary.locate_page("page", 2, 2).unwrap().unwrap();
    assert_eq!(second.basis(), MatchBasis::RawExact);
    assert_eq!(second.total(), 3);
    assert_eq!(second.iter().map(KeyOrdinal::get).collect::<Vec<_>>(), [4]);

    let normalized = dictionary.locate_page("PAGE", 1, 2).unwrap().unwrap();
    assert_eq!(normalized.basis(), MatchBasis::HeaderNormalized);
    assert_eq!(normalized.total(), 5);
    assert_eq!(
        normalized.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
        [1, 2]
    );
}

#[test]
fn raw_exact_duplicate_range_excludes_normalized_only_collisions() {
    let fixture = FixtureBuilder::mdx([
        ("TARGET", "normalized zero"),
        ("target", "raw one"),
        ("Target", "normalized two"),
        ("tArGeT", "normalized three"),
        ("target", "raw four"),
    ])
    .key_blocks(vec![1, 1, 1, 1, 1])
    .build();
    let dictionary_file = fixture.write("raw-duplicates-exclude-normalized");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let matches = dictionary.locate("target").unwrap().unwrap();
    assert_eq!(matches.basis(), MatchBasis::RawExact);
    assert_eq!(matches.iter().len(), 2);
    assert_eq!(
        matches.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
        [1, 4]
    );
    assert_eq!(
        dictionary.lookup("target").unwrap().unwrap().text(),
        "raw one"
    );
}

#[test]
fn case_sensitive_header_disables_case_folded_fallback() {
    let fixture = FixtureBuilder::mdx([("CaseSensitive", "value")])
        .key_case_attribute("KeyCaseSensitive", "Yes")
        .build();
    let dictionary_file = fixture.write("case-sensitive-lookup");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    assert!(dictionary.header().key_case_sensitive());
    assert!(dictionary.locate("casesensitive").unwrap().is_none());
    let matches = dictionary.locate("CaseSensitive").unwrap().unwrap();
    assert_eq!(matches.basis(), MatchBasis::RawExact);
    assert_eq!(matches.first(), KeyOrdinal::new(0));
}

#[test]
fn strip_key_author_profile_removes_non_alphanumeric_ascii_only() {
    let fixture = FixtureBuilder::mdx([
        (" Foo-Bar_Baz! ", "first"),
        ("FOO\tBAR.BAZ", "second"),
        ("foo/bar+baz", "third"),
        ("fOo:bAr@bAz", "fourth"),
    ])
    .key_blocks(vec![1, 1, 1, 1])
    .strip_key_attribute("StripKey", "Yes")
    .build();
    let dictionary_file = fixture.write("strip-key-candidate-profile");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let matches = dictionary.locate("FOO\rBAR#BAZ").unwrap().unwrap();
    assert_eq!(matches.basis(), MatchBasis::HeaderNormalized);
    assert_eq!(
        matches.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
}

#[test]
fn decoded_keys_that_disagree_with_raw_block_summaries_are_rejected() {
    let fixture = FixtureBuilder::mdx([("actual-key", "value")])
        .key_summaries([("unrelated-first", "unrelated-last")])
        .build();
    let dictionary_file = fixture.write("mismatched-raw-block-summary");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    assert!(dictionary.locate("actual-key").is_err());
}

#[test]
fn strip_key_preserves_non_ascii_whitespace_and_punctuation() {
    let fixture = FixtureBuilder::mdx([
        ("foo\u{2003}bar", "unicode space"),
        ("foo—bar", "unicode punctuation"),
    ])
    .key_blocks(vec![1, 1])
    .strip_key_attribute("StripKey", "Yes")
    .build();
    let dictionary_file = fixture.write("strip-key-non-ascii-negative-control");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    assert!(dictionary.locate("foo bar").unwrap().is_none());
    assert!(dictionary.locate("foo-bar").unwrap().is_none());
    assert_eq!(
        dictionary
            .locate("FOO\u{2003}BAR")
            .unwrap()
            .unwrap()
            .basis(),
        MatchBasis::HeaderNormalized
    );
}

#[test]
fn locator_ignores_nonmonotonic_physical_block_summaries_for_raw_exact_search() {
    let fixture = FixtureBuilder::mdx([
        ("needle", "found"),
        ("zulu", "z"),
        ("alpha", "a"),
        ("bravo", "b"),
        ("charlie", "c"),
    ])
    .key_blocks(vec![1, 1, 1, 1, 1])
    .build();
    let dictionary_file = fixture.write("nonmonotonic-summaries");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let matches = dictionary.locate("needle").unwrap().unwrap();
    assert_eq!(matches.basis(), MatchBasis::RawExact);
    assert_eq!(matches.iter().collect::<Vec<_>>(), [KeyOrdinal::new(0)]);
    assert_eq!(
        dictionary.lookup("needle").unwrap().unwrap().text(),
        "found"
    );
}

#[test]
fn normalized_collisions_across_more_than_three_blocks_remain_complete() {
    let fixture = FixtureBuilder::mdx([
        ("NEEDLE!", "zero"),
        ("Needle?", "one"),
        ("nEedle_", "two"),
        ("needlE-", "three"),
        ("NEedLE.", "four"),
    ])
    .key_blocks(vec![1, 1, 1, 1, 1])
    .strip_key_attribute("STRIPKEY", "Yes")
    .build();
    let dictionary_file = fixture.write("normalized-collisions-many-blocks");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let matches = dictionary.locate("needle").unwrap().unwrap();
    assert_eq!(matches.basis(), MatchBasis::HeaderNormalized);
    assert_eq!(
        matches.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
    );
}

#[test]
fn strip_key_attribute_aliases_are_ascii_case_insensitive() {
    for alias in ["Stripkey", "STRIPKEY"] {
        let fixture = FixtureBuilder::mdx([("alpha-beta", "value")])
            .strip_key_attribute(alias, "Yes")
            .build();
        let dictionary_file = fixture.write("strip-key-alias");
        let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

        assert!(dictionary.header().strip_key());
        assert_eq!(dictionary.header().attribute(alias), Some("Yes"));
        let matches = dictionary.locate("ALPHA BETA").unwrap().unwrap();
        assert_eq!(matches.basis(), MatchBasis::HeaderNormalized);
        assert_eq!(matches.first(), KeyOrdinal::new(0));
    }
}

#[test]
fn conflicting_strip_key_alias_values_are_rejected() {
    for (primary_name, alias_name) in [("StripKey", "STRIPKEY"), ("Stripkey", "stripkey")] {
        let fixture = FixtureBuilder::mdx([("alpha", "value")])
            .strip_key_attribute(primary_name, "Yes")
            .header_attribute(alias_name, "No")
            .build();
        let dictionary_file = fixture.write("strip-key-conflict");

        assert!(MdxFile::open(dictionary_file.path()).is_err());
    }
}

#[test]
fn equivalent_strip_key_alias_values_are_accepted() {
    let fixture = FixtureBuilder::mdx([("alpha-beta", "value")])
        .strip_key_attribute("StripKey", "Yes")
        .header_attribute("STRIPKEY", "YES")
        .build();
    let dictionary_file = fixture.write("equivalent-strip-key-aliases");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    assert!(dictionary.header().strip_key());
    assert_eq!(
        dictionary.locate("alpha beta").unwrap().unwrap().first(),
        KeyOrdinal::new(0)
    );
}

#[test]
fn mdd_locator_preserves_duplicate_resource_identity_and_routes_lookup_by_ordinal() {
    let fixture = FixtureBuilder::mdd([
        ("\\asset.bin", vec![0x01]),
        ("\\asset.bin", vec![0x02]),
        ("\\asset.bin", vec![0x03]),
    ])
    .key_blocks(vec![1, 1, 1])
    .build();
    let dictionary_file = fixture.write("mdd-duplicate-locator");
    let dictionary = MddFile::open(dictionary_file.path()).unwrap();

    let matches = dictionary.locate("\\asset.bin").unwrap().unwrap();
    assert_eq!(matches.basis(), MatchBasis::RawExact);
    assert_eq!(
        matches.iter().map(KeyOrdinal::get).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    let resource = dictionary.lookup("\\asset.bin").unwrap().unwrap();
    assert_eq!(resource.ordinal(), KeyOrdinal::new(0));
    assert_eq!(resource.bytes(), [0x01]);

    let span = dictionary.lookup_span("\\asset.bin").unwrap().unwrap();
    assert_eq!(span.ordinal(), KeyOrdinal::new(0));
    assert_eq!(span.read().unwrap().bytes(), [0x01]);
}

#[test]
fn mdd_lookup_accepts_relative_and_forward_slash_resource_paths() {
    let fixture = FixtureBuilder::mdd([(r"\assets\theme.css", b"theme".to_vec())]).build();
    let dictionary_file = fixture.write("mdd-compatible-resource-paths");
    let dictionary = MddFile::open(dictionary_file.path()).unwrap();

    let exact = dictionary.locate(r"\assets\theme.css").unwrap().unwrap();
    assert_eq!(exact.basis(), MatchBasis::RawExact);
    for query in ["assets/theme.css", "/assets/theme.css", r"assets\theme.css"] {
        let matches = dictionary.locate(query).unwrap().unwrap();
        assert_eq!(matches.basis(), MatchBasis::HeaderNormalized, "{query:?}");
        assert_eq!(dictionary.lookup(query).unwrap().unwrap().bytes(), b"theme");
    }
}

#[test]
fn prefix_keys_walk_normalized_order_and_preserve_duplicates() {
    let fixture = FixtureBuilder::mdx([
        ("Apple", "one"),
        ("apple", "two"),
        ("app-let", "three"),
        ("Application", "four"),
        ("banana", "five"),
    ])
    .strip_key_attribute("StripKey", "Yes")
    .key_blocks(vec![2, 2, 1])
    .build();
    let dictionary_file = fixture.write("prefix-keys-normalized");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let keys: Vec<String> = dictionary
        .prefix_keys("APP", 10)
        .unwrap()
        .into_iter()
        .map(KeyEntry::into_key)
        .collect();
    assert_eq!(keys, ["Apple", "apple", "app-let", "Application"]);

    // The limit bounds physical rows, duplicates included.
    assert_eq!(dictionary.prefix_keys("app", 2).unwrap().len(), 2);
    assert!(dictionary.prefix_keys("app", 0).unwrap().is_empty());
    assert!(dictionary.prefix_keys("zebra", 10).unwrap().is_empty());
    // A prefix that only matches after normalization still matches.
    assert_eq!(dictionary.prefix_keys("applet", 10).unwrap().len(), 1);
}

#[test]
fn scan_normalized_keys_visits_physical_order_and_stops_on_break() {
    let fixture = FixtureBuilder::mdx([("Ice-Cream", "one"), ("beta", "two"), ("GAMMA", "three")])
        .strip_key_attribute("StripKey", "Yes")
        .build();
    let dictionary_file = fixture.write("scan-normalized-keys");
    let dictionary = MdxFile::open(dictionary_file.path()).unwrap();

    let mut seen = Vec::new();
    dictionary
        .scan_normalized_keys(|ordinal, normalized| {
            seen.push((ordinal.get(), normalized.to_owned()));
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!(
        seen,
        [
            (0, "icecream".to_owned()),
            (1, "beta".to_owned()),
            (2, "gamma".to_owned()),
        ]
    );

    let mut visited = 0;
    dictionary
        .scan_normalized_keys(|_, _| {
            visited += 1;
            ControlFlow::Break(())
        })
        .unwrap();
    assert_eq!(visited, 1);
}
