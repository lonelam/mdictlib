//! Independent, test-only encoder for the MDict **version 1** layout.
//!
//! This module is physically separate from the version 2 encoder in
//! [`super`] and shares only primitive text/checksum/codec helpers. It never
//! calls the library's parser, index, checksum, or block-codec code, so a
//! fixture that the reader accepts is evidence about the wire format rather
//! than a tautology about `mdictlib` agreeing with itself.
//!
//! The layout it writes was derived from bounded observation of authorized
//! real v1.2 artifacts:
//!
//! ```text
//! header          u32-BE xml length, UTF-16LE XML, u32-LE ADLER32
//! keyword header  four u32-BE fields; no decoded-size field, no checksum
//! keyword index   RAW (never compressed):
//!                   u32-BE entry count
//!                   u8 first-summary length (encoding units) + bytes
//!                   u8 last-summary length  (encoding units) + bytes
//!                   u32-BE compressed size, u32-BE decompressed size
//! key blocks      tag(4) + ADLER32-BE(4) + payload
//!                   rows: u32-BE record offset + NUL-terminated key
//! record header   four u32-BE fields
//! record index    u32-BE compressed size, u32-BE decompressed size per block
//! record blocks   tag(4) + ADLER32-BE(4) + payload
//! ```
//!
//! Version 1 summaries carry no terminator, unlike version 2.

#![allow(dead_code)]

use std::ops::Range;

use super::{
    FixtureCompression, FixtureEncoding, FixtureKind, TempDictionary, adler32, encode_block,
    encode_text, escape_xml, put_u32_be, utf16le,
};

/// How a v1 fixture encodes its key and record blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V1BlockCoding {
    /// Stored uncompressed behind the shared eight-byte envelope.
    None,
    /// LZO1X literal runs only.
    Lzo,
    /// LZO1X with real lookbehind matches, exercising the decoder's copy path.
    LzoBackReference,
    /// zlib. No authorized real v1 artifact was observed using this; it exists
    /// so tests can pin what the shared envelope does rather than to claim
    /// creator compatibility.
    Zlib,
}

#[derive(Debug, Clone)]
struct V1Entry {
    key: String,
    record: Vec<u8>,
}

/// Independent builder for whole version 1 MDX and MDD files.
#[derive(Debug, Clone)]
pub struct V1FixtureBuilder {
    kind: FixtureKind,
    encoding: FixtureEncoding,
    encoding_label: Option<String>,
    generated_engine_version: String,
    required_engine_version: String,
    entries: Vec<V1Entry>,
    key_block_counts: Vec<usize>,
    record_block_sizes: Option<Vec<usize>>,
    record_starts: Option<Vec<u64>>,
    key_summaries: Option<Vec<(String, String)>>,
    summary_length_overrides: Option<Vec<(u8, u8)>>,
    key_case_attribute: (String, String),
    strip_key_attribute: (String, String),
    extra_header_attributes: Vec<(String, String)>,
    key_info_trailing_bytes: Vec<u8>,
    record_index_trailing_bytes: Vec<u8>,
    key_block_coding: V1BlockCoding,
    record_block_coding: V1BlockCoding,
    encryption_bits: Option<u8>,
}

impl V1FixtureBuilder {
    /// Builds a version 1 MDX fixture from key/text pairs.
    pub fn mdx(entries: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>) -> Self {
        Self::new(
            FixtureKind::Mdx,
            entries
                .into_iter()
                .map(|(key, text)| V1Entry {
                    key: key.into(),
                    record: text.into().into_bytes(),
                })
                .collect(),
        )
    }

    /// Builds a version 1 MDD fixture from key/bytes pairs.
    pub fn mdd(entries: impl IntoIterator<Item = (impl Into<String>, Vec<u8>)>) -> Self {
        Self::new(
            FixtureKind::Mdd,
            entries
                .into_iter()
                .map(|(key, record)| V1Entry {
                    key: key.into(),
                    record,
                })
                .collect(),
        )
    }

    fn new(kind: FixtureKind, entries: Vec<V1Entry>) -> Self {
        let entry_count = entries.len();
        Self {
            kind,
            encoding: match kind {
                FixtureKind::Mdx => FixtureEncoding::Utf8,
                FixtureKind::Mdd => FixtureEncoding::Utf16Le,
            },
            encoding_label: None,
            generated_engine_version: "1.2".to_owned(),
            required_engine_version: "1.2".to_owned(),
            entries,
            key_block_counts: if entry_count == 0 {
                Vec::new()
            } else {
                vec![entry_count]
            },
            record_block_sizes: None,
            record_starts: None,
            key_summaries: None,
            summary_length_overrides: None,
            key_case_attribute: ("KeyCaseSensitive".to_owned(), "No".to_owned()),
            strip_key_attribute: ("StripKey".to_owned(), "No".to_owned()),
            extra_header_attributes: Vec::new(),
            key_info_trailing_bytes: Vec::new(),
            record_index_trailing_bytes: Vec::new(),
            key_block_coding: V1BlockCoding::None,
            record_block_coding: V1BlockCoding::None,
            encryption_bits: None,
        }
    }

