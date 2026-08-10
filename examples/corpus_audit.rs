//! Audits exactly one locked MDX or MDD artifact for the repository corpus tooling.
//!
//! This example is intentionally a narrow subprocess protocol. Use
//! `scripts/corpus/validate.mjs` rather than invoking it directly for a lock.

#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod fixture_support;
#[path = "support/sha256.rs"]
mod sha256;

use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mdictlib::{KeyEntry, KeyMatches, KeyOrdinal, MatchBasis, MddFile, MdxFile};

use sha256::{Sha256, hex};

const PROTOCOL: &str = "mdictlib-corpus-audit-v1";
const IDENTITY_PROTOCOL: &str = "mdictlib-corpus-audit-identity-v1";
const TOOL: &str = "mdictlib corpus_audit";
const MAX_DIAGNOSTIC_CHARS: usize = 2_048;

#[derive(Clone, Copy)]
enum Kind {
    Mdx,
    Mdd,
}

impl Kind {
    fn parse(value: &OsString) -> Result<Self, String> {
        match value.to_str() {
            Some("mdx") => Ok(Self::Mdx),
            Some("mdd") => Ok(Self::Mdd),
            _ => Err("kind must be `mdx` or `mdd`".to_owned()),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Mdx => "mdx",
            Self::Mdd => "mdd",
        }
    }
}

struct Audit {
    entries: u64,
    key_sha256: [u8; 32],
    payload_sha256: [u8; 32],
}

fn main() -> ExitCode {
    if identity_requested() {
        println!(
            "{IDENTITY_PROTOCOL}\t{PROTOCOL}\t{TOOL}\t{}",
            env!("CARGO_PKG_VERSION")
        );
        return ExitCode::SUCCESS;
    }
    match run() {
        Ok((kind, audit)) => {
            println!(
                "{PROTOCOL}\t{}\t{}\t{}\t{}",
                kind.as_str(),
                audit.entries,
                hex(&audit.key_sha256),
                hex(&audit.payload_sha256)
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("corpus-audit: {}", sanitize_diagnostic(&error));
            ExitCode::FAILURE
        }
    }
}

fn identity_requested() -> bool {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    arguments.next().as_deref() == Some(std::ffi::OsStr::new("--identity"))
        && arguments.next().is_none()
}

fn run() -> Result<(Kind, Audit), String> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let kind =
        Kind::parse(&arguments.next().ok_or_else(|| {
            "usage: corpus_audit <mdx|mdd> <path> <expected-entries>".to_owned()
        })?)?;
    let path = PathBuf::from(
        arguments
            .next()
            .ok_or_else(|| "usage: corpus_audit <mdx|mdd> <path> <expected-entries>".to_owned())?,
    );
    let expected = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| "expected-entries must be an unsigned decimal integer".to_owned())?
        .parse::<u64>()
        .map_err(|_| "expected-entries must be an unsigned decimal integer".to_owned())?;
    if arguments.next().is_some() {
        return Err("unexpected extra argument".to_owned());
    }

    let audit = match kind {
        Kind::Mdx => audit_mdx(&path, expected)?,
        Kind::Mdd => audit_mdd(&path, expected)?,
    };
    Ok((kind, audit))
}

fn audit_mdx(path: &Path, expected: u64) -> Result<Audit, String> {
    let dictionary = MdxFile::open(path).map_err(|error| format!("open failed: {error}"))?;
    if dictionary.len() != expected {
        return Err(format!(
            "declared entry count is {}; lock expects {expected}",
            dictionary.len()
        ));
    }

    let mut key_digest = Sha256::new();
    let mut payload_digest = Sha256::new();
    let mut count = 0u64;
    for result in dictionary.keys() {
        let key =
            result.map_err(|error| format!("key iteration failed at ordinal {count}: {error}"))?;
        require_contiguous(&key, count)?;
        digest_key(&mut key_digest, &key);

        let ordinal_key = dictionary
            .key_at(key.ordinal())
            .map_err(|error| format!("key_at failed at ordinal {count}: {error}"))?
            .ok_or_else(|| format!("key_at returned no row at ordinal {count}"))?;
        if ordinal_key != key {
            return Err(format!("key_at disagreed at ordinal {count}"));
        }

        let entry = dictionary
            .entry_at(key.ordinal())
            .map_err(|error| format!("entry_at failed at ordinal {count}: {error}"))?
            .ok_or_else(|| format!("entry_at returned no row at ordinal {count}"))?;
        if entry.key_entry() != &key {
            return Err(format!("entry_at key disagreed at ordinal {count}"));
        }
        digest_key(&mut payload_digest, entry.key_entry());
        digest_bytes(&mut payload_digest, entry.text().as_bytes())?;

        let matches = dictionary
            .locate(key.key())
            .map_err(|error| format!("raw locate failed at ordinal {count}: {error}"))?
            .ok_or_else(|| format!("raw locate returned no matches at ordinal {count}"))?;
        let is_group_first = validate_raw_match_group_if_first(&key, &matches, count, |ordinal| {
            dictionary.key_at(ordinal)
        })?;
        if is_group_first {
            let lookup = dictionary
                .lookup(key.key())
                .map_err(|error| format!("raw lookup failed at ordinal {count}: {error}"))?
                .ok_or_else(|| format!("raw lookup returned no entry at ordinal {count}"))?;
            if lookup != entry {
                return Err(format!("raw lookup disagreed at ordinal {count}"));
            }
        }
        count = count
            .checked_add(1)
            .ok_or_else(|| "physical entry count overflowed u64".to_owned())?;
    }
    if count != expected {
        return Err(format!(
            "key iteration produced {count} rows; lock expects {expected}"
        ));
    }

    Ok(Audit {
        entries: count,
        key_sha256: key_digest.finish(),
        payload_sha256: payload_digest.finish(),
    })
}

