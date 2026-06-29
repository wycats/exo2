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
    /// ```ignore
    /// let id = ExoUlid::new();
    /// assert!(id.canonical_ref("phase").starts_with("phase@"));
    /// ```
    #[must_use]
    pub fn canonical_ref(&self, type_name: &str) -> String {
        format!("{type_name}@{self}")
    }

    /// Check if a string is a valid ULID.
    #[must_use]
    pub const fn is_valid(s: &str) -> bool {
        Ulid::from_string(s).is_ok()
    }

    /// Parse a canonical reference string.
    ///
    /// Returns `Some((type_name, ulid))` if valid, `None` otherwise.
    ///
    /// # Example
    /// ```ignore
    /// let parsed = ExoUlid::parse_canonical_ref("phase@01HZVY3X4M5N6P7Q8R9S0TABC1");
    /// assert!(parsed.is_some());
    /// let (type_name, ulid) = parsed.unwrap();
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
/// ```ignore
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
pub const fn is_valid_ulid(s: &str) -> bool {
    ExoUlid::is_valid(s)
}

/// Parse a canonical reference string.
///
/// Returns `Some((type_name, ulid))` if valid, `None` otherwise.
#[must_use]
pub fn parse_canonical_ref(s: &str) -> Option<(String, ExoUlid)> {
    ExoUlid::parse_canonical_ref(s)
}

/// Result of ID resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdMatch {
    /// Matched by ULID (either raw ULID string or canonical ref)
    Ulid(ExoUlid),
    /// Matched by slug
    Slug(String),
    /// Matched by legacy ID or alias
    LegacyId(String),
}

/// Trait for entities that can be looked up by various ID formats.
pub trait UlidResolvable {
    /// Get the ULID if present.
    fn get_ulid(&self) -> Option<&ExoUlid>;
    /// Get the slug if present.
    fn get_slug(&self) -> Option<&str>;
    /// Get the primary string ID.
    fn get_id(&self) -> &str;
    /// Get any aliases.
    fn get_aliases(&self) -> &[String];

    /// Check if this entity matches the given lookup key.
    ///
    /// Matches against (in order):
    /// 1. Canonical reference (type@ULID) - type is ignored for matching
    /// 2. Raw ULID string
    /// 3. Slug
    /// 4. Primary ID
    /// 5. Aliases
    fn matches_id(&self, lookup: &str) -> Option<IdMatch> {
        // Try canonical reference first (e.g., "epoch:01KGC1817T...")
        // Canonical refs are unambiguous, so if it parses but doesn't match, return None
        if let Some((_, ulid)) = parse_canonical_ref(lookup) {
            if self
                .get_ulid()
                .is_some_and(|entity_ulid| *entity_ulid == ulid)
            {
                return Some(IdMatch::Ulid(ulid));
            }
            return None;
        }

        // Try raw ULID match first
        // Note: Don't return None on mismatch - the lookup might be a slug that
        // happens to parse as a ULID (e.g., lowercase ULID-like strings used as slugs)
        if let Some(lookup_ulid) = parse_ulid(lookup)
            && self
                .get_ulid()
                .is_some_and(|entity_ulid| *entity_ulid == lookup_ulid)
        {
            return Some(IdMatch::Ulid(lookup_ulid));
        }

        // Try slug
        if let Some(slug) = self.get_slug()
            && slug == lookup
        {
            return Some(IdMatch::Slug(slug.to_string()));
        }

        // Try primary ID
        if self.get_id() == lookup {
            return Some(IdMatch::LegacyId(lookup.to_string()));
        }

        // Try aliases
        for alias in self.get_aliases() {
            if alias == lookup {
                return Some(IdMatch::LegacyId(alias.clone()));
            }
        }

        None
    }
}