    pub fn encoding(mut self, encoding: FixtureEncoding) -> Self {
        assert_eq!(self.kind, FixtureKind::Mdx, "MDD keys are always UTF-16LE");
        self.encoding = encoding;
        self
    }

    pub fn encoding_label(mut self, label: impl Into<String>) -> Self {
        assert_eq!(self.kind, FixtureKind::Mdx, "MDD keys are always UTF-16LE");
        self.encoding_label = Some(label.into());
        self
    }

    pub fn engine_versions(
        mut self,
        generated: impl Into<String>,
        required: impl Into<String>,
    ) -> Self {
        self.generated_engine_version = generated.into();
        self.required_engine_version = required.into();
        self
    }

    pub fn coding(mut self, coding: V1BlockCoding) -> Self {
        self.key_block_coding = coding;
        self.record_block_coding = coding;
        self
    }

    pub fn mixed_coding(mut self, key_blocks: V1BlockCoding, record_blocks: V1BlockCoding) -> Self {
        self.key_block_coding = key_blocks;
        self.record_block_coding = record_blocks;
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

    /// Overrides the declared one-byte summary lengths without changing the
    /// summary bytes, so malformed-length handling can be exercised.
    pub fn summary_length_overrides(mut self, lengths: impl Into<Vec<(u8, u8)>>) -> Self {
        self.summary_length_overrides = Some(lengths.into());
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

    /// Declares encryption bits in the header without encrypting anything.
    ///
    /// No authorized real v1 artifact declares encryption, so the reader must
    /// refuse rather than guess at a framing.
    pub fn declare_encryption(mut self, bits: u8) -> Self {
        self.encryption_bits = Some(bits);
        self
    }

    pub fn key_info_trailing_bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.key_info_trailing_bytes = bytes.into();
        self
    }

    pub fn record_index_trailing_bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.record_index_trailing_bytes = bytes.into();
        self
    }