fn validate_raw_match_group_if_first(
    key: &KeyEntry,
    matches: &KeyMatches,
    current: u64,
    mut key_at: impl FnMut(KeyOrdinal) -> mdictlib::Result<Option<KeyEntry>>,
) -> Result<bool, String> {
    if matches.basis() != MatchBasis::RawExact {
        return Err(format!(
            "raw locate used normalized matching at ordinal {current}"
        ));
    }

    if ordinal_position(matches, key.ordinal()).is_none() {
        return Err(format!(
            "raw duplicate set omitted physical ordinal {current}"
        ));
    }
    if key.ordinal() != matches.first() {
        return Ok(false);
    }

    // The first physical member validates the complete raw-key group. Every
    // later member still proves its own membership above without rescanning
    // the group, including when equal raw keys are not physically contiguous.
    let mut previous = None;
    for ordinal in matches.iter() {
        if previous.is_some_and(|value| value >= ordinal) {
            return Err(format!(
                "raw duplicate ordinals were not strictly ascending at ordinal {current}"
            ));
        }
        let matched = key_at(ordinal)
            .map_err(|error| format!("duplicate key_at failed at ordinal {current}: {error}"))?
            .ok_or_else(|| format!("duplicate key_at returned no row at ordinal {current}"))?;
        if matched.key() != key.key() {
            return Err(format!(
                "raw duplicate set included a different key at ordinal {current}"
            ));
        }
        previous = Some(ordinal);
    }
    Ok(true)
}

fn ordinal_position(matches: &KeyMatches, target: KeyOrdinal) -> Option<usize> {
    ordinal_position_by(matches.len(), target, |index| matches.get(index))
}

fn ordinal_position_by(
    len: usize,
    target: KeyOrdinal,
    mut ordinal_at: impl FnMut(usize) -> Option<KeyOrdinal>,
) -> Option<usize> {
    let mut start = 0usize;
    let mut end = len;
    while start < end {
        let middle = start + (end - start) / 2;
        match ordinal_at(middle)?.cmp(&target) {
            std::cmp::Ordering::Less => start = middle + 1,
            std::cmp::Ordering::Equal => return Some(middle),
            std::cmp::Ordering::Greater => end = middle,
        }
    }
    None
}

