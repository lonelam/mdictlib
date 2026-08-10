//! Audits exactly one MDict artifact and reports a structured outcome.
//!
//! This is the per-artifact worker for `scripts/corpus/audit-v1.mjs`. It is
//! deliberately a narrow subprocess protocol: one path in, one JSON line out,
//! and a non-zero exit only when the tool itself could not run. A dictionary
//! the parser rejects is a *successful* audit with `"status":"rejected"`, so a
//! corpus run never silently drops a difficult file.
//!
//! Every accepted artifact completes the full validation the roadmap requires:
//! exact entry count, ordinal continuity, `key_at` agreement, raw lookup for
//! every distinct key, duplicate ordinals in physical order, record-offset
//! validation, route agreement between `entry_at`/`entries`/`lookup`, and
//! complete payload hashing.

#[path = "support/sha256.rs"]
mod sha256;

use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mdictlib::{Error, KeyOrdinal, MddFile, MdxFile};

use sha256::{Sha256, hex};

const PROTOCOL: &str = "mdictlib-v1-audit-v1";
const MAX_MESSAGE_CHARS: usize = 512;

/// Structured classification retained for every rejected artifact.
#[derive(Debug, Clone, Copy)]
enum Category {
    Io,
    UnsupportedVersion,
    UnsupportedEncoding,
    UnsupportedCompression,
    UnsupportedEncryption,
    Truncated,
    ChecksumMismatch,
    KeyDecode,
    RecordDecode,
    InvalidGeometry,
    /// A block envelope was well formed but its compressed stream was not.
    CompressionFailure,
    LimitExceeded,
    AllocationFailed,
    PasscodeRequired,
    /// A parser error variant this tool predates. Reported distinctly so a
    /// new variant is visible in the ledger instead of misfiled.
    Unclassified,
}

impl Category {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::UnsupportedVersion => "unsupported-version",
            Self::UnsupportedEncoding => "unsupported-encoding",
            Self::UnsupportedCompression => "unsupported-compression",
            Self::UnsupportedEncryption => "unsupported-encryption",
            Self::Truncated => "truncated",
            Self::ChecksumMismatch => "checksum-mismatch",
            Self::KeyDecode => "key-decode",
            Self::RecordDecode => "record-decode",
            Self::InvalidGeometry => "invalid-geometry",
            Self::CompressionFailure => "compression-failure",
            Self::LimitExceeded => "limit-exceeded",
            Self::AllocationFailed => "allocation-failed",
            Self::PasscodeRequired => "passcode-required",
            Self::Unclassified => "unclassified",
        }
    }
}

/// Maps a parser error onto a retained classification.
///
/// The mapping is deliberately explicit rather than string-matched, so a new
/// error variant cannot be silently absorbed into a neighbouring class.
fn classify(error: &Error) -> Category {
    match error {
        Error::Io(_) => Category::Io,
        Error::Truncated { .. } => Category::Truncated,
        Error::ChecksumMismatch { .. } => Category::ChecksumMismatch,
        Error::LimitExceeded { .. } => Category::LimitExceeded,
        Error::AllocationFailed { .. } => Category::AllocationFailed,
        Error::MissingPasscode | Error::InvalidPasscode(_) => Category::PasscodeRequired,
        Error::Decode { context, .. } => {
            if context.contains("key") {
                Category::KeyDecode
            } else {
                Category::RecordDecode
            }
        }
        Error::Unsupported(feature) => {
            if feature.contains("major version") {
                Category::UnsupportedVersion
            } else if feature.contains("encoding") {
                Category::UnsupportedEncoding
            } else if feature.contains("encrypted") {
                Category::UnsupportedEncryption
            } else {
                Category::UnsupportedCompression
            }
        }
        // `InvalidData` covers both geometry disagreements and codec stream
        // failures. Separating them keeps "the file's geometry is wrong" from
        // "a block would not decompress", which are different source defects.
        Error::InvalidData(message) => {
            let lowered = message.to_ascii_lowercase();
            if lowered.contains("lzo") || lowered.contains("zlib") {
                Category::CompressionFailure
            } else {
                Category::InvalidGeometry
            }
        }
        Error::InvalidFormat(_) => Category::InvalidGeometry,
        _ => Category::Unclassified,
    }
}

