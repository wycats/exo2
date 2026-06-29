//! Shared reactive types for the Exosuit revision algebra.
//!
//! This crate contains the core type vocabulary shared between `exosuit-storage`
//! (which records traces via virtual tables) and `exosuit-reactivity` (which
//! validates and manages reactive roots). It compiles to any target — no
//! platform-specific dependencies.
//!
//! See RFC 10165 §8 for the design rationale.

mod revision;
mod trace;
mod types;

pub use revision::{Epoch, Revision};
pub use trace::{ResourceSpec, StateProvider, Trace, TraceEntry};
pub use types::CellId;
