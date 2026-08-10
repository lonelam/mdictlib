//! Wire-format parsing, and the one place that knows more than one version
//! exists.
//!
//! # Dispatch contract
//!
//! Opening a dictionary parses the bounded common header exactly once,
//! resolves exactly one [`WireVersion`] from it, and then enters exactly one
//! grammar:
//!
//! ```text
//! common header -> WireVersion -> v1::parse_layout | v2::parse_layout
//!                              -> ValidatedLayout  -> shared core
//! ```
//!
//! [`open_layout`] contains the only `match` on [`WireVersion`] in the crate.
//! A grammar that fails is never retried under the other grammar, and no code
//! path rewrites one version's bytes into another's shape.
//!
//! `format::v1` and `format::v2` cannot see each other, and cannot see the
//! core or the MDX/MDD facades. Everything they share lives in [`common`], and
//! everything they produce is a
//! [`ValidatedLayout`](common::descriptors::ValidatedLayout).

pub(crate) mod common;
mod v1;
mod v2;

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::limits::MemoryBudget;
use crate::types::{ContainerKind, OpenOptions};

use self::common::descriptors::ValidatedLayout;
use self::common::header::{HeaderSection, parse_header};
use self::common::source::FileSource;

pub(crate) use self::common::encoding::TextEncoding;

/// The major wire version a file declares.
///
/// This enum exists only inside this module. Resolving it and matching on it
/// both happen once, during open; nothing downstream can name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WireVersion {
    V1,
    V2,
}

/// Everything a grammar needs to parse a file, so both `parse_layout`
/// signatures stay identical and reviewable side by side.
pub(crate) struct LayoutRequest<'a> {
    pub(crate) source: &'a FileSource,
    pub(crate) header_section: HeaderSection,
    pub(crate) kind: ContainerKind,
    pub(crate) options: &'a OpenOptions,
    pub(crate) memory: &'a Arc<MemoryBudget>,
}

/// Parses one dictionary file into a version-neutral validated layout.
///
/// # Errors
///
/// Returns an error if the common header is malformed or exceeds a limit, the
/// declared version is unsupported or ambiguous, or the selected grammar
/// rejects the file's geometry.
pub(crate) fn open_layout(
    source: &FileSource,
    kind: ContainerKind,
    options: &OpenOptions,
    memory: &Arc<MemoryBudget>,
) -> Result<ValidatedLayout> {
    let header_section = parse_header(source, kind, &options.limits, memory)?;
    let version = resolve_wire_version(declared_major_version(
        header_section.header.generated_by_engine_version(),
    ))?;
    let request = LayoutRequest {
        source,
        header_section,
        kind,
        options,
        memory,
    };

    // The only version match in the crate. Each arm is entered at most once
    // per open, and a failure inside an arm propagates instead of falling
    // through to the other grammar.
    let layout = match version {
        WireVersion::V1 => v1::parse_layout(request),
        WireVersion::V2 => v2::parse_layout(request),
    }?;

    layout.verify()?;
    Ok(layout)
}

/// Extracts the declared major component of an engine-version attribute.
///
/// Returns `None` when the attribute is empty, non-numeric, or otherwise
/// unparsable, which the caller turns into a structured refusal rather than a
/// guess.
fn declared_major_version(version: &str) -> Option<u32> {
    version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
}

/// Maps a declared major version onto an implemented grammar.
///
/// `GeneratedByEngineVersion` is the authority, matching the shipped `0.1.0`
/// behavior; `RequiredEngineVersion` is not consulted, so changing the version
/// authority cannot happen as a side effect of an unrelated edit.
fn resolve_wire_version(declared_major: Option<u32>) -> Result<WireVersion> {
    match declared_major {
        Some(1) => Ok(WireVersion::V1),
        Some(2) => Ok(WireVersion::V2),
        _ => Err(Error::Unsupported(
            "MDict format major version other than 1 or 2",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_each_implemented_major_version() {
        assert_eq!(
            resolve_wire_version(declared_major_version("1.2")).unwrap(),
            WireVersion::V1
        );
        assert_eq!(
            resolve_wire_version(declared_major_version("2.0")).unwrap(),
            WireVersion::V2
        );
    }

    #[test]
    fn accepts_a_bare_major_component() {
        assert_eq!(declared_major_version("1"), Some(1));
        assert_eq!(declared_major_version("2"), Some(2));
    }

    #[test]
    fn rejects_unsupported_and_unparsable_versions() {
        for declared in ["0.9", "3.0", "", "x.y", "two", " 2.0", "-1.0", "1x.2"] {
            let error = resolve_wire_version(declared_major_version(declared)).unwrap_err();
            assert!(
                matches!(error, Error::Unsupported(_)),
                "expected {declared:?} to be refused"
            );
        }
    }

    #[test]
    fn ignores_minor_and_trailing_components() {
        assert_eq!(declared_major_version("1.2.3.4"), Some(1));
        assert_eq!(declared_major_version("2.0-beta"), Some(2));
    }
}
