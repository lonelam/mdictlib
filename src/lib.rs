//! Library-first parser for MDict `.mdx` and `.mdd` files.
//!
//! MDX and MDD share one defensive, file-backed parsing core. Opening parses
//! bounded metadata and block indexes; key and record blocks remain lazy.
//!
//! Physical ordinals preserve duplicate identity. Global raw-exact lookup wins
//! before header-normalized fallback, and MDD resources can be streamed through
//! source-bound spans. Per-open limits bound untrusted input and parser work.
//!
//! # Wire versions
//!
//! MDict major versions 1 and 2 are both read through this API. The version is
//! resolved once, from the header, and selects one grammar; it never reaches
//! lookup, iteration, ordinal access, record decoding, or MDD streaming. A file
//! that fails one grammar is never retried under the other.
//!
//! Version 1 support covers unencrypted files using uncompressed or LZO blocks.
//! Encrypted version 1 files and the ISO8859-1 text label are refused with a
//! precise error rather than parsed speculatively.
//!
//! Version `0.2.0` adds MDict major version 1 support behind the same public
//! API as the first public release, `0.1.0`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::missing_errors_doc)]

mod core;
mod error;
mod format;
mod limits;
mod lookup;
mod mdd;
mod mdx;
mod types;

#[cfg(fuzzing)]
#[doc(hidden)]
pub mod fuzzing;

pub use error::{Error, Result};
pub use lookup::{KeyMatches, MatchBasis};
pub use mdd::{MddFile, MddResource, MddResourceSpan};
pub use mdx::{MdxEntry, MdxFile};
pub use types::{Header, KeyEntry, KeyOrdinal, Limits, MemoryUsage, OpenOptions, Passcode};
