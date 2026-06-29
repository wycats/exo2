//! Type-safe operation path definitions.
//!
//! This module defines all operation paths as constants, eliminating
//! the need for duplicate string literals across the handler registry,
//! `InputExtractor`, and steering functions.

/// A type-safe operation path that provides both array and dotted representations.
#[derive(Debug, Clone, Copy)]
pub struct OpPath {
    /// The path as an array of segments (e.g., `["feedback", "thread", "create"]`)
    pub segments: &'static [&'static str],
    /// The path as a dotted string (e.g., `"feedback.thread.create"`)
    pub dotted: &'static str,
}

impl OpPath {
    /// Create a new `OpPath`. Use the `op_path!` macro for const construction.
    pub const fn new(segments: &'static [&'static str], dotted: &'static str) -> Self {
        Self { segments, dotted }
    }

    /// Get the segments for registry lookup.
    pub const fn segments(&self) -> &'static [&'static str] {
        self.segments
    }

    /// Get the dotted name for error messages.
    pub const fn name(&self) -> &'static str {
        self.dotted
    }
}

/// Macro to define an operation path with both representations.
///
/// Usage: `op_path!["feedback", "thread", "create"]`
/// Expands to: `OpPath::new(&["feedback", "thread", "create"], "feedback.thread.create")`
#[macro_export]
macro_rules! op_path {
    [$($segment:literal),+ $(,)?] => {
        $crate::api::operations::OpPath::new(
            &[$($segment),+],
            concat!($($segment, "."),+).trim_end_matches('.')
        )
    };
}

// Re-export for convenience
pub use op_path;
