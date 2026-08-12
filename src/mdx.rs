use std::fmt;
use std::iter::FusedIterator;
use std::ops::ControlFlow;
use std::path::Path;

use crate::core::{MdictFile, RecordDescriptor, RecordIter};
use crate::error::Result;
use crate::format::TextEncoding;
use crate::lookup::KeyMatches;
use crate::types::{ContainerKind, Header, KeyEntry, KeyOrdinal, MemoryUsage, OpenOptions};

/// An opened MDX text dictionary.
pub struct MdxFile {
    inner: MdictFile,
}

impl fmt::Debug for MdxFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MdxFile")
            .field("len", &self.len())
            .field("title", &self.header().title())
            .finish_non_exhaustive()
    }
}

/// One decoded MDX entry with its physical identity.
#[derive(PartialEq, Eq)]
pub struct MdxEntry {
    key: KeyEntry,
    text: String,
}

impl fmt::Debug for MdxEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MdxEntry")
            .field("ordinal", &self.ordinal())
            .field("key", &self.key())
            .field("text_len", &self.text.len())
            .finish()
    }
}

impl MdxEntry {
    fn new(key: KeyEntry, text: String) -> Self {
        Self { key, text }
    }

    /// Returns the entry's physical ordinal.
    pub const fn ordinal(&self) -> KeyOrdinal {
        self.key.ordinal()
    }

    /// Returns the original decoded key.
    pub fn key(&self) -> &str {
        self.key.key()
    }

    /// Returns the physical key row.
    pub fn key_entry(&self) -> &KeyEntry {
        &self.key
    }

    /// Returns the decoded MDX record text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Consumes the entry into its key row and decoded text.
    pub fn into_parts(self) -> (KeyEntry, String) {
        (self.key, self.text)
    }
}