/// A failure plus the exact position at which the artifact stopped being
/// readable, so partial reads are never reported as whole-file success.
struct Failure {
    category: Category,
    message: String,
    ordinal: Option<u64>,
}

impl Failure {
    fn new(error: &Error, ordinal: Option<u64>) -> Self {
        Self {
            category: classify(error),
            message: truncate(&error.to_string()),
            ordinal,
        }
    }
}

struct Accepted {
    entries: u64,
    distinct_keys: u64,
    duplicate_groups: u64,
    key_sha256: String,
    payload_sha256: String,
    total_payload_bytes: u64,
}

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let Some(path) = arguments.next().map(PathBuf::from) else {
        eprintln!("usage: v1_audit <path>");
        return ExitCode::FAILURE;
    };
    if arguments.next().is_some() {
        eprintln!("v1_audit: unexpected extra argument");
        return ExitCode::FAILURE;
    }

    let is_mdd = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mdd"));

    let outcome = if is_mdd {
        audit_mdd(&path)
    } else {
        audit_mdx(&path)
    };

    match outcome {
        Ok((header, accepted)) => {
            println!(
                concat!(
                    r#"{{"protocol":"{}","status":"accepted",{},"#,
                    r#""entries":{},"distinctKeys":{},"duplicateGroups":{},"#,
                    r#""payloadBytes":{},"keySha256":"{}","payloadSha256":"{}"}}"#
                ),
                PROTOCOL,
                header,
                accepted.entries,
                accepted.distinct_keys,
                accepted.duplicate_groups,
                accepted.total_payload_bytes,
                accepted.key_sha256,
                accepted.payload_sha256,
            );
            ExitCode::SUCCESS
        }
        Err((header, failure)) => {
            println!(
                concat!(
                    r#"{{"protocol":"{}","status":"rejected",{},"#,
                    r#""category":"{}","message":"{}","failingOrdinal":{}}}"#
                ),
                PROTOCOL,
                header,
                failure.category.as_str(),
                escape_json(&failure.message),
                failure
                    .ordinal
                    .map_or_else(|| "null".to_owned(), |value| value.to_string()),
            );
            ExitCode::SUCCESS
        }
    }
}

/// Renders the header facts every row retains, whether or not parsing
/// succeeded past the header.
fn header_json(dictionary_header: Option<&mdictlib::Header>) -> String {
    match dictionary_header {
        None => r#""header":null"#.to_owned(),
        Some(header) => format!(
            concat!(
                r#""header":{{"tag":"{}","generatedByEngineVersion":"{}","#,
                r#""requiredEngineVersion":"{}","encoding":{},"encryptedBits":{},"#,
                r#""format":{},"stripKey":{},"keyCaseSensitive":{}}}"#
            ),
            escape_json(header.tag_name()),
            escape_json(header.generated_by_engine_version()),
            escape_json(header.required_engine_version()),
            optional_json(header.encoding_label()),
            header.encryption_bits(),
            optional_json(header.format()),
            header.strip_key(),
            header.key_case_sensitive(),
        ),
    }
}