    pub fn build(self) -> V1Fixture {
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
        let record_starts = match self.record_starts.clone() {
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
                        offset += u64::try_from(record.len()).unwrap();
                        start
                    })
                    .collect()
            }
        };

        let record_block_payloads = split_blocks(
            &record_data,
            self.record_block_sizes.clone().unwrap_or_else(|| {
                if record_data.is_empty() {
                    Vec::new()
                } else {
                    vec![record_data.len()]
                }
            }),
        );

        let encoded_keys = self
            .entries
            .iter()
            .map(|entry| encode_text(self.encoding, &entry.key))
            .collect::<Vec<_>>();

        // Key rows: u32-BE record offset, then a terminated key.
        let mut entry_cursor = 0usize;
        let mut key_block_payloads = Vec::with_capacity(self.key_block_counts.len());
        let mut default_summaries = Vec::with_capacity(self.key_block_counts.len());
        for &entry_count in &self.key_block_counts {
            let end = entry_cursor + entry_count;
            let mut payload = Vec::new();
            for entry_index in entry_cursor..end {
                put_u32_be(
                    &mut payload,
                    u32::try_from(record_starts[entry_index]).unwrap(),
                );
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

        let summaries = self.key_summaries.clone().unwrap_or(default_summaries);
        assert_eq!(summaries.len(), self.key_block_counts.len());

        let key_blocks = key_block_payloads
            .iter()
            .map(|payload| encode_v1_block(payload, self.key_block_coding))
            .collect::<Vec<_>>();
        let record_blocks = record_block_payloads
            .iter()
            .map(|payload| encode_v1_block(payload, self.record_block_coding))
            .collect::<Vec<_>>();

        // Keyword index metadata, written raw.
        let mut key_info = Vec::new();
        for (index, (((&entry_count, (first, last)), payload), block)) in self
            .key_block_counts
            .iter()
            .zip(summaries.iter())
            .zip(key_block_payloads.iter())
            .zip(key_blocks.iter())
            .enumerate()
        {
            put_u32_be(&mut key_info, u32::try_from(entry_count).unwrap());
            let overrides = self
                .summary_length_overrides
                .as_ref()
                .and_then(|values| values.get(index).copied());
            put_v1_summary(&mut key_info, self.encoding, first, overrides.map(|o| o.0));
            put_v1_summary(&mut key_info, self.encoding, last, overrides.map(|o| o.1));
            put_u32_be(&mut key_info, u32::try_from(block.len()).unwrap());
            put_u32_be(&mut key_info, u32::try_from(payload.len()).unwrap());
        }
        key_info.extend_from_slice(&self.key_info_trailing_bytes);

        let header_xml = self.header_xml();
        let header_xml_bytes = utf16le(&header_xml);

        let mut bytes = Vec::new();
        put_u32_be(&mut bytes, u32::try_from(header_xml_bytes.len()).unwrap());
        bytes.extend_from_slice(&header_xml_bytes);
        let header_checksum_offset = bytes.len();
        bytes.extend_from_slice(&adler32(&header_xml_bytes).to_le_bytes());

        let keyword_header_offset = bytes.len();
        let key_blocks_len = key_blocks.iter().map(Vec::len).sum::<usize>();
        put_u32_be(
            &mut bytes,
            u32::try_from(self.key_block_counts.len()).unwrap(),
        );
        put_u32_be(&mut bytes, u32::try_from(self.entries.len()).unwrap());
        put_u32_be(&mut bytes, u32::try_from(key_info.len()).unwrap());
        put_u32_be(&mut bytes, u32::try_from(key_blocks_len).unwrap());

        let key_info_start = bytes.len();
        bytes.extend_from_slice(&key_info);
        let key_info_range = key_info_start..bytes.len();

        let mut key_block_ranges = Vec::with_capacity(key_blocks.len());
        for block in &key_blocks {
            let start = bytes.len();
            bytes.extend_from_slice(block);
            key_block_ranges.push(start..bytes.len());
        }

        let record_header_offset = bytes.len();
        let record_index_len = record_blocks.len() * 8 + self.record_index_trailing_bytes.len();
        let record_blocks_len = record_blocks.iter().map(Vec::len).sum::<usize>();
        put_u32_be(&mut bytes, u32::try_from(record_blocks.len()).unwrap());
        put_u32_be(&mut bytes, u32::try_from(self.entries.len()).unwrap());
        put_u32_be(&mut bytes, u32::try_from(record_index_len).unwrap());
        put_u32_be(&mut bytes, u32::try_from(record_blocks_len).unwrap());

        let record_index_start = bytes.len();
        for (payload, block) in record_block_payloads.iter().zip(record_blocks.iter()) {
            put_u32_be(&mut bytes, u32::try_from(block.len()).unwrap());
            put_u32_be(&mut bytes, u32::try_from(payload.len()).unwrap());
        }
        bytes.extend_from_slice(&self.record_index_trailing_bytes);
        let record_index_range = record_index_start..bytes.len();

        let mut record_block_ranges = Vec::with_capacity(record_blocks.len());
        for block in &record_blocks {
            let start = bytes.len();
            bytes.extend_from_slice(block);
            record_block_ranges.push(start..bytes.len());
        }

        V1Fixture {
            kind: self.kind,
            bytes,
            layout: V1FixtureLayout {
                header_checksum_offset,
                keyword_header_offset,
                key_info: key_info_range,
                key_blocks: key_block_ranges,
                record_header_offset,
                record_index: record_index_range,
                record_blocks: record_block_ranges,
            },
        }
    }

    fn header_xml(&self) -> String {
        let tag = match self.kind {
            FixtureKind::Mdx => "Dictionary",
            FixtureKind::Mdd => "Library_Data",
        };
        let mut xml = format!(
            "<{tag} GeneratedByEngineVersion=\"{}\" RequiredEngineVersion=\"{}\"",
            self.generated_engine_version, self.required_engine_version
        );
        if self.kind == FixtureKind::Mdx {
            let label = self
                .encoding_label
                .clone()
                .unwrap_or_else(|| encoding_label(self.encoding).to_owned());
            xml.push_str(&format!(" Encoding=\"{label}\""));
        }
        if let Some(bits) = self.encryption_bits {
            xml.push_str(&format!(" Encrypted=\"{bits}\""));
        }
        xml.push_str(&format!(
            " Format=\"Html\" {}=\"{}\" {}=\"{}\"",
            self.key_case_attribute.0,
            escape_xml(&self.key_case_attribute.1),
            self.strip_key_attribute.0,
            escape_xml(&self.strip_key_attribute.1),
        ));
        for (name, value) in &self.extra_header_attributes {
            xml.push_str(&format!(" {name}=\"{}\"", escape_xml(value)));
        }
        xml.push_str("/>");
        xml
    }
}

