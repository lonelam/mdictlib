use crate::error::{Error, Result};
use crate::limits::try_reserve_string;
use crate::types::Header;

/// The clean-room compatibility profile used when `StripKey` is enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StripKeyProfile {
    /// Retain ASCII letters/digits and leave every non-ASCII character intact.
    AsciiAlphanumeric,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct KeyNormalizer {
    case_sensitive: bool,
    strip_profile: Option<StripKeyProfile>,
}

impl KeyNormalizer {
    pub(super) const fn from_header(header: &Header) -> Self {
        Self {
            case_sensitive: header.key_case_sensitive,
            strip_profile: if header.strip_key {
                Some(StripKeyProfile::AsciiAlphanumeric)
            } else {
                None
            },
        }
    }

    pub(super) fn normalize(self, raw: &str) -> Result<String> {
        let capacity = self.normalized_len(raw)?;
        let mut normalized = String::new();
        try_reserve_string(&mut normalized, capacity, "normalized key")?;

        for character in raw.chars().filter(|character| {
            !matches!(self.strip_profile, Some(StripKeyProfile::AsciiAlphanumeric))
                || !character.is_ascii()
                || character.is_ascii_alphanumeric()
        }) {
            if self.case_sensitive {
                normalized.push(character);
            } else {
                normalized.extend(character.to_lowercase());
            }
        }
        Ok(normalized)
    }

    pub(super) fn normalized_len(self, raw: &str) -> Result<usize> {
        let mut length = 0usize;
        for character in raw.chars().filter(|character| {
            !matches!(self.strip_profile, Some(StripKeyProfile::AsciiAlphanumeric))
                || !character.is_ascii()
                || character.is_ascii_alphanumeric()
        }) {
            if self.case_sensitive {
                length = length
                    .checked_add(character.len_utf8())
                    .ok_or(Error::InvalidFormat("normalized key length overflow"))?;
            } else {
                for lowercase in character.to_lowercase() {
                    length = length
                        .checked_add(lowercase.len_utf8())
                        .ok_or(Error::InvalidFormat("normalized key length overflow"))?;
                }
            }
        }
        Ok(length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalizer(case_sensitive: bool, strip: bool) -> KeyNormalizer {
        KeyNormalizer {
            case_sensitive,
            strip_profile: strip.then_some(StripKeyProfile::AsciiAlphanumeric),
        }
    }

    #[test]
    fn preserves_raw_shape_without_strip_key() {
        assert_eq!(
            normalizer(true, false).normalize(" A-b.c ").unwrap(),
            " A-b.c "
        );
    }

    #[test]
    fn strip_key_removes_non_alphanumeric_ascii_anywhere() {
        assert_eq!(
            normalizer(false, true).normalize(" A-b.c_D ").unwrap(),
            "abcd"
        );
    }

    #[test]
    fn strip_key_preserves_non_ascii_whitespace_and_punctuation() {
        assert_eq!(
            normalizer(true, true).normalize("A\u{2003}B—C").unwrap(),
            "A\u{2003}B—C"
        );
    }

    #[test]
    fn case_folding_can_expand_unicode_characters() {
        assert_eq!(normalizer(false, false).normalize("İ").unwrap(), "i\u{307}");
    }
}