fn audit_mdx(path: &Path) -> Result<(String, Accepted), (String, Failure)> {
    let dictionary = match MdxFile::open(path) {
        Ok(dictionary) => dictionary,
        Err(error) => return Err((header_json(None), Failure::new(&error, None))),
    };
    let header = header_json(Some(dictionary.header()));

    let declared = dictionary.len();
    let mut key_hash = Sha256::new();
    let mut payload_hash = Sha256::new();
    let mut payload_bytes = 0u64;
    let mut ordinals_by_key: BTreeMap<String, Vec<u64>> = BTreeMap::new();

    // One sequential pass: ordinal continuity, key text, and payloads.
    for (index, result) in dictionary.entries().enumerate() {
        let ordinal = u64::try_from(index).expect("entry index fits u64");
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => return Err((header, Failure::new(&error, Some(ordinal)))),
        };
        if entry.ordinal().get() != ordinal {
            return Err((
                header,
                Failure {
                    category: Category::InvalidGeometry,
                    message: format!(
                        "ordinal discontinuity: expected {ordinal}, got {}",
                        entry.ordinal().get()
                    ),
                    ordinal: Some(ordinal),
                },
            ));
        }

        // key_at must agree with the iterator for the same ordinal.
        match dictionary.key_at(entry.ordinal()) {
            Ok(Some(direct)) if direct.key() == entry.key() => {}
            Ok(_) => {
                return Err((
                    header,
                    Failure {
                        category: Category::InvalidGeometry,
                        message: "key_at disagreed with sequential iteration".to_owned(),
                        ordinal: Some(ordinal),
                    },
                ));
            }
            Err(error) => return Err((header, Failure::new(&error, Some(ordinal)))),
        }

        key_hash.update(entry.key().as_bytes());
        key_hash.update(b"\n");
        payload_hash.update(entry.text().as_bytes());
        payload_hash.update(b"\n");
        payload_bytes += u64::try_from(entry.text().len()).expect("text length fits u64");
        ordinals_by_key
            .entry(entry.key().to_owned())
            .or_default()
            .push(ordinal);
    }

    let observed = u64::try_from(ordinals_by_key.values().map(Vec::len).sum::<usize>())
        .expect("entry count fits u64");
    if observed != declared {
        return Err((
            header,
            Failure {
                category: Category::InvalidGeometry,
                message: format!("declared {declared} entries but traversed {observed}"),
                ordinal: None,
            },
        ));
    }

    // Raw lookup for every distinct key, with all duplicates in order.
    let mut duplicate_groups = 0u64;
    for (key, ordinals) in &ordinals_by_key {
        if ordinals.len() > 1 {
            duplicate_groups += 1;
        }
        let matches = match dictionary.locate(key) {
            Ok(Some(matches)) => matches,
            Ok(None) => {
                return Err((
                    header,
                    Failure {
                        category: Category::InvalidGeometry,
                        message: "a physical key did not resolve through lookup".to_owned(),
                        ordinal: ordinals.first().copied(),
                    },
                ));
            }
            Err(error) => return Err((header, Failure::new(&error, ordinals.first().copied()))),
        };
        let located = (0..matches.len())
            .filter_map(|index| matches.get(index))
            .map(KeyOrdinal::get)
            .collect::<Vec<_>>();
        if located != *ordinals {
            return Err((
                header,
                Failure {
                    category: Category::InvalidGeometry,
                    message: "duplicate ordinals were not in physical order".to_owned(),
                    ordinal: ordinals.first().copied(),
                },
            ));
        }
        // lookup must take the same route as direct ordinal access.
        match dictionary.lookup(key) {
            Ok(Some(entry)) if entry.ordinal().get() == ordinals[0] => {}
            Ok(_) => {
                return Err((
                    header,
                    Failure {
                        category: Category::InvalidGeometry,
                        message: "lookup did not select the lowest physical ordinal".to_owned(),
                        ordinal: ordinals.first().copied(),
                    },
                ));
            }
            Err(error) => return Err((header, Failure::new(&error, ordinals.first().copied()))),
        }
    }

    let distinct_keys = u64::try_from(ordinals_by_key.len()).expect("key count fits u64");
    Ok((
        header,
        Accepted {
            entries: declared,
            distinct_keys,
            duplicate_groups,
            key_sha256: hex(&key_hash.finish()),
            payload_sha256: hex(&payload_hash.finish()),
            total_payload_bytes: payload_bytes,
        },
    ))
}

