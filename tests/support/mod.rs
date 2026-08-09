#![allow(dead_code)]

mod crypto;

use std::fs::{self, OpenOptions as FsOpenOptions};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use encoding_rs::{BIG5, GB18030, GBK};

pub fn independent_ripemd128(bytes: &[u8]) -> [u8; 16] {
    crypto::ripemd128(bytes)
}

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureKind {
    Mdx,
    Mdd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureEncoding {
    Utf8,
    Utf16Le,
    Gbk,
    Gb18030,
    Big5,
}

impl FixtureEncoding {
    fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16Le => "UTF-16LE",
            Self::Gbk => "GBK",
            Self::Gb18030 => "GB18030",
            Self::Big5 => "BIG5",
        }
    }

    fn unit_size(self) -> usize {
        match self {
            Self::Utf16Le => 2,
            Self::Utf8 | Self::Gbk | Self::Gb18030 | Self::Big5 => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureCompression {
    None,
    Zlib,
    Lzo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixturePasscode {
    pub reg_code_hex: String,
    pub user_id: String,
}

impl FixturePasscode {
    pub fn new(reg_code_hex: impl Into<String>, user_id: impl Into<String>) -> Self {
        let passcode = Self {
            reg_code_hex: reg_code_hex.into(),
            user_id: user_id.into(),
        };
        assert_eq!(passcode.reg_code_hex.len(), 32);
        assert!(
            passcode
                .reg_code_hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
        passcode
    }
}

#[derive(Debug, Clone)]
struct FixtureEntry {
    key: String,
    record: Vec<u8>,
}

/// Independent, test-only builder for the MDict v2 layout consumed by the
/// public reader. It does not call the library's parser, checksum, crypto, or
/// block-codec implementation; standard text/zlib crates are invoked directly.
#[derive(Debug, Clone)]
pub struct FixtureBuilder {
    kind: FixtureKind,
    encoding: FixtureEncoding,
    entries: Vec<FixtureEntry>,
    key_block_counts: Vec<usize>,
    record_block_sizes: Option<Vec<usize>>,
    record_starts: Option<Vec<u64>>,
    key_summaries: Option<Vec<(String, String)>>,
    key_case_attribute: (String, String),
    strip_key_attribute: (String, String),
    extra_header_attributes: Vec<(String, String)>,
    key_index_trailing_bytes: Vec<u8>,
    record_index_trailing_bytes: Vec<u8>,
    key_index_compression: FixtureCompression,
    key_block_compression: FixtureCompression,
    record_block_compression: FixtureCompression,
    encrypt_keyword_index: bool,
    keyword_header_passcode: Option<FixturePasscode>,
}

impl FixtureBuilder {
    pub fn mdx(entries: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>) -> Self {
        Self::new(
            FixtureKind::Mdx,
            entries
                .into_iter()
                .map(|(key, text)| FixtureEntry {
                    key: key.into(),
                    record: text.into().into_bytes(),
                })
                .collect(),
        )
    }

    pub fn mdd(entries: impl IntoIterator<Item = (impl Into<String>, Vec<u8>)>) -> Self {
        Self::new(
            FixtureKind::Mdd,
            entries
                .into_iter()
                .map(|(key, record)| FixtureEntry {
                    key: key.into(),
                    record,
                })
                .collect(),
        )
    }

    fn new(kind: FixtureKind, entries: Vec<FixtureEntry>) -> Self {
        let entry_count = entries.len();
        Self {
            kind,
            encoding: match kind {
                FixtureKind::Mdx => FixtureEncoding::Utf8,
                FixtureKind::Mdd => FixtureEncoding::Utf16Le,
            },
            entries,
            key_block_counts: if entry_count == 0 {
                Vec::new()
            } else {
                vec![entry_count]
            },
            record_block_sizes: None,
            record_starts: None,
            key_summaries: None,
            key_case_attribute: ("KeyCaseSensitive".to_owned(), "No".to_owned()),
            strip_key_attribute: ("StripKey".to_owned(), "No".to_owned()),
            extra_header_attributes: Vec::new(),
            key_index_trailing_bytes: Vec::new(),
            record_index_trailing_bytes: Vec::new(),
            key_index_compression: FixtureCompression::None,
            key_block_compression: FixtureCompression::None,
            record_block_compression: FixtureCompression::None,
            encrypt_keyword_index: false,
            keyword_header_passcode: None,
        }
    }

    pub fn encoding(mut self, encoding: FixtureEncoding) -> Self {
        assert_eq!(self.kind, FixtureKind::Mdx, "MDD keys are always UTF-16LE");
        self.encoding = encoding;
        self
    }

    pub fn compression(mut self, compression: FixtureCompression) -> Self {
        self.key_index_compression = compression;
        self.key_block_compression = compression;
        self.record_block_compression = compression;
        self
    }

    pub fn mixed_compression(
        mut self,
        key_index: FixtureCompression,
        key_blocks: FixtureCompression,
        record_blocks: FixtureCompression,
    ) -> Self {
        self.key_index_compression = key_index;
        self.key_block_compression = key_blocks;
        self.record_block_compression = record_blocks;
        self
    }

    pub fn encrypt_keyword_index(mut self) -> Self {
        self.encrypt_keyword_index = true;
        self
    }

    pub fn encrypt_keyword_header(mut self, passcode: FixturePasscode) -> Self {
        self.keyword_header_passcode = Some(passcode);
        self
    }

    pub fn key_blocks(mut self, entry_counts: impl Into<Vec<usize>>) -> Self {
        self.key_block_counts = entry_counts.into();
        self
    }

    pub fn record_blocks(mut self, decompressed_sizes: impl Into<Vec<usize>>) -> Self {
        self.record_block_sizes = Some(decompressed_sizes.into());
        self
    }

    pub fn record_starts(mut self, starts: impl Into<Vec<u64>>) -> Self {
        self.record_starts = Some(starts.into());
        self
    }

    pub fn key_summaries(
        mut self,
        summaries: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.key_summaries = Some(
            summaries
                .into_iter()
                .map(|(first, last)| (first.into(), last.into()))
                .collect(),
        );
        self
    }

    pub fn key_case_attribute(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.key_case_attribute = (name.into(), value.into());
        self
    }

    pub fn strip_key_attribute(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.strip_key_attribute = (name.into(), value.into());
        self
    }

    pub fn header_attribute(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_header_attributes
            .push((name.into(), value.into()));
        self
    }

    pub fn key_index_trailing_bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.key_index_trailing_bytes = bytes.into();
        self
    }

    pub fn record_index_trailing_bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.record_index_trailing_bytes = bytes.into();
        self
    }

    pub fn build(self) -> BuiltFixture {
        assert_eq!(
            self.key_block_counts.iter().sum::<usize>(),
            self.entries.len(),
            "key-block entry counts must cover every fixture entry"
        );
        assert!(
            self.key_block_counts.iter().all(|count| *count > 0),
            "fixture key blocks must not be empty"
        );

        let encoded_records = self
            .entries
            .iter()
            .map(|entry| match self.kind {
                FixtureKind::Mdx => {
                    let text = std::str::from_utf8(&entry.record).unwrap();
                    encode_text(self.encoding, text)
                }
                FixtureKind::Mdd => entry.record.clone(),
            })
            .collect::<Vec<_>>();
        let record_data = encoded_records
            .iter()
            .flat_map(|record| record.iter().copied())
            .collect::<Vec<_>>();
        let record_starts = match self.record_starts {
            Some(starts) => {
                assert_eq!(starts.len(), self.entries.len());
                starts
            }
            None => {
                let mut offset = 0u64;
                encoded_records
                    .iter()
                    .map(|record| {
                        let start = offset;
                        offset = offset
                            .checked_add(u64::try_from(record.len()).unwrap())
                            .unwrap();
                        start
                    })
                    .collect()
            }
        };

        let record_block_payloads = split_record_blocks(
            &record_data,
            self.record_block_sizes.unwrap_or_else(|| {
                (!record_data.is_empty())
                    .then_some(vec![record_data.len()])
                    .unwrap_or_default()
            }),
        );

        let encoded_keys = self
            .entries
            .iter()
            .map(|entry| encode_text(self.encoding, &entry.key))
            .collect::<Vec<_>>();

        let mut entry_cursor = 0usize;
        let mut key_block_payloads = Vec::with_capacity(self.key_block_counts.len());
        let mut default_summaries = Vec::with_capacity(self.key_block_counts.len());
        for &entry_count in &self.key_block_counts {
            let end = entry_cursor + entry_count;
            let mut payload = Vec::new();
            for entry_index in entry_cursor..end {
                put_u64_be(&mut payload, record_starts[entry_index]);
                payload.extend_from_slice(&encoded_keys[entry_index]);
                payload.extend(std::iter::repeat_n(0, self.encoding.unit_size()));
            }
            key_block_payloads.push(payload);
            default_summaries.push((
                self.entries[entry_cursor].key.clone(),
                self.entries[end - 1].key.clone(),
            ));
            entry_cursor = end;
        }

        let summaries = self.key_summaries.unwrap_or(default_summaries);
        assert_eq!(summaries.len(), self.key_block_counts.len());

        let key_blocks = key_block_payloads
            .iter()
            .map(|payload| encode_block(payload, self.key_block_compression))
            .collect::<Vec<_>>();
        let record_blocks = record_block_payloads
            .iter()
            .map(|payload| encode_block(payload, self.record_block_compression))
            .collect::<Vec<_>>();

        let mut key_index_payload = Vec::new();
        for (((&entry_count, (first, last)), payload), block) in self
            .key_block_counts
            .iter()
            .zip(summaries.iter())
            .zip(key_block_payloads.iter())
            .zip(key_blocks.iter())
        {
            put_u64_be(&mut key_index_payload, u64::try_from(entry_count).unwrap());
            put_summary(&mut key_index_payload, self.encoding, first);
            put_summary(&mut key_index_payload, self.encoding, last);
            put_u64_be(&mut key_index_payload, u64::try_from(block.len()).unwrap());
            put_u64_be(
                &mut key_index_payload,
                u64::try_from(payload.len()).unwrap(),
            );
        }
        key_index_payload.extend_from_slice(&self.key_index_trailing_bytes);
        let mut key_index_block = encode_block(&key_index_payload, self.key_index_compression);
        if self.encrypt_keyword_index {
            let checksum = u32::from_be_bytes(key_index_block[4..8].try_into().unwrap());
            crypto::encrypt_keyword_index(checksum, &mut key_index_block[8..]);
        }

        let encryption_bits = u8::from(self.keyword_header_passcode.is_some())
            | (u8::from(self.encrypt_keyword_index) << 1);

        let header_xml = header_xml(
            self.kind,
            self.encoding,
            encryption_bits,
            &self.key_case_attribute,
            &self.strip_key_attribute,
            &self.extra_header_attributes,
        );
        let header_xml_bytes = utf16le(&header_xml);

        let mut bytes = Vec::new();
        put_u32_be(&mut bytes, u32::try_from(header_xml_bytes.len()).unwrap());
        bytes.extend_from_slice(&header_xml_bytes);
        let header_checksum_offset = bytes.len();
        put_u32_le(&mut bytes, adler32(&header_xml_bytes));

        let keyword_header_offset = bytes.len();
        let key_blocks_len = key_blocks.iter().map(Vec::len).sum::<usize>();
        let mut keyword_header_payload = Vec::with_capacity(40);
        put_u64_be(
            &mut keyword_header_payload,
            u64::try_from(self.key_block_counts.len()).unwrap(),
        );
        put_u64_be(
            &mut keyword_header_payload,
            u64::try_from(self.entries.len()).unwrap(),
        );
        put_u64_be(
            &mut keyword_header_payload,
            u64::try_from(key_index_payload.len()).unwrap(),
        );
        put_u64_be(
            &mut keyword_header_payload,
            u64::try_from(key_index_block.len()).unwrap(),
        );
        put_u64_be(
            &mut keyword_header_payload,
            u64::try_from(key_blocks_len).unwrap(),
        );
        debug_assert_eq!(keyword_header_payload.len(), 40);
        let keyword_header_checksum = adler32(&keyword_header_payload);
        if let Some(passcode) = &self.keyword_header_passcode {
            crypto::encrypt_keyword_header(
                &mut keyword_header_payload,
                &passcode.reg_code_hex,
                &passcode.user_id,
            );
        }
        bytes.extend_from_slice(&keyword_header_payload);
        put_u32_be(&mut bytes, keyword_header_checksum);

        let key_index_start = bytes.len();
        bytes.extend_from_slice(&key_index_block);
        let key_index_range = key_index_start..bytes.len();

        let mut key_block_ranges = Vec::with_capacity(key_blocks.len());
        for block in &key_blocks {
            let start = bytes.len();
            bytes.extend_from_slice(block);
            key_block_ranges.push(start..bytes.len());
        }

        let record_header_offset = bytes.len();
        let record_index_len = record_blocks.len() * 16 + self.record_index_trailing_bytes.len();
        let record_blocks_len = record_blocks.iter().map(Vec::len).sum::<usize>();
        put_u64_be(&mut bytes, u64::try_from(record_blocks.len()).unwrap());
        put_u64_be(&mut bytes, u64::try_from(self.entries.len()).unwrap());
        put_u64_be(&mut bytes, u64::try_from(record_index_len).unwrap());
        put_u64_be(&mut bytes, u64::try_from(record_blocks_len).unwrap());

        let record_index_start = bytes.len();
        for (payload, block) in record_block_payloads.iter().zip(record_blocks.iter()) {
            put_u64_be(&mut bytes, u64::try_from(block.len()).unwrap());
            put_u64_be(&mut bytes, u64::try_from(payload.len()).unwrap());
        }
        bytes.extend_from_slice(&self.record_index_trailing_bytes);
        let record_index_range = record_index_start..bytes.len();

        let mut record_block_ranges = Vec::with_capacity(record_blocks.len());
        for block in &record_blocks {
            let start = bytes.len();
            bytes.extend_from_slice(block);
            record_block_ranges.push(start..bytes.len());
        }

        BuiltFixture {
            kind: self.kind,
            bytes,
            layout: FixtureLayout {
                header_checksum_offset,
                keyword_header_offset,
                key_index_block: key_index_range,
                key_blocks: key_block_ranges,
                record_header_offset,
                record_index: record_index_range,
                record_blocks: record_block_ranges,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct FixtureLayout {
    pub header_checksum_offset: usize,
    pub keyword_header_offset: usize,
    pub key_index_block: Range<usize>,
    pub key_blocks: Vec<Range<usize>>,
    pub record_header_offset: usize,
    pub record_index: Range<usize>,
    pub record_blocks: Vec<Range<usize>>,
}

#[derive(Debug, Clone)]
pub struct BuiltFixture {
    pub kind: FixtureKind,
    pub bytes: Vec<u8>,
    pub layout: FixtureLayout,
}

impl BuiltFixture {
    pub fn write(&self, name: &str) -> TempDictionary {
        TempDictionary::write(name, self.kind, &self.bytes)
    }

    pub fn write_sparse(&self, name: &str, logical_len: u64) -> TempDictionary {
        assert!(logical_len >= u64::try_from(self.bytes.len()).unwrap());
        TempDictionary::write_sparse(name, self.kind, &self.bytes, logical_len)
    }

    pub fn set_keyword_u64(&mut self, field_index: usize, value: u64) {
        assert!(field_index < 5);
        let offset = self.layout.keyword_header_offset + field_index * 8;
        self.bytes[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
        self.refresh_keyword_header_checksum();
    }

    pub fn set_record_u64(&mut self, field_index: usize, value: u64) {
        assert!(field_index < 4);
        let offset = self.layout.record_header_offset + field_index * 8;
        self.bytes[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
    }

    pub fn corrupt_block_checksum(&mut self, range: &Range<usize>) {
        assert!(range.end - range.start >= 8);
        self.bytes[range.start + 4] ^= 0x80;
    }

    pub fn set_uncompressed_payload_byte(
        &mut self,
        range: &Range<usize>,
        payload_index: usize,
        value: u8,
    ) {
        self.set_uncompressed_payload_bytes(range, payload_index, &[value]);
    }

    pub fn set_uncompressed_payload_bytes(
        &mut self,
        range: &Range<usize>,
        payload_index: usize,
        values: &[u8],
    ) {
        assert!(range.end - range.start >= 8);
        let payload_range = range.start + 8..range.end;
        let start = payload_range.start + payload_index;
        self.bytes[start..start + values.len()].copy_from_slice(values);
        let checksum = adler32(&self.bytes[payload_range]);
        self.bytes[range.start + 4..range.start + 8].copy_from_slice(&checksum.to_be_bytes());
    }

    fn refresh_keyword_header_checksum(&mut self) {
        let start = self.layout.keyword_header_offset;
        let checksum = adler32(&self.bytes[start..start + 40]);
        self.bytes[start + 40..start + 44].copy_from_slice(&checksum.to_be_bytes());
    }
}

#[derive(Debug)]
pub struct TempDictionary {
    path: PathBuf,
}

impl TempDictionary {
    fn write(name: &str, kind: FixtureKind, bytes: &[u8]) -> Self {
        let serial = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let extension = match kind {
            FixtureKind::Mdx => "mdx",
            FixtureKind::Mdd => "mdd",
        };
        let path = std::env::temp_dir().join(format!(
            "mdictlib-{name}-{}-{serial}.{extension}",
            std::process::id()
        ));
        fs::write(&path, bytes).unwrap();
        Self { path }
    }

    fn write_sparse(name: &str, kind: FixtureKind, prefix: &[u8], logical_len: u64) -> Self {
        let dictionary = Self::write(name, kind, prefix);
        FsOpenOptions::new()
            .write(true)
            .open(&dictionary.path)
            .unwrap()
            .set_len(logical_len)
            .unwrap();
        dictionary
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDictionary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn split_record_blocks(data: &[u8], sizes: Vec<usize>) -> Vec<Vec<u8>> {
    assert_eq!(
        sizes.iter().sum::<usize>(),
        data.len(),
        "record-block sizes must cover the concatenated record bytes"
    );
    assert!(
        sizes.iter().all(|size| *size > 0),
        "fixture record blocks must not be empty"
    );
    let mut cursor = 0usize;
    sizes
        .into_iter()
        .map(|size| {
            let block = data[cursor..cursor + size].to_vec();
            cursor += size;
            block
        })
        .collect()
}

fn header_xml(
    kind: FixtureKind,
    encoding: FixtureEncoding,
    encryption_bits: u8,
    key_case_attribute: &(String, String),
    strip_key_attribute: &(String, String),
    extra_attributes: &[(String, String)],
) -> String {
    let tag = match kind {
        FixtureKind::Mdx => "Dictionary",
        FixtureKind::Mdd => "Library_Data",
    };
    let mut attributes = vec![
        ("GeneratedByEngineVersion", "2.0"),
        ("RequiredEngineVersion", "2.0"),
    ];
    if kind == FixtureKind::Mdx {
        attributes.push(("Encoding", encoding.label()));
    }
    let encryption_value = encryption_bits.to_string();
    if encryption_bits != 0 {
        attributes.push(("Encrypted", &encryption_value));
    }

    let mut xml = format!("<{tag}");
    for (name, value) in attributes {
        xml.push(' ');
        xml.push_str(name);
        xml.push_str("=\"");
        xml.push_str(value);
        xml.push('"');
    }
    for (name, value) in [key_case_attribute, strip_key_attribute]
        .into_iter()
        .chain(extra_attributes.iter())
    {
        xml.push(' ');
        xml.push_str(name);
        xml.push_str("=\"");
        xml.push_str(&escape_xml(value));
        xml.push('"');
    }
    xml.push_str("/>");
    xml
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn put_summary(output: &mut Vec<u8>, encoding: FixtureEncoding, summary: &str) {
    let encoded = encode_text(encoding, summary);
    let units = encoded.len() / encoding.unit_size();
    put_u16_be(output, u16::try_from(units).unwrap());
    output.extend_from_slice(&encoded);
    output.extend(std::iter::repeat_n(0, encoding.unit_size()));
}

fn encode_text(encoding: FixtureEncoding, text: &str) -> Vec<u8> {
    match encoding {
        FixtureEncoding::Utf8 => text.as_bytes().to_vec(),
        FixtureEncoding::Utf16Le => utf16le(text),
        FixtureEncoding::Gbk => encode_legacy(GBK, text),
        FixtureEncoding::Gb18030 => encode_legacy(GB18030, text),
        FixtureEncoding::Big5 => encode_legacy(BIG5, text),
    }
}

fn encode_legacy(encoding: &'static encoding_rs::Encoding, text: &str) -> Vec<u8> {
    let (encoded, _actual_encoding, had_errors) = encoding.encode(text);
    assert!(
        !had_errors,
        "fixture text is not representable by {encoding:?}"
    );
    encoded.into_owned()
}

fn encode_block(payload: &[u8], compression: FixtureCompression) -> Vec<u8> {
    let (tag, compressed) = match compression {
        FixtureCompression::None => ([0, 0, 0, 0], payload.to_vec()),
        FixtureCompression::Zlib => (
            [2, 0, 0, 0],
            miniz_oxide::deflate::compress_to_vec_zlib(payload, 6),
        ),
        FixtureCompression::Lzo => ([1, 0, 0, 0], encode_literal_only_lzo(payload)),
    };
    let mut block = Vec::with_capacity(8 + compressed.len());
    block.extend_from_slice(&tag);
    put_u32_be(&mut block, adler32(payload));
    block.extend_from_slice(&compressed);
    block
}

/// Minimal independent LZO1X encoder that emits only literal runs followed by
/// the standard M4 terminator. Compression ratio is irrelevant for fixtures.
fn encode_literal_only_lzo(payload: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(payload.len() + payload.len() / 255 + 5);
    match payload.len() {
        0 => {}
        length @ 1..=238 => output.push(17 + u8::try_from(length).unwrap()),
        length => {
            output.push(0);
            let mut extension = length - 18;
            while extension > 255 {
                output.push(0);
                extension -= 255;
            }
            output.push(u8::try_from(extension).unwrap());
        }
    }
    output.extend_from_slice(payload);
    output.extend_from_slice(&[17, 0, 0]);
    output
}

fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>()
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

/// Independent RFC 1950 ADLER32 implementation used only by synthetic tests.
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
