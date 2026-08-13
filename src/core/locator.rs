use std::cmp::Ordering;
use std::mem::size_of;
use std::ops::{ControlFlow, Range};
use std::sync::Arc;

use super::{CachedFailure, CachedValue, MdictFile};
use crate::error::{Error, Result};
use crate::limits::{
    MemoryReservation, checked_usize, ensure_usize_limit, try_reserve_string_amortized,
    try_reserve_vec, try_reserve_vec_amortized,
};
use crate::types::KeyOrdinal;

/// Every key's normalized text in one allocation, plus the orderings needed to
/// answer a query.
///
/// Storing normalized text alone, rather than a normalized *and* a raw copy per
/// row, is what keeps this affordable on a multi-million-entry file: a
/// raw-exact match necessarily normalizes to the same text as its query, so
/// every raw candidate already lies inside the normalized equal range. The
/// range is then filtered by a cheap raw-text digest, and only a digest hit
/// pays for the key block that proves it.
pub(crate) struct KeyLocator {
    /// Normalized keys concatenated in physical order.
    text: Box<str>,
    /// `bounds[row]..bounds[row + 1]` is that row's slice of `text`.
    bounds: Box<[u32]>,
    /// Rows sorted by normalized text, then by physical ordinal.
    order: Box<[u32]>,
    /// One digest of each row's *raw* text, in physical order.
    raw_digest: Box<[u32]>,
    _memory: MemoryReservation,
}

