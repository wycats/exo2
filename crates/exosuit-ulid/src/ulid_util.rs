//! ULID utilities for Exosuit identifiers.
//!
//! Provides generation, formatting, and parsing of ULIDs for use as
//! canonical identifiers for epochs, phases, tasks, and other entities.
//!
//! # Canonical Reference Format
//!
//! Entities are referenced using the format: `type@ULID`
//! Examples:
//! - `phase@01HZVY3X4M5N6P7Q8R9S0TABC1`
//! - `task@01HZVY3X4M5N6P7Q8R9S0TABC2`
//! - `epoch@01HZVY3X4M5N6P7Q8R9S0TABC3`

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;
use ulid::Ulid;

/// A wrapper around [`ulid::Ulid`] that implements [`serde`] traits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExoUlid(Ulid);

impl ExoUlid {
    /// Generate a new ULID with the current timestamp.
    #[must_use]
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    /// Create an `ExoUlid` from a `Ulid`.
    #[must_use]
    pub const fn from_ulid(ulid: Ulid) -> Self {
        Self(ulid)
    }

    /// Get the inner `Ulid`.
    #[must_use]
    pub const fn inner(&self) -> Ulid {
        self.0
    }

    /// Format as a canonical reference string.
    ///
    /// # Example
    /// ```
    /// use exosuit_ulid::ExoUlid;
    /// let id = ExoUlid::new();
    /// assert!(id.canonical_ref("phase").starts_with("phase@"));
    /// ```
    #[must_use]
    pub fn canonical_ref(&self, type_name: &str) -> String {
        format!("{type_name}@{self}")
    }

    /// Check if a string is a valid ULID.
    #[must_use]
    pub fn is_valid(s: &str) -> bool {
        Ulid::from_string(s).is_ok()
    }

    /// Parse a canonical reference string.
    ///
    /// Returns `Some((type_name, ulid))` if valid, `None` otherwise.
    ///
    /// # Example
    /// ```
    /// use exosuit_ulid::ExoUlid;
    /// let id = ExoUlid::new();
    /// let ref_str = id.canonical_ref("phase");
    /// let parsed = ExoUlid::parse_canonical_ref(&ref_str);
    /// assert!(parsed.is_some());
    /// let (type_name, _) = parsed.unwrap();
    /// assert_eq!(type_name, "phase");
    /// ```
    #[must_use]
    pub fn parse_canonical_ref(s: &str) -> Option<(String, Self)> {
        let (type_name, ulid_str) = s.split_once('@')?;
        if type_name.is_empty() {
            return None;
        }
        let ulid = Ulid::from_string(ulid_str).ok()?;
        Some((type_name.to_string(), Self(ulid)))
    }
}

impl Default for ExoUlid {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ExoUlid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ExoUlid {
    type Err = ulid::DecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ulid::from_string(s).map(Self)
    }
}

impl Serialize for ExoUlid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for ExoUlid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ulid::from_string(&s)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

/// Generate a new ULID.
#[must_use]
pub fn generate_ulid() -> ExoUlid {
    ExoUlid::new()
}

/// Format a ULID as a canonical reference.
///
/// # Example
/// ```
/// use exosuit_ulid::{generate_ulid, format_canonical_ref};
/// let id = generate_ulid();
/// let ref_str = format_canonical_ref("phase", &id);
/// assert!(ref_str.starts_with("phase@"));
/// ```
#[must_use]
pub fn format_canonical_ref(type_name: &str, ulid: &ExoUlid) -> String {
    ulid.canonical_ref(type_name)
}

/// Parse a ULID from a string.
///
/// Returns `None` if the string is not a valid ULID.
#[must_use]
pub fn parse_ulid(s: &str) -> Option<ExoUlid> {
    s.parse().ok()
}

/// Check if a string is a valid ULID.
#[must_use]
pub fn is_valid_ulid(s: &str) -> bool {
    ExoUlid::is_valid(s)
}

/// Parse a canonical reference string.
///
/// Returns `Some((type_name, ulid))` if valid, `None` otherwise.
#[must_use]
pub fn parse_canonical_ref(s: &str) -> Option<(String, ExoUlid)> {
    ExoUlid::parse_canonical_ref(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_ulid() {
        let id1 = generate_ulid();
        let id2 = generate_ulid();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_canonical_ref() {
        let id = generate_ulid();
        let ref_str = format_canonical_ref("phase", &id);
        assert!(ref_str.starts_with("phase@"));
        assert_eq!(ref_str.len(), 6 + 26); // "phase@" + 26-char ULID
    }

    #[test]
    fn test_parse_canonical_ref() {
        let id = generate_ulid();
        let ref_str = format_canonical_ref("task", &id);
        let parsed = parse_canonical_ref(&ref_str);
        assert!(parsed.is_some());
        let (type_name, parsed_id) = parsed.unwrap();
        assert_eq!(type_name, "task");
        assert_eq!(parsed_id, id);
    }

    #[test]
    fn test_parse_canonical_ref_invalid() {
        assert!(parse_canonical_ref("invalid").is_none());
        assert!(parse_canonical_ref("@ULID").is_none());
        assert!(parse_canonical_ref("type@invalid").is_none());
    }

    #[test]
    fn test_is_valid_ulid() {
        let id = generate_ulid();
        assert!(is_valid_ulid(&id.to_string()));
        assert!(!is_valid_ulid("invalid"));
        assert!(!is_valid_ulid(""));
    }

    #[test]
    fn test_ulid_display() {
        let id = generate_ulid();
        let s = id.to_string();
        assert_eq!(s.len(), 26);
        // ULID uses Crockford Base32
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_serde_roundtrip() {
        let id = generate_ulid();
        let json = serde_json::to_string(&id).unwrap();
        let parsed: ExoUlid = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_parse_ulid() {
        let id = generate_ulid();
        let s = id.to_string();
        let parsed = parse_ulid(&s);
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap(), id);
    }
}