fn audit_mdd(path: &Path, expected: u64) -> Result<Audit, String> {
    let dictionary = MddFile::open(path).map_err(|error| format!("open failed: {error}"))?;
    if dictionary.len() != expected {
        return Err(format!(
            "declared entry count is {}; lock expects {expected}",
            dictionary.len()
        ));
    }

    let mut key_digest = Sha256::new();
    let mut payload_digest = Sha256::new();
    let mut count = 0u64;
    for result in dictionary.keys() {
        let key =
            result.map_err(|error| format!("key iteration failed at ordinal {count}: {error}"))?;
        require_contiguous(&key, count)?;
        digest_key(&mut key_digest, &key);

        let ordinal_key = dictionary
            .key_at(key.ordinal())
            .map_err(|error| format!("key_at failed at ordinal {count}: {error}"))?
            .ok_or_else(|| format!("key_at returned no row at ordinal {count}"))?;
        if ordinal_key != key {
            return Err(format!("key_at disagreed at ordinal {count}"));
        }

        let span = dictionary
            .span_at(key.ordinal())
            .map_err(|error| format!("span_at failed at ordinal {count}: {error}"))?
            .ok_or_else(|| format!("span_at returned no row at ordinal {count}"))?;
        if span.key_entry() != &key {
            return Err(format!("span_at key disagreed at ordinal {count}"));
        }
        let resource = dictionary
            .resource_at(key.ordinal())
            .map_err(|error| format!("resource_at failed at ordinal {count}: {error}"))?
            .ok_or_else(|| format!("resource_at returned no row at ordinal {count}"))?;
        if resource.key_entry() != &key {
            return Err(format!("resource_at key disagreed at ordinal {count}"));
        }
        let span_resource = span
            .read()
            .map_err(|error| format!("span read failed at ordinal {count}: {error}"))?;
        if span_resource != resource {
            return Err(format!("span read disagreed at ordinal {count}"));
        }
        drop(span_resource);
        let resource_len = u64::try_from(resource.bytes().len())
            .map_err(|_| format!("resource length exceeds u64 at ordinal {count}"))?;
        if span.len() != resource_len {
            return Err(format!("span length disagreed at ordinal {count}"));
        }
        let mut streamed_digest = Sha256::new();
        let copied = span
            .copy_to(&mut DigestWriter(&mut streamed_digest))
            .map_err(|error| format!("span stream failed at ordinal {count}: {error}"))?;
        if copied != span.len() {
            return Err(format!("span stream length disagreed at ordinal {count}"));
        }
        let mut materialized_digest = Sha256::new();
        materialized_digest.update(resource.bytes());
        if streamed_digest.finish() != materialized_digest.finish() {
            return Err(format!(
                "span stream bytes disagreed with materialization at ordinal {count}"
            ));
        }

        digest_key(&mut payload_digest, resource.key_entry());
        payload_digest.update(&span.len().to_be_bytes());
        payload_digest.update(resource.bytes());

        let matches = dictionary
            .locate(key.key())
            .map_err(|error| format!("raw locate failed at ordinal {count}: {error}"))?
            .ok_or_else(|| format!("raw locate returned no matches at ordinal {count}"))?;
        let is_group_first = validate_raw_match_group_if_first(&key, &matches, count, |ordinal| {
            dictionary.key_at(ordinal)
        })?;
        if is_group_first {
            let lookup_span = dictionary
                .lookup_span(key.key())
                .map_err(|error| format!("raw lookup_span failed at ordinal {count}: {error}"))?
                .ok_or_else(|| format!("raw lookup_span returned no row at ordinal {count}"))?;
            if lookup_span.ordinal() != key.ordinal()
                || lookup_span.key() != key.key()
                || lookup_span.len() != span.len()
            {
                return Err(format!("raw lookup_span disagreed at ordinal {count}"));
            }
            let lookup = dictionary
                .lookup(key.key())
                .map_err(|error| format!("raw lookup failed at ordinal {count}: {error}"))?
                .ok_or_else(|| format!("raw lookup returned no resource at ordinal {count}"))?;
            if lookup != resource {
                return Err(format!("raw lookup disagreed at ordinal {count}"));
            }
        }
        count = count
            .checked_add(1)
            .ok_or_else(|| "physical entry count overflowed u64".to_owned())?;
    }
    if count != expected {
        return Err(format!(
            "key iteration produced {count} rows; lock expects {expected}"
        ));
    }

    Ok(Audit {
        entries: count,
        key_sha256: key_digest.finish(),
        payload_sha256: payload_digest.finish(),
    })
}

fn require_contiguous(key: &KeyEntry, expected: u64) -> Result<(), String> {
    if key.ordinal().get() == expected {
        Ok(())
    } else {
        Err(format!(
            "physical ordinal {} appeared where {expected} was required",
            key.ordinal().get()
        ))
    }
}