impl std::fmt::Debug for KeyLocator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KeyLocator")
            .field("rows", &self.order.len())
            .field("text_bytes", &self.text.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocatorBasis {
    RawExact,
    HeaderNormalized,
}

#[derive(Debug, Clone)]
pub(crate) struct LocatedKeys {
    locator: Arc<KeyLocator>,
    basis: LocatorBasis,
    /// The normalized equal range, used whole for a header-normalized match.
    range: Range<usize>,
    /// Physical ordinals proven raw-exact, when the basis is [`LocatorBasis::RawExact`].
    exact: Option<Arc<[u32]>>,
}

impl LocatedKeys {
    pub(crate) const fn basis(&self) -> LocatorBasis {
        self.basis
    }

    pub(crate) fn len(&self) -> usize {
        match &self.exact {
            Some(exact) => exact.len(),
            None => self.range.len(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn ordinal_at(&self, index: usize) -> Option<KeyOrdinal> {
        let ordinal = match &self.exact {
            Some(exact) => *exact.get(index)?,
            None => {
                let position = self.range.start.checked_add(index)?;
                if position >= self.range.end {
                    return None;
                }
                *self.locator.order.get(position)?
            }
        };
        Some(KeyOrdinal::new(u64::from(ordinal)))
    }
}

impl MdictFile {
    pub(crate) fn locate_keys(&self, query: &str) -> Result<Option<LocatedKeys>> {
        let locator = self.key_locator()?;

        let normalized_len = self.normalizer.normalized_len(query)?;
        ensure_usize_limit(
            "normalized_query_bytes",
            normalized_len,
            self.limits.locator_bytes,
        )?;
        let _query_memory = self
            .memory
            .reserve(normalized_len, "normalized lookup query")?;
        let normalized = self.normalizer.normalize(query)?;

        let Some(range) = locator.equal_range(&normalized) else {
            return Ok(None);
        };

        // Raw-exact still wins over the normalized fallback, but the search for
        // it is confined to this range: raw equality implies normalized
        // equality, so nothing outside can be raw-exact.
        if let Some(exact) = self.raw_exact_within(&locator, &range, query)? {
            return Ok(Some(LocatedKeys {
                locator,
                basis: LocatorBasis::RawExact,
                range,
                exact: Some(exact),
            }));
        }

        Ok(Some(LocatedKeys {
            locator,
            basis: LocatorBasis::HeaderNormalized,
            range,
            exact: None,
        }))
    }

    /// Physical ordinals inside `range` whose raw key equals `query`, in
    /// ascending physical order, or `None` when there are none.
    fn raw_exact_within(
        &self,
        locator: &KeyLocator,
        range: &Range<usize>,
        query: &str,
    ) -> Result<Option<Arc<[u32]>>> {
        let digest = raw_digest(query);
        let mut matches: Vec<u32> = Vec::new();
        for position in range.clone() {
            let row = *locator.order.get(position).ok_or(Error::InvalidFormat(
                "locator range escaped its order index",
            ))?;
            let index = usize::try_from(row)
                .map_err(|_| Error::InvalidFormat("locator row index exceeds usize"))?;
            if *locator
                .raw_digest
                .get(index)
                .ok_or(Error::InvalidFormat("locator row has no raw digest"))?
                != digest
            {
                continue;
            }
            // A digest hit is only a candidate; the key block decides.
            let ordinal = KeyOrdinal::new(u64::from(row));
            let hit = self
                .key_entry_at_ordinal(ordinal)?
                .ok_or(Error::InvalidFormat("locator row has no key block entry"))?;
            let entry = hit
                .entries
                .get(hit.entry_index)
                .ok_or(Error::InvalidFormat("locator row escaped its key block"))?;
            if entry.key == query {
                try_reserve_vec_amortized(&mut matches, 1, "raw-exact locator matches")?;
                matches.push(row);
            }
        }
        if matches.is_empty() {
            return Ok(None);
        }
        matches.sort_unstable();
        Ok(Some(Arc::from(matches.into_boxed_slice())))
    }

    /// Physical ordinals whose normalized key starts with the normalized
    /// `prefix`, in normalized order, stopping after `limit` of them.
    pub(crate) fn locate_prefix(&self, prefix: &str, limit: usize) -> Result<Vec<KeyOrdinal>> {
        let mut found = Vec::new();
        if limit == 0 {
            return Ok(found);
        }
        let locator = self.key_locator()?;

        let normalized_len = self.normalizer.normalized_len(prefix)?;
        ensure_usize_limit(
            "normalized_query_bytes",
            normalized_len,
            self.limits.locator_bytes,
        )?;
        let _query_memory = self
            .memory
            .reserve(normalized_len, "normalized prefix query")?;
        let normalized = self.normalizer.normalize(prefix)?;

        let start = locator
            .order
            .partition_point(|row| locator.row(*row) < normalized.as_str());
        for row in locator.order.iter().skip(start) {
            if !locator.row(*row).starts_with(normalized.as_str()) {
                break;
            }
            try_reserve_vec_amortized(&mut found, 1, "prefix locator matches")?;
            found.push(KeyOrdinal::new(u64::from(*row)));
            if found.len() == limit {
                break;
            }
        }
        Ok(found)
    }

    /// Visits every physical row's normalized key in physical order.
    pub(crate) fn scan_normalized_keys<F>(&self, mut visit: F) -> Result<()>
    where
        F: FnMut(KeyOrdinal, &str) -> ControlFlow<()>,
    {
        let locator = self.key_locator()?;
        for row in 0..locator.row_count() {
            let ordinal = KeyOrdinal::new(u64::from(row));
            if visit(ordinal, locator.row(row)).is_break() {
                break;
            }
        }
        Ok(())
    }

    fn key_locator(&self) -> Result<Arc<KeyLocator>> {
        if let Some(locator) = self.locator.get() {
            return cached_locator(locator);
        }

        let _build = self
            .locator_build
            .lock()
            .map_err(|_| Error::InvalidFormat("key locator build mutex poisoned"))?;
        if let Some(locator) = self.locator.get() {
            return cached_locator(locator);
        }

        match self.build_key_locator() {
            Ok(locator) => {
                let locator = Arc::new(locator);
                let _ = self.locator.set(CachedValue::Ready(Arc::clone(&locator)));
                Ok(locator)
            }
            Err(error) => {
                if let Some(cached) = CachedFailure::capture(&error, &self.memory) {
                    let _ = self.locator.set(CachedValue::Failed(cached));
                }
                Err(error)
            }
        }
    }

    fn build_key_locator(&self) -> Result<KeyLocator> {
        let maximum_entries = self.limits.locator_entries.min(u64::from(u32::MAX));
        if self.len() > maximum_entries {
            return Err(Error::LimitExceeded {
                limit: "locator_entries",
                value: self.len(),
                max: maximum_entries,
            });
        }
        let entry_count = checked_usize(self.len(), "locator entry count")?;

        let bound_slots = entry_count
            .checked_add(1)
            .ok_or(Error::InvalidFormat("key locator size overflow"))?;
        let order_and_digest_slots = entry_count
            .checked_mul(2)
            .ok_or(Error::InvalidFormat("key locator size overflow"))?;
        let fixed_bytes = bound_slots
            .checked_add(order_and_digest_slots)
            .and_then(|slots| slots.checked_mul(size_of::<u32>()))
            .ok_or(Error::InvalidFormat("key locator size overflow"))?;
        enforce_locator_size(fixed_bytes, self.limits.locator_bytes)?;
        let mut retained_memory = self.memory.reserve(fixed_bytes, "key locator")?;

        let mut text = String::new();
        let mut bounds = Vec::new();
        let mut digests = Vec::new();
        try_reserve_vec(&mut bounds, bound_slots, "key locator bounds")?;
        try_reserve_vec(&mut digests, entry_count, "key locator raw digests")?;
        bounds.push(0);
        let mut estimated_bytes = fixed_bytes;
        let mut previous_record_start = None;

        for block_index in 0..self.key_block_count() {
            let block = self
                .layout
                .key_blocks
                .get(block_index)
                .ok_or(Error::InvalidFormat("key block index out of range"))?;
            let entries = self.decode_key_block(block_index)?;
            for entry in entries.iter() {
                if entry.record_start > self.layout.total_decoded_record_len {
                    return Err(Error::InvalidData(format!(
                        "record start {} exceeds total record bytes {}",
                        entry.record_start, self.layout.total_decoded_record_len
                    )));
                }
                if let Some(previous) = previous_record_start
                    && previous > entry.record_start
                {
                    return Err(Error::InvalidData(format!(
                        "record starts decrease from {previous} to {}",
                        entry.record_start
                    )));
                }
                previous_record_start = Some(entry.record_start);

                let normalized_len = self.normalizer.normalized_len(&entry.key)?;
                estimated_bytes = estimated_bytes
                    .checked_add(normalized_len)
                    .ok_or(Error::InvalidFormat("key locator size overflow"))?;
                enforce_locator_size(estimated_bytes, self.limits.locator_bytes)?;
                retained_memory.grow(normalized_len)?;
                // Amortized rather than exact: growing by one key at a time
                // would reallocate the whole arena on every entry.
                try_reserve_string_amortized(&mut text, normalized_len, "key locator text")?;
                self.normalizer.normalize_into(&entry.key, &mut text);
                bounds.push(
                    u32::try_from(text.len())
                        .map_err(|_| Error::InvalidFormat("key locator text exceeds u32"))?,
                );
                digests.push(raw_digest(&entry.key));
            }

            let expected_end = usize::try_from(
                block
                    .entry_start_index
                    .checked_add(block.entry_count)
                    .ok_or(Error::InvalidFormat("keyword entry count overflow"))?,
            )
            .map_err(|_| Error::InvalidFormat("keyword entry count exceeds usize"))?;
            if digests.len() != expected_end {
                return Err(Error::InvalidData(format!(
                    "locator row count mismatch after block {block_index}: expected {expected_end}, decoded {}",
                    digests.len()
                )));
            }
            drop(entries);
        }

        if digests.len() != entry_count {
            return Err(Error::InvalidData(format!(
                "locator row count mismatch: expected {entry_count}, decoded {}",
                digests.len()
            )));
        }

        let text = text.into_boxed_str();
        let bounds = bounds.into_boxed_slice();
        let mut order = Vec::new();
        try_reserve_vec(&mut order, entry_count, "normalized locator index")?;
        for index in 0..entry_count {
            let index = u32::try_from(index)
                .map_err(|_| Error::InvalidFormat("locator row index exceeds u32"))?;
            order.push(index);
        }
        order.sort_unstable_by(|left, right| compare_rows(&text, &bounds, *left, *right));

        Ok(KeyLocator {
            text,
            bounds,
            order: order.into_boxed_slice(),
            raw_digest: digests.into_boxed_slice(),
            _memory: retained_memory,
        })
    }
}

fn cached_locator(locator: &CachedValue<KeyLocator>) -> Result<Arc<KeyLocator>> {
    match locator {
        CachedValue::Ready(locator) => Ok(Arc::clone(locator)),
        CachedValue::Failed(error) => Err(error.replay()),
    }
}

impl KeyLocator {
    pub(super) const fn memory_bytes(&self) -> usize {
        self._memory.bytes()
    }

    /// The normalized text of one physical row.
    fn row(&self, row: u32) -> &str {
        row_text(&self.text, &self.bounds, row)
    }

    fn row_count(&self) -> u32 {
        // The build path proved this fits `u32`.
        self.order.len() as u32
    }

    fn equal_range(&self, query: &str) -> Option<Range<usize>> {
        let start = self.order.partition_point(|row| self.row(*row) < query);
        let end = self.order.partition_point(|row| self.row(*row) <= query);
        (start < end).then_some(start..end)
    }
}

fn row_text<'a>(text: &'a str, bounds: &[u32], row: u32) -> &'a str {
    let index = row as usize;
    let (Some(start), Some(end)) = (bounds.get(index), bounds.get(index + 1)) else {
        // Only reachable through a corrupt in-memory index, which the build
        // path rules out; an empty key simply sorts first.
        return "";
    };
    text.get(*start as usize..*end as usize).unwrap_or_default()
}

fn compare_rows(text: &str, bounds: &[u32], left: u32, right: u32) -> Ordering {
    row_text(text, bounds, left)
        .cmp(row_text(text, bounds, right))
        .then_with(|| left.cmp(&right))
}

/// A cheap order-sensitive digest, used only to skip rows that cannot be
/// raw-exact. Collisions cost one key-block probe, never a wrong answer.
fn raw_digest(raw: &str) -> u32 {
    const OFFSET: u32 = 2_166_136_261;
    const PRIME: u32 = 16_777_619;
    let mut hash = OFFSET;
    for byte in raw.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

fn enforce_locator_size(bytes: usize, maximum: usize) -> Result<()> {
    if bytes > maximum {
        let value = u64::try_from(bytes).unwrap_or(u64::MAX);
        let max = u64::try_from(maximum).unwrap_or(u64::MAX);
        return Err(Error::LimitExceeded {
            limit: "locator_bytes",
            value,
            max,
        });
    }
    Ok(())
}
