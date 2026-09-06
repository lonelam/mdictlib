use std::cmp::Ordering;
use std::mem::size_of;
use std::ops::Range;
use std::sync::Arc;

use super::{CachedFailure, CachedValue, MdictFile};
use crate::error::{Error, Result};
use crate::limits::{
    MemoryReservation, checked_usize, ensure_usize_limit, try_clone_string, try_reserve_vec,
};
use crate::types::KeyOrdinal;

#[derive(Debug)]
struct LocatorRow {
    raw: Box<str>,
    normalized: Box<str>,
}

#[derive(Debug)]
pub(crate) struct KeyLocator {
    rows: Box<[LocatorRow]>,
    by_raw: Box<[u32]>,
    by_normalized: Box<[u32]>,
    _memory: MemoryReservation,
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
    range: Range<usize>,
}

impl LocatedKeys {
    pub(crate) const fn basis(&self) -> LocatorBasis {
        self.basis
    }

    pub(crate) fn len(&self) -> usize {
        self.range.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.range.is_empty()
    }

    pub(crate) fn ordinal_at(&self, index: usize) -> Option<KeyOrdinal> {
        let position = self.range.start.checked_add(index)?;
        if position >= self.range.end {
            return None;
        }
        let ordinal = match self.basis {
            LocatorBasis::RawExact => *self.locator.by_raw.get(position)?,
            LocatorBasis::HeaderNormalized => *self.locator.by_normalized.get(position)?,
        };
        Some(KeyOrdinal::new(u64::from(ordinal)))
    }
}

impl MdictFile {
    pub(crate) fn locate_keys(&self, query: &str) -> Result<Option<LocatedKeys>> {
        let locator = self.key_locator()?;
        if let Some(range) = locator.equal_range(LocatorBasis::RawExact, query) {
            return Ok(Some(LocatedKeys {
                locator,
                basis: LocatorBasis::RawExact,
                range,
            }));
        }

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
        if let Some(range) = locator.equal_range(LocatorBasis::HeaderNormalized, &normalized) {
            return Ok(Some(LocatedKeys {
                locator,
                basis: LocatorBasis::HeaderNormalized,
                range,
            }));
        }
        Ok(None)
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

        let fixed_bytes = entry_count
            .checked_mul(size_of::<LocatorRow>() + 2 * size_of::<u32>())
            .ok_or(Error::InvalidFormat("key locator size overflow"))?;
        enforce_locator_size(fixed_bytes, self.limits.locator_bytes)?;
        let mut retained_memory = self.memory.reserve(fixed_bytes, "key locator")?;

        let mut rows = Vec::new();
        try_reserve_vec(&mut rows, entry_count, "key locator rows")?;
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
                    .checked_add(entry.key.len())
                    .and_then(|value| value.checked_add(normalized_len))
                    .ok_or(Error::InvalidFormat("key locator size overflow"))?;
                enforce_locator_size(estimated_bytes, self.limits.locator_bytes)?;
                retained_memory.grow(
                    entry
                        .key
                        .len()
                        .checked_add(normalized_len)
                        .ok_or(Error::InvalidFormat("key locator size overflow"))?,
                )?;
                let normalized = self.normalizer.normalize(&entry.key)?;
                rows.push(LocatorRow {
                    raw: try_clone_string(&entry.key, "key locator raw key")?.into_boxed_str(),
                    normalized: normalized.into_boxed_str(),
                });
            }

            let expected_end = usize::try_from(
                block
                    .entry_start_index
                    .checked_add(block.entry_count)
                    .ok_or(Error::InvalidFormat("keyword entry count overflow"))?,
            )
            .map_err(|_| Error::InvalidFormat("keyword entry count exceeds usize"))?;
            if rows.len() != expected_end {
                return Err(Error::InvalidData(format!(
                    "locator row count mismatch after block {block_index}: expected {expected_end}, decoded {}",
                    rows.len()
                )));
            }
            drop(entries);
        }

        if rows.len() != entry_count {
            return Err(Error::InvalidData(format!(
                "locator row count mismatch: expected {entry_count}, decoded {}",
                rows.len()
            )));
        }

        let mut by_raw = Vec::new();
        let mut by_normalized = Vec::new();
        try_reserve_vec(&mut by_raw, entry_count, "raw locator index")?;
        try_reserve_vec(&mut by_normalized, entry_count, "normalized locator index")?;
        for index in 0..entry_count {
            let index = u32::try_from(index)
                .map_err(|_| Error::InvalidFormat("locator row index exceeds u32"))?;
            by_raw.push(index);
            by_normalized.push(index);
        }

        by_raw.sort_unstable_by(|left, right| compare_rows(&rows, *left, *right, false));
        by_normalized.sort_unstable_by(|left, right| compare_rows(&rows, *left, *right, true));

        Ok(KeyLocator {
            rows: rows.into_boxed_slice(),
            by_raw: by_raw.into_boxed_slice(),
            by_normalized: by_normalized.into_boxed_slice(),
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

    fn equal_range(&self, basis: LocatorBasis, query: &str) -> Option<Range<usize>> {
        let index = match basis {
            LocatorBasis::RawExact => &self.by_raw,
            LocatorBasis::HeaderNormalized => &self.by_normalized,
        };
        let key = |ordinal: &u32| {
            let row_index = usize::try_from(*ordinal)
                .expect("locator ordinal was created from an in-range usize");
            let row = &self.rows[row_index];
            match basis {
                LocatorBasis::RawExact => row.raw.as_ref(),
                LocatorBasis::HeaderNormalized => row.normalized.as_ref(),
            }
        };
        let start = index.partition_point(|ordinal| key(ordinal) < query);
        let end = index.partition_point(|ordinal| key(ordinal) <= query);
        (start < end).then_some(start..end)
    }
}

fn compare_rows(rows: &[LocatorRow], left: u32, right: u32, normalized: bool) -> Ordering {
    let left = usize::try_from(left).expect("locator ordinal originated as usize");
    let right = usize::try_from(right).expect("locator ordinal originated as usize");
    let left_row = &rows[left];
    let right_row = &rows[right];
    let key_order = if normalized {
        left_row.normalized.cmp(&right_row.normalized)
    } else {
        left_row.raw.cmp(&right_row.raw)
    };
    key_order.then_with(|| left.cmp(&right))
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