fn digest_key(digest: &mut Sha256, key: &KeyEntry) {
    digest.update(&key.ordinal().get().to_be_bytes());
    let bytes = key.key().as_bytes();
    digest.update(&(bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

fn digest_bytes(digest: &mut Sha256, bytes: &[u8]) -> Result<(), String> {
    let len = u64::try_from(bytes.len()).map_err(|_| "payload length exceeds u64".to_owned())?;
    digest.update(&len.to_be_bytes());
    digest.update(bytes);
    Ok(())
}

struct DigestWriter<'a>(&'a mut Sha256);

impl Write for DigestWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn sanitize_diagnostic(value: &str) -> String {
    let mut output = String::new();
    let mut truncated = false;
    for (count, character) in value.chars().enumerate() {
        if count >= MAX_DIAGNOSTIC_CHARS {
            truncated = true;
            break;
        }
        match character {
            '\n' | '\r' | '\t' => output.push(' '),
            value if value.is_control() => output.push('\u{fffd}'),
            value => output.push(value),
        }
    }
    if truncated {
        output.push('…');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        IDENTITY_PROTOCOL, PROTOCOL, TOOL, audit_mdd, audit_mdx,
        fixture_support::FixtureBuilder,
        ordinal_position_by, sanitize_diagnostic,
        sha256::{Sha256, hex},
        validate_raw_match_group_if_first,
    };
    use mdictlib::{KeyOrdinal, MdxFile};

    #[test]
    fn identity_protocol_is_versioned_and_unambiguous() {
        assert_eq!(IDENTITY_PROTOCOL, "mdictlib-corpus-audit-identity-v1");
        assert_eq!(PROTOCOL, "mdictlib-corpus-audit-v1");
        assert_eq!(TOOL, "mdictlib corpus_audit");
        assert!(!env!("CARGO_PKG_VERSION").is_empty());
    }

    #[test]
    fn sha256_matches_the_standard_abc_vector() {
        let mut digest = Sha256::new();
        digest.update(b"abc");
        assert_eq!(
            hex(&digest.finish()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn synthetic_mdx_and_mdd_complete_the_real_audit_routes() {
        let mdx = FixtureBuilder::mdx([
            ("duplicate", "first"),
            ("duplicate", "second"),
            ("omega", "third"),
        ])
        .key_blocks(vec![1, 1, 1])
        .build()
        .write("corpus-audit-mdx");
        let mdx_audit = audit_mdx(mdx.path(), 3).unwrap();
        assert_eq!(mdx_audit.entries, 3);
        assert_ne!(mdx_audit.key_sha256, [0; 32]);
        assert_ne!(mdx_audit.payload_sha256, [0; 32]);

        let mdd = FixtureBuilder::mdd([
            ("\\duplicate.bin", vec![1, 2, 3]),
            ("\\duplicate.bin", vec![4, 5]),
            ("\\omega.bin", vec![6, 7, 8, 9]),
        ])
        .key_blocks(vec![1, 1, 1])
        .build()
        .write("corpus-audit-mdd");
        let mdd_audit = audit_mdd(mdd.path(), 3).unwrap();
        assert_eq!(mdd_audit.entries, 3);
        assert_ne!(mdd_audit.key_sha256, [0; 32]);
        assert_ne!(mdd_audit.payload_sha256, [0; 32]);
    }

    #[test]
    fn large_noncontiguous_duplicate_run_is_validated_once_with_logarithmic_membership() {
        const DUPLICATES: usize = 4_096;
        const INTERLOPER: usize = DUPLICATES / 2;

        let entries = (0..=DUPLICATES).map(|index| {
            if index == INTERLOPER {
                ("interloper".to_owned(), format!("payload-{index}"))
            } else {
                ("duplicate".to_owned(), format!("payload-{index}"))
            }
        });
        let fixture = FixtureBuilder::mdx(entries)
            .key_blocks(vec![INTERLOPER, 1, DUPLICATES - INTERLOPER])
            .build()
            .write("corpus-audit-large-duplicate-run");
        let dictionary = MdxFile::open(fixture.path()).unwrap();

        let mut groups = 0usize;
        let mut group_key_at_calls = 0usize;
        for result in dictionary.keys() {
            let key = result.unwrap();
            let matches = dictionary.locate(key.key()).unwrap().unwrap();
            if validate_raw_match_group_if_first(&key, &matches, key.ordinal().get(), |ordinal| {
                group_key_at_calls += 1;
                dictionary.key_at(ordinal)
            })
            .unwrap()
            {
                groups += 1;
            }
        }
        assert_eq!(groups, 2);
        assert_eq!(group_key_at_calls, DUPLICATES + 1);

        let mut membership_probes = 0usize;
        for target in 0..DUPLICATES {
            let target = KeyOrdinal::new(u64::try_from(target).unwrap());
            assert_eq!(
                ordinal_position_by(DUPLICATES, target, |index| {
                    membership_probes += 1;
                    Some(KeyOrdinal::new(u64::try_from(index).unwrap()))
                }),
                Some(usize::try_from(target.get()).unwrap())
            );
        }
        assert!(membership_probes <= DUPLICATES * 13);
    }

    #[test]
    fn diagnostics_are_single_line_and_bounded() {
        let source = format!("start\n{}\0end", "x".repeat(3_000));
        let sanitized = sanitize_diagnostic(&source);
        assert!(!sanitized.contains('\n'));
        assert!(!sanitized.contains('\0'));
        assert!(sanitized.chars().count() <= 2_049);
        assert!(sanitized.ends_with('…'));
    }
}
