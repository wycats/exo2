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

#![forbid(unsafe_code)]

mod ulid_util;

pub use ulid_util::{
    ExoUlid, format_canonical_ref, generate_ulid, is_valid_ulid, parse_canonical_ref, parse_ulid,
};

#[cfg(feature = "wasm")]
mod wasm;
