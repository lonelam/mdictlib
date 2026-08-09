use std::io::Write;
use std::iter::FusedIterator;

use mdictlib::{
    Header, KeyEntry, KeyMatches, KeyOrdinal, Limits, MatchBasis, MddFile, MddResource,
    MddResourceSpan, MdxEntry, MdxFile, MemoryUsage, OpenOptions, Passcode, Result,
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
    let _: Option<MdxEntry> = file.lookup("query")?;
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
fn span_writer_contract(span: &MddResourceSpan, writer: &mut dyn Write) -> Result<u64> {
    span.copy_to(writer)
}

#[test]
fn value_api_contract() {
    let ordinal = KeyOrdinal::new(u64::MAX);
    assert_eq!(ordinal.get(), u64::MAX);

    let passcode = Passcode::new("0123456789abcdef0123456789abcdef", "user").unwrap();
    let options = OpenOptions::new()
        .with_passcode(passcode)
        .with_limits(Limits::new());
    let limits = options.limits();
    assert!(limits.working_memory_bytes() >= limits.locator_bytes());
    assert!(!format!("{options:?}").contains("0123456789abcdef0123456789abcdef"));
}
