use std::io::{Seek, Write};
use std::iter::FusedIterator;
use std::ops::ControlFlow;
use std::path::Path;

use mdictlib::{
    ChecksumPolicy, Header, KEY_INDEX_FORMAT_REVISION, KEY_INDEX_NORMALIZATION_REVISION,
    KEY_INDEX_PARSER_REVISION, KEY_INDEX_REVISION, KeyEntry, KeyIndex, KeyIndexBuild,
    KeyIndexOptions, KeyIndexRejection, KeyIndexSourceIdentity, KeyMatchPage, KeyMatches,
    KeyOrdinal, Limits, MatchBasis, MddFile, MddResource, MddResourceSpan, MdxEntry, MdxFile,
    MemoryUsage, OpenOptions, Passcode, Result,
};

fn assert_fused_result_iter<T>(_iterator: impl FusedIterator<Item = Result<T>>) {}

#[allow(dead_code)]
fn mdx_contract(file: &MdxFile, ordinal: KeyOrdinal) -> Result<()> {
    let _: &Header = file.header();
    let _: u64 = file.len();
    let _: bool = file.is_empty();
    let _: MemoryUsage = file.memory_usage()?;
    assert_fused_result_iter::<KeyEntry>(file.keys());
    assert_fused_result_iter::<MdxEntry>(file.entries());

    let _: Option<KeyEntry> = file.key_at(ordinal)?;
    let _: Vec<Option<KeyEntry>> = file.keys_at(&[ordinal, ordinal])?;
    let _: Option<MdxEntry> = file.entry_at(ordinal)?;
    let _: Option<KeyMatches> = file.locate("query")?;
    let _: Option<KeyMatchPage> = file.locate_page("query", 0, 10)?;
    let _: Option<MdxEntry> = file.lookup("query")?;
    Ok(())
}

#[allow(dead_code)]
fn persistent_index_contract<W>(file: &MdxFile, index_path: &Path, writer: &mut W) -> Result<()>
where
    W: Write + Seek,
{
    let options = KeyIndexOptions::new()
        .with_max_index_bytes(u64::MAX)
        .with_max_metadata_bytes(1024 * 1024)
        .with_build_memory_bytes(1024 * 1024)
        .with_chunk_bytes(64 * 1024)
        .with_checksum_policy(ChecksumPolicy::Verify)
        .with_scratch_directory(index_path.parent().unwrap_or_else(|| Path::new(".")));
    let _: u64 = options.max_index_bytes();
    let _: usize = options.max_metadata_bytes();
    let _: usize = options.build_memory_bytes();
    let _: usize = options.chunk_bytes();
    let _: Option<&Path> = options.scratch_directory();
    assert_eq!(options.checksum_policy(), ChecksumPolicy::Verify);

    let identity: KeyIndexSourceIdentity = file.key_index_source_identity()?;
    let _: u64 = identity.source_bytes();
    let _: i128 = identity.source_modified_unix_nanos();
    let _: u64 = identity.key_count();
    let _: KeyIndexSourceIdentity = KeyIndexSourceIdentity::new(
        identity.source_bytes(),
        identity.source_modified_unix_nanos(),
        identity.key_count(),
    );

    let build: KeyIndexBuild = file.build_key_index(writer, &options, || false)?;
    let _: KeyIndexSourceIdentity = build.source_identity();
    let _: u64 = build.bytes_written();
    let _: KeyIndexBuild = file.build_key_index_to_path(index_path, &options, || false)?;
    let index: KeyIndex = file.open_key_index(index_path, &identity, &options)?;
    let _: KeyIndexSourceIdentity = index.source_identity();
    let _: u64 = index.len();
    let _: bool = index.is_empty();
    let _: Option<KeyMatches> = file.locate_with_key_index(&index, "query")?;
    let _: Option<KeyMatchPage> = file.locate_page_with_key_index(&index, "query", 0, 10)?;
    let _: Vec<KeyEntry> = file.prefix_keys_with_index(&index, "prefix", 10)?;
    file.scan_normalized_keys_with_index(&index, |_, _| ControlFlow::Continue(()))?;
    Ok(())
}

#[allow(dead_code)]
fn mdd_contract(file: &MddFile, ordinal: KeyOrdinal) -> Result<()> {
    let _: &Header = file.header();
    let _: u64 = file.len();
    let _: bool = file.is_empty();
    let _: MemoryUsage = file.memory_usage()?;
    assert_fused_result_iter::<KeyEntry>(file.keys());
    assert_fused_result_iter::<MddResource>(file.resources());

    let _: Option<KeyEntry> = file.key_at(ordinal)?;
    let _: Vec<Option<KeyEntry>> = file.keys_at(&[ordinal, ordinal])?;
    let _: Option<MddResource> = file.resource_at(ordinal)?;
    let _: Option<MddResourceSpan> = file.span_at(ordinal)?;
    let _: Option<KeyMatches> = file.locate("query")?;
    let _: Option<KeyMatchPage> = file.locate_page("query", 0, 10)?;
    let _: Option<MddResource> = file.lookup("query")?;
    let _: Option<MddResourceSpan> = file.lookup_span("query")?;
    Ok(())
}

#[allow(dead_code)]
fn match_contract(matches: &KeyMatches) {
    let _: MatchBasis = matches.basis();
    let _: usize = matches.len();
    let _: bool = matches.is_empty();
    let _: KeyOrdinal = matches.first();
    let _: Option<KeyOrdinal> = matches.get(0);
    let _: Vec<KeyOrdinal> = matches.iter().collect();
}

#[allow(dead_code)]
fn match_page_contract(page: &KeyMatchPage) {
    let _: MatchBasis = page.basis();
    let _: usize = page.total();
    let _: usize = page.len();
    let _: bool = page.is_empty();
    let _: Option<KeyOrdinal> = page.get(0);
    let _: Vec<KeyOrdinal> = page.iter().collect();
}

#[allow(dead_code)]
fn span_writer_contract(span: &MddResourceSpan, writer: &mut dyn Write) -> Result<u64> {
    span.copy_to(writer)
}

#[test]
fn value_api_contract() {
    let _: u32 = KEY_INDEX_FORMAT_REVISION;
    let _: u32 = KEY_INDEX_PARSER_REVISION;
    let _: u32 = KEY_INDEX_NORMALIZATION_REVISION;
    let _: &str = KEY_INDEX_REVISION;
    let _: Option<KeyIndexRejection> = None;
    let ordinal = KeyOrdinal::new(u64::MAX);
    assert_eq!(ordinal.get(), u64::MAX);

    let passcode = Passcode::new("0123456789abcdef0123456789abcdef", "user").unwrap();
    let options = OpenOptions::new()
        .with_passcode(passcode)
        .with_limits(Limits::new());
    assert_eq!(options.checksum_policy(), ChecksumPolicy::Skip);
    assert_eq!(
        options
            .clone()
            .with_checksum_policy(ChecksumPolicy::Verify)
            .checksum_policy(),
        ChecksumPolicy::Verify
    );
    let limits = options.limits();
    assert!(limits.working_memory_bytes() >= limits.locator_bytes());
    let large_limits: Limits = Limits::large_dictionary();
    assert!(large_limits.working_memory_bytes() >= limits.working_memory_bytes());
    assert!(!format!("{options:?}").contains("0123456789abcdef0123456789abcdef"));
}
