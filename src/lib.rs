//! Library-first parser for MDict `.mdx` and `.mdd` files.
//!
//! MDX and MDD share one defensive, file-backed parsing core. Opening parses
//! bounded metadata and block indexes; key and record blocks remain lazy.
//!
//! Physical ordinals preserve duplicate identity. Global raw-exact lookup wins
//! before header-normalized fallback, and MDD resources can be streamed through
//! source-bound spans. Per-open limits bound untrusted input and parser work.
//!
//! Version `0.1.0` is the first public release of `mdictlib`.

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
mod source;
mod types;

#[cfg(fuzzing)]
#[doc(hidden)]
pub mod fuzzing;

pub use error::{Error, Result};
pub use lookup::{KeyMatches, MatchBasis};
pub use mdd::{MddFile, MddResource, MddResourceSpan};
pub use mdx::{MdxEntry, MdxFile};
pub use types::{Header, KeyEntry, KeyOrdinal, Limits, MemoryUsage, OpenOptions, Passcode};