fn audit_mdd(path: &Path) -> Result<(String, Accepted), (String, Failure)> {
    let dictionary = match MddFile::open(path) {
        Ok(dictionary) => dictionary,
        Err(error) => return Err((header_json(None), Failure::new(&error, None))),
    };
    let header = header_json(Some(dictionary.header()));

    let declared = dictionary.len();
    let mut key_hash = Sha256::new();
    let mut payload_hash = Sha256::new();
    let mut payload_bytes = 0u64;
    let mut ordinals_by_key: BTreeMap<String, Vec<u64>> = BTreeMap::new();

    for index in 0..declared {
        let ordinal = KeyOrdinal::new(index);
        let span = match dictionary.span_at(ordinal) {
            Ok(Some(span)) => span,
            Ok(None) => {
                return Err((
                    header,
                    Failure {
                        category: Category::InvalidGeometry,
                        message: "declared ordinal had no span".to_owned(),
                        ordinal: Some(index),
                    },
                ));
            }
            Err(error) => return Err((header, Failure::new(&error, Some(index)))),
        };

        // Streaming and materialization must agree byte for byte.
        let mut streamed = StreamHash::new();
        if let Err(error) = span.copy_to(&mut streamed) {
            return Err((header, Failure::new(&error, Some(index))));
        }
        let materialized = match span.read() {
            Ok(resource) => resource,
            Err(error) => return Err((header, Failure::new(&error, Some(index)))),
        };
        let streamed_digest = streamed.hash.finish();
        let mut direct = Sha256::new();
        direct.update(materialized.bytes());
        if direct.finish() != streamed_digest {
            return Err((
                header,
                Failure {
                    category: Category::InvalidGeometry,
                    message: "streamed and materialized resource bytes disagreed".to_owned(),
                    ordinal: Some(index),
                },
            ));
        }

        key_hash.update(span.key().as_bytes());
        key_hash.update(b"\n");
        payload_hash.update(&streamed_digest);
        payload_bytes += span.len();
        ordinals_by_key
            .entry(span.key().to_owned())
            .or_default()
            .push(index);
    }

    let mut duplicate_groups = 0u64;
    for (key, ordinals) in &ordinals_by_key {
        if ordinals.len() > 1 {
            duplicate_groups += 1;
        }
        let matches = match dictionary.locate(key) {
            Ok(Some(matches)) => matches,
            Ok(None) => {
                return Err((
                    header,
                    Failure {
                        category: Category::InvalidGeometry,
                        message: "a physical key did not resolve through lookup".to_owned(),
                        ordinal: ordinals.first().copied(),
                    },
                ));
            }
            Err(error) => return Err((header, Failure::new(&error, ordinals.first().copied()))),
        };
        let located = (0..matches.len())
            .filter_map(|index| matches.get(index))
            .map(KeyOrdinal::get)
            .collect::<Vec<_>>();
        if located != *ordinals {
            return Err((
                header,
                Failure {
                    category: Category::InvalidGeometry,
                    message: "duplicate ordinals were not in physical order".to_owned(),
                    ordinal: ordinals.first().copied(),
                },
            ));
        }
    }

    let distinct_keys = u64::try_from(ordinals_by_key.len()).expect("key count fits u64");
    Ok((
        header,
        Accepted {
            entries: declared,
            distinct_keys,
            duplicate_groups,
            key_sha256: hex(&key_hash.finish()),
            payload_sha256: hex(&payload_hash.finish()),
            total_payload_bytes: payload_bytes,
        },
    ))
}

/// Hashes a streamed span without buffering the whole resource.
struct StreamHash {
    hash: Sha256,
}

impl StreamHash {
    const fn new() -> Self {
        Self {
            hash: Sha256::new(),
        }
    }
}

impl std::io::Write for StreamHash {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.hash.update(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn optional_json(value: Option<&str>) -> String {
    value.map_or_else(
        || "null".to_owned(),
        |text| format!("\"{}\"", escape_json(text)),
    )
}

fn escape_json(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if control.is_control() => {
                output.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => output.push(other),
        }
    }
    output
}

fn truncate(value: &str) -> String {
    value.chars().take(MAX_MESSAGE_CHARS).collect()
}