fn encoding_label(encoding: FixtureEncoding) -> &'static str {
    match encoding {
        FixtureEncoding::Utf8 => "UTF-8",
        FixtureEncoding::Utf16Le => "UTF-16",
        FixtureEncoding::Gbk => "GBK",
        FixtureEncoding::Gb18030 => "GB18030",
        FixtureEncoding::Big5 => "BIG5",
    }
}

/// Byte ranges inside a built v1 fixture, for targeted corruption.
#[derive(Debug, Clone)]
pub struct V1FixtureLayout {
    pub header_checksum_offset: usize,
    pub keyword_header_offset: usize,
    pub key_info: Range<usize>,
    pub key_blocks: Vec<Range<usize>>,
    pub record_header_offset: usize,
    pub record_index: Range<usize>,
    pub record_blocks: Vec<Range<usize>>,
}

/// A complete version 1 dictionary file held in memory.
#[derive(Debug, Clone)]
pub struct V1Fixture {
    pub kind: FixtureKind,
    pub bytes: Vec<u8>,
    pub layout: V1FixtureLayout,
}

impl V1Fixture {
    pub fn write(&self, name: &str) -> TempDictionary {
        TempDictionary::write(name, self.kind, &self.bytes)
    }

    pub fn write_truncated(&self, name: &str, keep: usize) -> TempDictionary {
        TempDictionary::write(name, self.kind, &self.bytes[..keep])
    }

    /// Overwrites one of the four u32 keyword-header fields.
    ///
    /// Version 1 has no keyword-header checksum, so nothing needs refreshing.
    pub fn set_keyword_u32(&mut self, field_index: usize, value: u32) {
        assert!(field_index < 4);
        let offset = self.layout.keyword_header_offset + field_index * 4;
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }

    /// Overwrites one of the four u32 record-header fields.
    pub fn set_record_u32(&mut self, field_index: usize, value: u32) {
        assert!(field_index < 4);
        let offset = self.layout.record_header_offset + field_index * 4;
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }

    pub fn corrupt_block_checksum(&mut self, range: &Range<usize>) {
        assert!(range.end - range.start >= 8);
        self.bytes[range.start + 4] ^= 0x80;
    }