impl MdxFile {
    /// Opens an MDX file with default options.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, requests an unsupported
    /// format path, exceeds a safety limit, or contains malformed metadata or
    /// indexes.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_options(path, &OpenOptions::new())
    }

    /// Opens an MDX file with reusable explicit options.
    ///
    /// # Errors
    ///
    /// Returns an error under the same conditions as [`Self::open`], including
    /// when an encrypted header cannot be opened with the supplied options.
    pub fn open_with_options(path: impl AsRef<Path>, options: &OpenOptions) -> Result<Self> {
        Ok(Self {
            inner: MdictFile::open(path, ContainerKind::Mdx, options)?,
        })
    }

    /// Returns parsed top-level header metadata.
    pub fn header(&self) -> &Header {
        self.inner.header()
    }

    /// Returns the number of physical dictionary entries.
    pub fn len(&self) -> u64 {
        self.inner.len()
    }

    /// Returns whether the dictionary declares no entries.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Reports parser memory charged to this open handle's safety budget.
    ///
    /// # Errors
    ///
    /// Returns an error if an internal cache mutex was poisoned by a panic in
    /// another thread.
    pub fn memory_usage(&self) -> Result<MemoryUsage> {
        self.inner.memory_usage()
    }

    /// Iterates over original keys in physical file order without reading
    /// record payloads.
    ///
    /// Block read and decode failures are yielded as an item; after yielding
    /// an error, the iterator is exhausted.
    pub fn keys(&self) -> impl FusedIterator<Item = Result<KeyEntry>> + '_ {
        self.inner.keys()
    }

    /// Returns one physical key row by ordinal without reading its record.
    ///
    /// # Errors
    ///
    /// Returns an error if the corresponding key block cannot be read or is
    /// malformed.
    pub fn key_at(&self, ordinal: KeyOrdinal) -> Result<Option<KeyEntry>> {
        self.inner.key_at_ordinal(ordinal)
    }

    /// Resolves physical key rows for the supplied ordinals in input order.
    /// Repeated and out-of-range ordinals are preserved as repeated values and
    /// `None`, respectively.
    ///
    /// # Errors
    ///
    /// Returns an error if any requested key block cannot be read or is
    /// malformed.
    pub fn keys_at(&self, ordinals: &[KeyOrdinal]) -> Result<Vec<Option<KeyEntry>>> {
        self.inner.keys_at_ordinals(ordinals)
    }

    /// Reads and decodes one MDX entry by physical ordinal.
    ///
    /// # Errors
    ///
    /// Returns an error if its key or record blocks cannot be read, are
    /// malformed, exceed a materialization limit, or contain invalid text.
    pub fn entry_at(&self, ordinal: KeyOrdinal) -> Result<Option<MdxEntry>> {
        self.inner
            .record_at_ordinal(ordinal)?
            .map(|descriptor| self.read_entry(descriptor))
            .transpose()
    }

    /// Locates every physical entry matching a query without reading records.
    ///
    /// Raw-exact matches across the entire file always win. Header-normalized
    /// fallback is considered only when no raw key matches, and duplicates are
    /// returned in ascending physical order.
    ///
    /// # Errors
    ///
    /// Returns an error if the global locator cannot be built because key data
    /// is malformed, unreadable, or exceeds a safety limit.
    pub fn locate(&self, query: &str) -> Result<Option<KeyMatches>> {
        self.inner
            .locate_keys(query)
            .map(|matches| matches.map(KeyMatches::from_located))
    }

    /// Locates physical entries whose key starts with `prefix` under the
    /// header's own normalization, in normalized order, stopping after `limit`.
    ///
    /// Duplicates are preserved, so a caller presenting completions to a person
    /// should ask for more than it intends to show and collapse repeats itself.
    ///
    /// # Errors
    ///
    /// Returns an error if the global locator cannot be built because key data
    /// is malformed, unreadable, or exceeds a safety limit.
    pub fn prefix_keys(&self, prefix: &str, limit: usize) -> Result<Vec<KeyEntry>> {
        let ordinals = self.inner.locate_prefix(prefix, limit)?;
        Ok(self
            .inner
            .keys_at_ordinals(&ordinals)?
            .into_iter()
            .flatten()
            .collect())
    }

    /// Visits every entry's normalized key in physical order, without copying
    /// it, until the callback breaks.
    ///
    /// This exists so a caller can apply its own search policy — completion
    /// ranking, edit distance, transliteration — across the whole key space
    /// without building and retaining a second copy of it. Resolve a chosen
    /// [`KeyOrdinal`] back to its original key with [`Self::key_at`].
    ///
    /// # Errors
    ///
    /// Returns an error if the global locator cannot be built because key data
    /// is malformed, unreadable, or exceeds a safety limit.
    pub fn scan_normalized_keys<F>(&self, visit: F) -> Result<()>
    where
        F: FnMut(KeyOrdinal, &str) -> ControlFlow<()>,
    {
        self.inner.scan_normalized_keys(visit)
    }

    /// Looks up and decodes one entry using the dictionary's current key
    /// matching behavior.
    ///
    /// # Errors
    ///
    /// Returns an error if lookup encounters malformed key data or the matched
    /// record cannot be read and decoded.
    pub fn lookup(&self, query: &str) -> Result<Option<MdxEntry>> {
        let Some(matches) = self.locate(query)? else {
            return Ok(None);
        };
        self.entry_at(matches.first())
    }

    /// Iterates lazily over decoded entries in physical file order.
    ///
    /// Block read and decode failures are yielded as an item; after yielding
    /// an error, the iterator is exhausted.
    pub fn entries(&self) -> impl FusedIterator<Item = Result<MdxEntry>> + '_ {
        MdxEntryIter {
            file: self,
            records: self.inner.records(),
            done: false,
        }
    }

    fn read_entry(&self, descriptor: RecordDescriptor) -> Result<MdxEntry> {
        let encoded_len = descriptor
            .end
            .checked_sub(descriptor.start)
            .ok_or(crate::Error::InvalidFormat("record range is inverted"))?;
        let _memory = self.inner.reserve_decoded_record_text(encoded_len)?;
        let bytes = self
            .inner
            .read_record_span(descriptor.start, descriptor.end)?;
        let text = decode_record_text(self.inner.record_text_encoding()?, &bytes)?;
        Ok(MdxEntry::new(descriptor.key, text))
    }
}

struct MdxEntryIter<'a> {
    file: &'a MdxFile,
    records: RecordIter<'a>,
    done: bool,
}

impl Iterator for MdxEntryIter<'_> {
    type Item = Result<MdxEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let result = self
            .records
            .next()
            .map(|result| result.and_then(|descriptor| self.file.read_entry(descriptor)));
        if result.as_ref().is_some_and(Result::is_err) {
            self.done = true;
        }
        result
    }
}

impl FusedIterator for MdxEntryIter<'_> {}

fn decode_record_text(encoding: TextEncoding, bytes: &[u8]) -> Result<String> {
    let mut text = encoding.decode(bytes, "mdx record")?;
    let trimmed_len = text.trim_end_matches('\0').len();
    text.truncate(trimmed_len);
    Ok(text)
}
