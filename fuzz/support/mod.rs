#![allow(dead_code)]

use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mdictlib::{KeyOrdinal, Limits, MddFile, MdxFile, OpenOptions};

pub const MAX_WHOLE_FILE_INPUT: usize = 1024 * 1024;

const MAX_ROWS: usize = 16;
const MAX_STREAMED_SPAN: u64 = 64 * 1024;
const MAX_MUTATIONS: usize = 64;

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Mdx,
    Mdd,
}

impl Kind {
    const fn extension(self) -> &'static str {
        match self {
            Self::Mdx => "mdx",
            Self::Mdd => "mdd",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub keyword_header: Range<usize>,
    pub key_index: Range<usize>,
    pub key_blocks: Vec<Range<usize>>,
    pub record_header: Range<usize>,
    pub record_index: Range<usize>,
    pub record_blocks: Vec<Range<usize>>,
    pub record_offsets: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct Fixture {
    pub kind: Kind,
    pub bytes: Vec<u8>,
    pub layout: Layout,
}

pub fn fixture(kind: Kind) -> Fixture {
    let keys = match kind {
        Kind::Mdx => ["alpha", "duplicate", "duplicate", "empty", "omega", "zulu"],
        Kind::Mdd => [
            "\\alpha.bin",
            "\\duplicate.bin",
            "\\duplicate.bin",
            "\\empty.bin",
            "\\omega.bin",
            "\\zulu.bin",
        ],
    };
    let records: [&[u8]; 6] = [b"A", b"BB", b"CCC", b"", b"DD", b"E"];
    let record_data = records.concat();
    let record_starts = records
        .iter()
        .scan(0u64, |offset, record| {
            let start = *offset;
            *offset += record.len() as u64;
            Some(start)
        })
        .collect::<Vec<_>>();

    let key_block_counts = [2usize, 2, 2];
    let mut key_payloads = Vec::new();
    let mut relative_record_offsets = Vec::new();
    let mut summaries = Vec::new();
    let mut entry_index = 0usize;
    for count in key_block_counts {
        let end = entry_index + count;
        let mut payload = Vec::new();
        let mut offsets = Vec::new();
        for index in entry_index..end {
            offsets.push(payload.len());
            put_u64_be(&mut payload, record_starts[index]);
            payload.extend_from_slice(&encode_key(kind, keys[index]));
            payload.extend(std::iter::repeat_n(0, key_unit_size(kind)));
        }
        key_payloads.push(payload);
        relative_record_offsets.push(offsets);
        summaries.push((keys[entry_index], keys[end - 1]));
        entry_index = end;
    }
    let key_blocks = key_payloads
        .iter()
        .map(|payload| uncompressed_block(payload))
        .collect::<Vec<_>>();

    let record_payloads = split_bytes(&record_data, &[2, 3, 4]);
    let record_blocks = record_payloads
        .iter()
        .map(|payload| uncompressed_block(payload))
        .collect::<Vec<_>>();

    let mut key_index_payload = Vec::new();
    for index in 0..key_blocks.len() {
        put_u64_be(&mut key_index_payload, 2);
        put_summary(&mut key_index_payload, kind, summaries[index].0);
        put_summary(&mut key_index_payload, kind, summaries[index].1);
        put_u64_be(&mut key_index_payload, key_blocks[index].len() as u64);
        put_u64_be(&mut key_index_payload, key_payloads[index].len() as u64);
    }
    let key_index_block = uncompressed_block(&key_index_payload);

    let header_xml = match kind {
        Kind::Mdx => concat!(
            "<Dictionary GeneratedByEngineVersion=\"2.0\" ",
            "RequiredEngineVersion=\"2.0\" Encoding=\"UTF-8\" ",
            "KeyCaseSensitive=\"No\" StripKey=\"No\"/>"
        ),
        Kind::Mdd => concat!(
            "<Library_Data GeneratedByEngineVersion=\"2.0\" ",
            "RequiredEngineVersion=\"2.0\" ",
            "KeyCaseSensitive=\"No\" StripKey=\"No\"/>"
        ),
    };
    let header_xml = utf16le(header_xml);

    let mut bytes = Vec::new();
    put_u32_be(&mut bytes, header_xml.len() as u32);
    bytes.extend_from_slice(&header_xml);
    put_u32_le(&mut bytes, adler32(&header_xml));

    let keyword_header_start = bytes.len();
    let mut keyword_header = Vec::new();
    put_u64_be(&mut keyword_header, key_blocks.len() as u64);
    put_u64_be(&mut keyword_header, keys.len() as u64);
    put_u64_be(&mut keyword_header, key_index_payload.len() as u64);
    put_u64_be(&mut keyword_header, key_index_block.len() as u64);
    put_u64_be(
        &mut keyword_header,
        key_blocks.iter().map(Vec::len).sum::<usize>() as u64,
    );
    bytes.extend_from_slice(&keyword_header);
    put_u32_be(&mut bytes, adler32(&keyword_header));
    let keyword_header_range = keyword_header_start..bytes.len();

    let key_index_start = bytes.len();
    bytes.extend_from_slice(&key_index_block);
    let key_index_range = key_index_start..bytes.len();

    let mut key_block_ranges = Vec::new();
    let mut record_offset_positions = Vec::new();
    for (index, block) in key_blocks.iter().enumerate() {
        let start = bytes.len();
        bytes.extend_from_slice(block);
        record_offset_positions.extend(
            relative_record_offsets[index]
                .iter()
                .map(|relative| start + 8 + relative),
        );
        key_block_ranges.push(start..bytes.len());
    }

    let record_header_start = bytes.len();
    put_u64_be(&mut bytes, record_blocks.len() as u64);
    put_u64_be(&mut bytes, keys.len() as u64);
    put_u64_be(&mut bytes, (record_blocks.len() * 16) as u64);
    put_u64_be(
        &mut bytes,
        record_blocks.iter().map(Vec::len).sum::<usize>() as u64,
    );
    let record_header_range = record_header_start..bytes.len();

    let record_index_start = bytes.len();
    for (payload, block) in record_payloads.iter().zip(&record_blocks) {
        put_u64_be(&mut bytes, block.len() as u64);
        put_u64_be(&mut bytes, payload.len() as u64);
    }
    let record_index_range = record_index_start..bytes.len();

    let mut record_block_ranges = Vec::new();
    for block in &record_blocks {
        let start = bytes.len();
        bytes.extend_from_slice(block);
        record_block_ranges.push(start..bytes.len());
    }

    Fixture {
        kind,
        bytes,
        layout: Layout {
            keyword_header: keyword_header_range,
            key_index: key_index_range,
            key_blocks: key_block_ranges,
            record_header: record_header_range,
            record_index: record_index_range,
            record_blocks: record_block_ranges,
            record_offsets: record_offset_positions,
        },
    }
}

pub fn mutate_region(bytes: &mut [u8], range: &Range<usize>, data: &[u8]) {
    if range.is_empty() {
        return;
    }
    for mutation in data.chunks_exact(3).take(MAX_MUTATIONS) {
        let relative = usize::from(u16::from_be_bytes([mutation[0], mutation[1]])) % range.len();
        bytes[range.start + relative] ^= mutation[2];
    }
}

pub fn mutate_block_payload(bytes: &mut [u8], range: &Range<usize>, data: &[u8]) {
    if range.len() <= 8 {
        return;
    }
    let payload = range.start + 8..range.end;
    mutate_region(bytes, &payload, data);
    refresh_block_checksum(bytes, range);
}

pub fn mutate_record_offsets(fixture: &mut Fixture, data: &[u8]) {
    if fixture.layout.record_offsets.is_empty() {
        return;
    }
    for mutation in data.chunks_exact(3).take(MAX_MUTATIONS) {
        let entry = usize::from(mutation[0]) % fixture.layout.record_offsets.len();
        let byte = usize::from(mutation[1]) % 8;
        fixture.bytes[fixture.layout.record_offsets[entry] + byte] ^= mutation[2];
    }
    for block in &fixture.layout.key_blocks {
        refresh_block_checksum(&mut fixture.bytes, block);
    }
}

pub fn corrupt_block_envelope(bytes: &mut [u8], range: &Range<usize>, data: &[u8]) {
    if range.len() < 8 || data.is_empty() {
        return;
    }
    let envelope = range.start..range.start + 8;
    mutate_region(bytes, &envelope, data);
}

pub fn exercise_bytes(kind: Kind, label: &str, bytes: &[u8], deep: bool) -> bool {
    let Some(temp) = TempDictionary::write(label, kind, bytes) else {
        return false;
    };
    match kind {
        Kind::Mdx => exercise_mdx(temp.path(), deep),
        Kind::Mdd => exercise_mdd(temp.path(), deep),
    }
}

fn exercise_mdx(path: &Path, deep: bool) -> bool {
    let options = fuzz_options();
    let Ok(dictionary) = MdxFile::open_with_options(path, &options) else {
        return false;
    };
    let len = dictionary.len();
    let keys = dictionary
        .keys()
        .take(MAX_ROWS)
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    for key in &keys {
        let _ = dictionary.key_at(key.ordinal());
    }
    if len <= MAX_ROWS as u64 {
        for key in &keys {
            let _ = dictionary.locate(key.key());
        }
    }
    if deep {
        for ordinal in 0..len.min(MAX_ROWS as u64) {
            let _ = dictionary.entry_at(KeyOrdinal::new(ordinal));
        }
        for entry in dictionary.entries().take(MAX_ROWS) {
            let _ = entry;
        }
    }
    true
}

fn exercise_mdd(path: &Path, deep: bool) -> bool {
    let options = fuzz_options();
    let Ok(dictionary) = MddFile::open_with_options(path, &options) else {
        return false;
    };
    let len = dictionary.len();
    let keys = dictionary
        .keys()
        .take(MAX_ROWS)
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    for key in &keys {
        let _ = dictionary.key_at(key.ordinal());
    }
    if len <= MAX_ROWS as u64 {
        for key in &keys {
            let _ = dictionary.locate(key.key());
        }
    }
    if deep {
        for ordinal in 0..len.min(MAX_ROWS as u64) {
            if let Ok(Some(span)) = dictionary.span_at(KeyOrdinal::new(ordinal))
                && span.len() <= MAX_STREAMED_SPAN
            {
                let _ = span.copy_to(&mut io::sink());
                let _ = span.read();
            }
        }
    }
    true
}

fn fuzz_options() -> OpenOptions {
    let limits = Limits::new()
        .with_header_xml_bytes(256 * 1024)
        .with_header_attributes(256)
        .with_key_index_bytes(1024 * 1024)
        .with_record_index_bytes(1024 * 1024)
        .with_compressed_block_bytes(1024 * 1024)
        .with_decompressed_block_bytes(2 * 1024 * 1024)
        .with_block_metadata_bytes(1024 * 1024)
        .with_key_block_entries(4096)
        .with_materialized_record_bytes(MAX_STREAMED_SPAN as usize)
        .with_locator_entries(MAX_ROWS as u64)
        .with_locator_bytes(2 * 1024 * 1024)
        .with_working_memory_bytes(16 * 1024 * 1024);
    OpenOptions::new().with_limits(limits)
}

fn refresh_block_checksum(bytes: &mut [u8], range: &Range<usize>) {
    let checksum = adler32(&bytes[range.start + 8..range.end]);
    bytes[range.start + 4..range.start + 8].copy_from_slice(&checksum.to_be_bytes());
}

struct TempDictionary {
    path: PathBuf,
}

impl TempDictionary {
    fn write(label: &str, kind: Kind, bytes: &[u8]) -> Option<Self> {
        let serial = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "mdictlib-fuzz-{label}-{}-{serial}.{}",
            std::process::id(),
            kind.extension()
        ));
        fs::write(&path, bytes).ok()?;
        Some(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDictionary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn split_bytes(bytes: &[u8], sizes: &[usize]) -> Vec<Vec<u8>> {
    let mut offset = 0usize;
    sizes
        .iter()
        .map(|size| {
            let part = bytes[offset..offset + size].to_vec();
            offset += size;
            part
        })
        .collect()
}

fn put_summary(output: &mut Vec<u8>, kind: Kind, summary: &str) {
    let encoded = encode_key(kind, summary);
    put_u16_be(output, (encoded.len() / key_unit_size(kind)) as u16);
    output.extend_from_slice(&encoded);
    output.extend(std::iter::repeat_n(0, key_unit_size(kind)));
}

fn encode_key(kind: Kind, key: &str) -> Vec<u8> {
    match kind {
        Kind::Mdx => key.as_bytes().to_vec(),
        Kind::Mdd => utf16le(key),
    }
}

const fn key_unit_size(kind: Kind) -> usize {
    match kind {
        Kind::Mdx => 1,
        Kind::Mdd => 2,
    }
}

fn uncompressed_block(payload: &[u8]) -> Vec<u8> {
    let mut output = vec![0, 0, 0, 0];
    put_u32_be(&mut output, adler32(payload));
    output.extend_from_slice(payload);
    output
}

fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn put_u16_be(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u32_be(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u32_le(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_u64_be(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn adler32(bytes: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65_521;
    let mut first = 1u32;
    let mut second = 0u32;
    for byte in bytes {
        first = (first + u32::from(*byte)) % MOD_ADLER;
        second = (second + first) % MOD_ADLER;
    }
    (second << 16) | first
}

// ---------------------------------------------------------------------------
// Version 1 fixtures
// ---------------------------------------------------------------------------

/// Which wire grammar a fixture is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    V1,
    V2,
}

/// Builds a structurally valid version 1 fixture with the same logical content
/// as [`fixture`], so the two grammars can be fuzzed against equal baselines.
pub fn fixture_v1(kind: Kind) -> Fixture {
    let keys = match kind {
        Kind::Mdx => ["alpha", "duplicate", "duplicate", "empty", "omega", "zulu"],
        Kind::Mdd => [
            "\\alpha.bin",
            "\\duplicate.bin",
            "\\duplicate.bin",
            "\\empty.bin",
            "\\omega.bin",
            "\\zulu.bin",
        ],
    };
    let records: [&[u8]; 6] = [b"A", b"BB", b"CCC", b"", b"DD", b"E"];
    let record_data = records.concat();
    let record_starts = records
        .iter()
        .scan(0u32, |offset, record| {
            let start = *offset;
            *offset += record.len() as u32;
            Some(start)
        })
        .collect::<Vec<_>>();

    let key_block_counts = [2usize, 2, 2];
    let mut key_payloads = Vec::new();
    let mut relative_record_offsets = Vec::new();
    let mut summaries = Vec::new();
    let mut entry_index = 0usize;
    for count in key_block_counts {
        let end = entry_index + count;
        let mut payload = Vec::new();
        let mut offsets = Vec::new();
        for index in entry_index..end {
            offsets.push(payload.len());
            put_u32_be(&mut payload, record_starts[index]);
            payload.extend_from_slice(&encode_key(kind, keys[index]));
            payload.extend(std::iter::repeat_n(0, key_unit_size(kind)));
        }
        key_payloads.push(payload);
        relative_record_offsets.push(offsets);
        summaries.push((keys[entry_index], keys[end - 1]));
        entry_index = end;
    }
    let key_blocks = key_payloads
        .iter()
        .map(|payload| uncompressed_block(payload))
        .collect::<Vec<_>>();

    let record_payloads = split_bytes(&record_data, &[2, 3, 4]);
    let record_blocks = record_payloads
        .iter()
        .map(|payload| uncompressed_block(payload))
        .collect::<Vec<_>>();

    // Raw keyword metadata: no envelope, one-byte summary lengths, no
    // terminators, u32 sizes.
    let mut key_info = Vec::new();
    for index in 0..key_blocks.len() {
        put_u32_be(&mut key_info, 2);
        put_v1_summary(&mut key_info, kind, summaries[index].0);
        put_v1_summary(&mut key_info, kind, summaries[index].1);
        put_u32_be(&mut key_info, key_blocks[index].len() as u32);
        put_u32_be(&mut key_info, key_payloads[index].len() as u32);
    }

    let header_xml = match kind {
        Kind::Mdx => concat!(
            "<Dictionary GeneratedByEngineVersion=\"1.2\" ",
            "RequiredEngineVersion=\"1.2\" Encoding=\"UTF-8\" ",
            "KeyCaseSensitive=\"No\" StripKey=\"No\"/>"
        ),
        Kind::Mdd => concat!(
            "<Library_Data GeneratedByEngineVersion=\"1.2\" ",
            "RequiredEngineVersion=\"1.2\" ",
            "KeyCaseSensitive=\"No\" StripKey=\"No\"/>"
        ),
    };
    let header_xml = utf16le(header_xml);

    let mut bytes = Vec::new();
    put_u32_be(&mut bytes, header_xml.len() as u32);
    bytes.extend_from_slice(&header_xml);
    put_u32_le(&mut bytes, adler32(&header_xml));

    let keyword_header_start = bytes.len();
    put_u32_be(&mut bytes, key_blocks.len() as u32);
    put_u32_be(&mut bytes, keys.len() as u32);
    put_u32_be(&mut bytes, key_info.len() as u32);
    put_u32_be(
        &mut bytes,
        key_blocks.iter().map(Vec::len).sum::<usize>() as u32,
    );
    let keyword_header_range = keyword_header_start..bytes.len();

    let key_index_start = bytes.len();
    bytes.extend_from_slice(&key_info);
    let key_index_range = key_index_start..bytes.len();

    let mut key_block_ranges = Vec::new();
    let mut record_offset_positions = Vec::new();
    for (index, block) in key_blocks.iter().enumerate() {
        let start = bytes.len();
        bytes.extend_from_slice(block);
        record_offset_positions.extend(
            relative_record_offsets[index]
                .iter()
                .map(|relative| start + 8 + relative),
        );
        key_block_ranges.push(start..bytes.len());
    }

    let record_header_start = bytes.len();
    put_u32_be(&mut bytes, record_blocks.len() as u32);
    put_u32_be(&mut bytes, keys.len() as u32);
    put_u32_be(&mut bytes, (record_blocks.len() * 8) as u32);
    put_u32_be(
        &mut bytes,
        record_blocks.iter().map(Vec::len).sum::<usize>() as u32,
    );
    let record_header_range = record_header_start..bytes.len();

    let record_index_start = bytes.len();
    for (payload, block) in record_payloads.iter().zip(&record_blocks) {
        put_u32_be(&mut bytes, block.len() as u32);
        put_u32_be(&mut bytes, payload.len() as u32);
    }
    let record_index_range = record_index_start..bytes.len();

    let mut record_block_ranges = Vec::new();
    for block in &record_blocks {
        let start = bytes.len();
        bytes.extend_from_slice(block);
        record_block_ranges.push(start..bytes.len());
    }

    Fixture {
        kind,
        bytes,
        layout: Layout {
            keyword_header: keyword_header_range,
            key_index: key_index_range,
            key_blocks: key_block_ranges,
            record_header: record_header_range,
            record_index: record_index_range,
            record_blocks: record_block_ranges,
            record_offsets: record_offset_positions,
        },
    }
}

/// Builds a fixture in the requested wire grammar.
pub fn fixture_for(wire: Wire, kind: Kind) -> Fixture {
    match wire {
        Wire::V1 => fixture_v1(kind),
        Wire::V2 => fixture(kind),
    }
}

/// Rewrites the declared engine version in a fixture's header XML.
///
/// The header XML is UTF-16LE and its ADLER32 must be refreshed, so this also
/// exercises the header checksum path.
pub fn set_declared_major_version(fixture: &mut Fixture, major: u8) {
    let xml_len = u32::from_be_bytes([
        fixture.bytes[0],
        fixture.bytes[1],
        fixture.bytes[2],
        fixture.bytes[3],
    ]) as usize;
    let xml_range = 4..4 + xml_len;
    // The major digit is the first UTF-16 unit after `EngineVersion="`.
    let needle = utf16le("EngineVersion=\"");
    let mut cursor = xml_range.start;
    while cursor + needle.len() + 2 <= xml_range.end {
        if fixture.bytes[cursor..cursor + needle.len()] == needle[..] {
            let digit = cursor + needle.len();
            fixture.bytes[digit] = b'0' + major;
            fixture.bytes[digit + 1] = 0;
        }
        cursor += 2;
    }
    let checksum = adler32(&fixture.bytes[xml_range]);
    let checksum_offset = 4 + xml_len;
    fixture.bytes[checksum_offset..checksum_offset + 4].copy_from_slice(&checksum.to_le_bytes());
}

fn put_v1_summary(output: &mut Vec<u8>, kind: Kind, summary: &str) {
    let encoded = encode_key(kind, summary);
    output.push((encoded.len() / key_unit_size(kind)) as u8);
    output.extend_from_slice(&encoded);
    // Version 1 summaries carry no terminator.
}