    pub fn corrupt_block_tag(&mut self, range: &Range<usize>, tag: [u8; 4]) {
        self.bytes[range.start..range.start + 4].copy_from_slice(&tag);
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

    /// Overwrites a big-endian u32 inside the raw keyword metadata.
    pub fn set_key_info_u32(&mut self, byte_offset: usize, value: u32) {
        let start = self.layout.key_info.start + byte_offset;
        self.bytes[start..start + 4].copy_from_slice(&value.to_be_bytes());
    }

    /// Overwrites a big-endian u32 inside the record index.
    pub fn set_record_index_u32(&mut self, byte_offset: usize, value: u32) {
        let start = self.layout.record_index.start + byte_offset;
        self.bytes[start..start + 4].copy_from_slice(&value.to_be_bytes());
    }
}

fn split_blocks(data: &[u8], sizes: Vec<usize>) -> Vec<Vec<u8>> {
    assert_eq!(
        sizes.iter().sum::<usize>(),
        data.len(),
        "block sizes must cover the concatenated bytes"
    );
    assert!(
        sizes.iter().all(|size| *size > 0),
        "fixture blocks must not be empty"
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

fn put_v1_summary(
    output: &mut Vec<u8>,
    encoding: FixtureEncoding,
    summary: &str,
    declared_units: Option<u8>,
) {
    let encoded = encode_text(encoding, summary);
    let units = encoded.len() / encoding.unit_size();
    output.push(declared_units.unwrap_or_else(|| u8::try_from(units).unwrap()));
    output.extend_from_slice(&encoded);
    // Version 1 summaries carry no terminator.
}

fn encode_v1_block(payload: &[u8], coding: V1BlockCoding) -> Vec<u8> {
    match coding {
        V1BlockCoding::None => encode_block(payload, FixtureCompression::None),
        V1BlockCoding::Lzo => encode_block(payload, FixtureCompression::Lzo),
        V1BlockCoding::Zlib => encode_block(payload, FixtureCompression::Zlib),
        V1BlockCoding::LzoBackReference => {
            let mut block = Vec::new();
            block.extend_from_slice(&[1, 0, 0, 0]);
            put_u32_be(&mut block, adler32(payload));
            block.extend_from_slice(&encode_lzo_with_back_references(payload));
            block
        }
    }
}

/// Encodes a payload as an LZO1X stream that uses real lookbehind matches.
///
/// The encoder is deliberately tiny: it emits one initial literal run and then
/// M2 match operations for any three-byte sequence that repeats at a distance
/// the M2 form can express. That is enough to drive the decoder's copy path,
/// which literal-only streams never reach.
///
/// Panics if the payload cannot be encoded this way, so a fixture can never
/// silently degrade into a literal-only stream.
fn encode_lzo_with_back_references(payload: &[u8]) -> Vec<u8> {
    const MIN_PREFIX: usize = 4;
    const MATCH_LEN: usize = 3;
    assert!(
        payload.len() > MIN_PREFIX,
        "back-reference fixtures need a payload longer than the literal prefix"
    );

    // Find the longest prefix after which the remainder is exactly a sequence
    // of three-byte matches at a fixed, M2-expressible distance.
    let mut prefix_len = None;
    for candidate in MIN_PREFIX..payload.len() {
        let remainder = payload.len() - candidate;
        if remainder == 0 || !remainder.is_multiple_of(MATCH_LEN) {
            continue;
        }
        let distance = MATCH_LEN;
        if candidate < distance {
            continue;
        }
        let matches_all =
            (candidate..payload.len()).all(|index| payload[index] == payload[index - distance]);
        if matches_all {
            prefix_len = Some(candidate);
            break;
        }
    }
    let prefix_len = prefix_len.expect(
        "back-reference fixture payload must end with three-byte repeats of the preceding bytes",
    );

    let mut output = Vec::new();
    // Initial literal run: a first byte above 17 means "copy (byte - 17)
    // literals" before the match loop begins.
    assert!(
        prefix_len <= 238,
        "back-reference fixture literal prefix must fit one LZO length byte"
    );
    output.push(17 + u8::try_from(prefix_len).unwrap());
    output.extend_from_slice(&payload[..prefix_len]);

    // M2 match: length 3 or 4, distance 1..=2048, no trailing literals.
    let distance = MATCH_LEN;
    let match_count = (payload.len() - prefix_len) / MATCH_LEN;
    for _ in 0..match_count {
        let length_bits = u8::try_from(MATCH_LEN - 3).unwrap() << 5;
        let low_distance = u8::try_from((distance - 1) & 0b111).unwrap() << 2;
        output.push(0b0100_0000 | length_bits | low_distance);
        output.push(u8::try_from((distance - 1) >> 3).unwrap());
    }

    // Canonical LZO1X end-of-stream marker.
    output.extend_from_slice(&[17, 0, 0]);
    output
}

/// Builds a payload whose tail is `repeats` copies of `prefix`'s last three
/// bytes, which is exactly the shape [`encode_lzo_with_back_references`] can
/// express as lookbehind matches.
pub fn repeating_payload(prefix: &[u8], repeats: usize) -> Vec<u8> {
    assert!(prefix.len() >= 4, "prefix must be at least four bytes");
    let mut payload = prefix.to_vec();
    let tail = prefix[prefix.len() - 3..].to_vec();
    for _ in 0..repeats {
        payload.extend_from_slice(&tail);
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent LZO1X reader used only to prove the fixture encoder emits a
    /// stream with real lookbehind copies. It understands exactly the two
    /// operations the encoder produces.
    fn decode_encoder_output(stream: &[u8]) -> (Vec<u8>, usize) {
        let mut output = Vec::new();
        let mut matches = 0usize;
        let mut index = 0usize;
        assert!(stream[index] > 17, "expected an initial literal run");
        let literal_len = usize::from(stream[index] - 17);
        index += 1;
        output.extend_from_slice(&stream[index..index + literal_len]);
        index += literal_len;

        loop {
            let op = stream[index];
            if op == 17 && stream[index + 1] == 0 && stream[index + 2] == 0 {
                break;
            }
            assert!(
                (0b0100_0000..0b1000_0000).contains(&op),
                "unexpected opcode"
            );
            let length = 3 + usize::from((op >> 5) & 1);
            let distance = (usize::from(stream[index + 1]) << 3) + usize::from((op >> 2) & 7) + 1;
            index += 2;
            let start = output.len() - distance;
            for offset in 0..length {
                output.push(output[start + offset]);
            }
            matches += 1;
        }
        (output, matches)
    }

    #[test]
    fn back_reference_encoder_emits_real_lookbehind_copies() {
        let payload = repeating_payload(b"abcd", 3);
        let stream = encode_lzo_with_back_references(&payload);
        let (decoded, matches) = decode_encoder_output(&stream);
        assert_eq!(decoded, payload);
        assert!(matches > 0, "stream must contain lookbehind matches");
        assert!(
            stream.len() < payload.len() + 8,
            "matches must actually shorten the stream"
        );
    }
}