/// Format output to echo the canonical reference.
///
/// Given a matched ID and entity, returns a string that can be shown to the user
/// to indicate how the ID was resolved.
#[allow(clippy::option_if_let_else)] // Nested if-let is clearer here than map_or_else
pub fn echo_canonical<T: UlidResolvable>(entity: &T, type_name: &str) -> String {
    if let Some(ulid) = entity.get_ulid() {
        let slug_part = entity
            .get_slug()
            .map_or(String::new(), |s| format!(" ({s})"));
        format!("{type_name}@{ulid}{slug_part}")
    } else if let Some(slug) = entity.get_slug() {
        format!("{type_name}:{slug}")
    } else {
        format!("{type_name}:{}", entity.get_id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_ulid() {
        let id1 = generate_ulid();
        let id2 = generate_ulid();
        // ULIDs should be unique
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_ulid_display() {
        let id = generate_ulid();
        let s = id.to_string();
        // ULID strings are 26 characters
        assert_eq!(s.len(), 26);
        // All characters should be valid Crockford Base32
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_ulid_roundtrip() {
        let id = generate_ulid();
        let s = id.to_string();
        let parsed: ExoUlid = s.parse().expect("should parse");
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_canonical_ref() {
        let id = generate_ulid();
        let ref_str = format_canonical_ref("phase", &id);
        assert!(ref_str.starts_with("phase@"));
        assert_eq!(ref_str.len(), 6 + 26); // "phase@" + 26 char ULID
    }

    #[test]
    fn test_parse_canonical_ref() {
        let id = generate_ulid();
        let ref_str = format_canonical_ref("task", &id);
        let (type_name, parsed_id) = parse_canonical_ref(&ref_str).expect("should parse");
        assert_eq!(type_name, "task");
        assert_eq!(parsed_id, id);
    }

    #[test]
    fn test_parse_canonical_ref_invalid() {
        assert!(parse_canonical_ref("invalid").is_none());
        assert!(parse_canonical_ref("phase@invalid").is_none());
        assert!(parse_canonical_ref("@01HZVY3X4M5N6P7Q8R9S0TABC1").is_none());
    }

    #[test]
    fn test_is_valid_ulid() {
        let id = generate_ulid();
        assert!(is_valid_ulid(&id.to_string()));
        assert!(!is_valid_ulid("invalid"));
        assert!(!is_valid_ulid(""));
        // Too short
        assert!(!is_valid_ulid("01HZVY3X4M5N6P7Q8R"));
        // Too long
        assert!(!is_valid_ulid("01HZVY3X4M5N6P7Q8R9S0TABC1EXTRA"));
    }

    #[test]
    fn test_serde_roundtrip() {
        let id = generate_ulid();
        let json = serde_json::to_string(&id).expect("should serialize");
        let parsed: ExoUlid = serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_ordering() {
        // ULIDs generated in sequence should be orderable (unique, not necessarily ordered
        // if generated in the same millisecond - the random portion handles uniqueness)
        let id1 = generate_ulid();
        let id2 = generate_ulid();
        // ULIDs should be unique, even if generated in quick succession
        assert_ne!(id1, id2);
        // And they should be orderable (either id1 < id2 or id1 > id2)
        assert!(id1 < id2 || id1 > id2);
    }

    // Test entity for UlidResolvable trait
    struct TestEntity {
        id: String,
        ulid: Option<ExoUlid>,
        slug: Option<String>,
        aliases: Vec<String>,
    }

    impl UlidResolvable for TestEntity {
        fn get_ulid(&self) -> Option<&ExoUlid> {
            self.ulid.as_ref()
        }
        fn get_slug(&self) -> Option<&str> {
            self.slug.as_deref()
        }
        fn get_id(&self) -> &str {
            &self.id
        }
        fn get_aliases(&self) -> &[String] {
            &self.aliases
        }
    }

    #[test]
    fn test_ulid_resolvable_by_ulid() {
        let ulid = generate_ulid();
        let entity = TestEntity {
            id: "legacy-id".to_string(),
            ulid: Some(ulid),
            slug: Some("my-slug".to_string()),
            aliases: vec!["alias1".to_string()],
        };

        // Match by raw ULID
        let result = entity.matches_id(&ulid.to_string());
        assert!(matches!(result, Some(IdMatch::Ulid(_))));

        // Match by canonical ref
        let canonical = format!("phase@{}", ulid);
        let result = entity.matches_id(&canonical);
        assert!(matches!(result, Some(IdMatch::Ulid(_))));
    }

    #[test]
    fn test_ulid_resolvable_by_slug() {
        let ulid = generate_ulid();
        let entity = TestEntity {
            id: "legacy-id".to_string(),
            ulid: Some(ulid),
            slug: Some("my-slug".to_string()),
            aliases: vec![],
        };

        let result = entity.matches_id("my-slug");
        assert!(matches!(result, Some(IdMatch::Slug(_))));
    }

    #[test]
    fn test_ulid_resolvable_by_legacy_id() {
        let entity = TestEntity {
            id: "legacy-id".to_string(),
            ulid: None,
            slug: None,
            aliases: vec![],
        };

        let result = entity.matches_id("legacy-id");
        assert!(matches!(result, Some(IdMatch::LegacyId(_))));
    }

    #[test]
    fn test_ulid_resolvable_by_alias() {
        let entity = TestEntity {
            id: "legacy-id".to_string(),
            ulid: None,
            slug: None,
            aliases: vec!["alias1".to_string(), "alias2".to_string()],
        };

        let result = entity.matches_id("alias2");
        assert!(matches!(result, Some(IdMatch::LegacyId(_))));
    }

    #[test]
    fn test_ulid_resolvable_no_match() {
        let entity = TestEntity {
            id: "legacy-id".to_string(),
            ulid: None,
            slug: Some("my-slug".to_string()),
            aliases: vec![],
        };

        let result = entity.matches_id("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_echo_canonical_with_ulid() {
        let ulid = generate_ulid();
        let entity = TestEntity {
            id: "legacy-id".to_string(),
            ulid: Some(ulid),
            slug: Some("my-slug".to_string()),
            aliases: vec![],
        };

        let echo = echo_canonical(&entity, "phase");
        assert!(echo.starts_with("phase@"));
        assert!(echo.contains("(my-slug)"));
    }

    #[test]
    fn test_echo_canonical_without_ulid() {
        let entity = TestEntity {
            id: "legacy-id".to_string(),
            ulid: None,
            slug: Some("my-slug".to_string()),
            aliases: vec![],
        };

        let echo = echo_canonical(&entity, "phase");
        assert_eq!(echo, "phase:my-slug");
    }
}
