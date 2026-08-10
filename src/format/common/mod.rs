//! Version-neutral parsing machinery.
//!
//! Everything in this module is shared by every wire grammar: the bounded
//! common header, bounded cursors and checked arithmetic, text decoding, the
//! block envelope and codecs, checksums, cryptographic primitives, file-backed
//! I/O, and the [`descriptors`] boundary that grammars hand to the core.
//!
//! Nothing here may branch on the wire version.

pub(crate) mod checked;
pub(crate) mod checksum;
pub(crate) mod compression;
pub(crate) mod crypto;
pub(crate) mod cursor;
pub(crate) mod descriptors;
pub(crate) mod encoding;
pub(crate) mod header;
pub(crate) mod source;
