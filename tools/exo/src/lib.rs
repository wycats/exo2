#![allow(missing_docs)]
#![warn(unreachable_pub)] // Flag pub items that don't need to be pub (prevents dead code accumulation)
#![allow(clippy::print_stdout, clippy::print_stderr)]
#![allow(clippy::disallowed_methods)] // CLI tool uses blocking I/O
// Crate-wide lint allowances for pedantic lints that are style-only:
#![allow(clippy::format_push_string)] // The performance difference is negligible
#![allow(clippy::must_use_candidate)] // Not all builder methods need must_use
#![allow(clippy::return_self_not_must_use)] // Builder methods return Self
#![allow(clippy::result_large_err)] // CLI error types are acceptable
#![allow(clippy::too_many_lines)] // Some functions are inherently complex
#![allow(clippy::option_if_let_else)] // Pattern matching is often clearer
#![allow(clippy::items_after_statements)] // Grouping imports near usage is sometimes clearer
#![allow(clippy::cast_precision_loss)] // Acceptable for progress/display calculations
#![allow(clippy::cast_possible_truncation)] // Acceptable for display calculations
#![allow(clippy::cast_sign_loss)] // Acceptable for display calculations
#![allow(clippy::match_same_arms)] // Sometimes clarity > deduplication
#![allow(clippy::branches_sharing_code)] // Sometimes clarity > deduplication
#![allow(clippy::needless_pass_by_value)] // CLI command structs are small
#![allow(clippy::let_and_return)] // Sometimes clearer with named bindings
#![allow(clippy::unnecessary_wraps)] // Matching trait signatures sometimes requires wrapping
#![allow(clippy::unused_self)] // Plugin trait methods may not use self
#![allow(clippy::case_sensitive_file_extension_comparisons)] // We control file extensions
#![allow(clippy::single_match_else)] // Pattern matching is often clearer
#![allow(clippy::if_not_else)] // Condition order varies by context
#![allow(clippy::needless_for_each)] // Loop clarity varies
#![allow(clippy::wrong_self_convention)] // into_ methods sometimes take &self
#![allow(clippy::manual_let_else)] // Pattern matching is often clearer
#![allow(clippy::needless_range_loop)] // Index-based loops sometimes clearer
#![cfg_attr(
    not(test),
    deny(clippy::expect_used, clippy::panic, clippy::unwrap_used)
)]
#![cfg_attr(
    test,
    allow(
        clippy::approx_constant,
        clippy::comparison_to_empty,
        clippy::doc_markdown,
        clippy::double_comparisons,
        clippy::expect_used,
        clippy::float_cmp,
        clippy::items_after_test_module,
        clippy::needless_collect,
        clippy::panic,
        clippy::redundant_clone,
        clippy::similar_names,
        clippy::single_char_add_str,
        clippy::unwrap_used
    )
)]

pub mod activity;
pub mod api;
pub mod argv_compiler;
pub mod axiom;
pub mod boundary;
pub mod cli_quote;
pub mod command;
pub mod command_guidance;
pub mod command_reference;
pub mod command_spec;
pub mod command_text;
pub mod config;
pub mod context;
pub mod daemon;
pub mod daemon_client;
pub mod daemon_diagnostics;
pub mod daemon_transport;
pub mod derived;
pub mod diagnostics;
pub mod docs_links;
pub mod event_db;
pub mod failure;
pub(crate) mod git_config;
pub(crate) mod github;
pub mod help_gen;
pub mod idea; // Re-exported for schema tests
pub mod inbox; // Re-exported for schema tests
pub mod json_schema;
pub mod map;
pub mod marker_inventory;
pub mod mcp;
pub mod merge_driver;
pub(crate) mod phase;
pub(crate) mod phase_owner;
pub(crate) mod plan;
pub mod post_write;
pub mod preload_guidance;
pub mod project;
pub mod rfc;
pub mod router;
pub mod run;
pub mod session_boundary;
pub mod shell_ops;
pub mod state_machine;
pub mod status;
pub mod steering;
pub mod structured_io;
pub(crate) mod task;
pub mod templates;
pub mod ui;
pub mod ulid_util;
pub mod upgrade;
pub mod utils;
pub mod verifiers;
pub mod verify;
pub mod world_state;

pub type ExoResult<T> = anyhow::Result<T>;
