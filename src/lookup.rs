use std::fmt;

use crate::core::{LocatedKeys, LocatorBasis};
use crate::types::KeyOrdinal;

/// Describes why a key query matched physical dictionary entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MatchBasis {
    /// The decoded key was exactly equal to the query.
    RawExact,
    /// No raw key matched globally, so header-controlled normalization was used.
    HeaderNormalized,
}

/// A non-empty, duplicate-preserving set of physical key matches.
///
/// Matches are ordered by [`KeyOrdinal`]. Cloning this value does not copy the
/// underlying keys or locator indices.
#[derive(Clone)]
pub struct KeyMatches {
    inner: LocatedKeys,
}

impl KeyMatches {
    pub(crate) fn from_located(inner: LocatedKeys) -> Self {
        debug_assert!(!inner.is_empty());
        Self { inner }
    }

    /// Returns whether the query matched raw text or normalized fallback text.
    pub const fn basis(&self) -> MatchBasis {
        match self.inner.basis() {
            LocatorBasis::RawExact => MatchBasis::RawExact,
            LocatorBasis::HeaderNormalized => MatchBasis::HeaderNormalized,
        }
    }

    /// Returns the number of matching physical entries.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns whether this set is empty.
    ///
    /// Values returned by `locate()` are always non-empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns the lowest matching physical ordinal.
    pub fn first(&self) -> KeyOrdinal {
        self.inner
            .ordinal_at(0)
            .expect("KeyMatches always contains at least one ordinal")
    }

    /// Returns one matching ordinal by position in physical order.
    pub fn get(&self, index: usize) -> Option<KeyOrdinal> {
        self.inner.ordinal_at(index)
    }

    /// Iterates over matching ordinals in ascending physical order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = KeyOrdinal> + '_ {
        (0..self.len()).map(|index| {
            self.inner
                .ordinal_at(index)
                .expect("iterator indices stay inside the locator range")
        })
    }
}

impl fmt::Debug for KeyMatches {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KeyMatches")
            .field("basis", &self.basis())
            .field("ordinals", &DebugOrdinals(self))
            .finish()
    }
}

struct DebugOrdinals<'a>(&'a KeyMatches);

impl fmt::Debug for DebugOrdinals<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.0.iter()).finish()
    }
}
