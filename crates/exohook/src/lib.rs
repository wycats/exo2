//! exohook: Git hook validation and workflow automation.
//!
//! This crate provides a system for running pre-commit, pre-push, and other
//! git hooks with features like parallel execution, auto-fixing, and CI generation.
//!
//! # Configuration Formats
//!
//! - **v2**: Current format with `[lane.*]` and `[check.*]` sections
//! - **v3**: New simplified format (RFC 00215) with `[hooks]` as primary interface
//!
//! See [`config`] module for schema types.

// The library re-exports public types from config module.
// Many internal functions are used by the binary but not the library,
// which triggers dead_code warnings. We also use blocking I/O.
#![allow(dead_code)]
#![allow(clippy::disallowed_methods)]

pub mod config;
pub mod fileset;
pub mod filter;
pub mod jsonl;
pub mod validate;

// Re-export key types for convenience
mod check_runner;
mod ci_emit;
pub mod hooks;
mod lane;
mod legacy;
mod migration;
pub mod output_buffer;
mod pipe_runner;
mod shell;
pub mod terminal;

#[cfg(unix)]
mod pty_runner;

use clap::ValueEnum;

pub(crate) use check_runner::{OutputMode, spawn_check};
pub(crate) use legacy::{resolve_check_command_parts, validate_hooks_doc};
pub(crate) use output_buffer::CheckProgressGroup;
pub use output_buffer::OutputBuffer;

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable compact output (default).
    Compact,
    /// Human-readable grouped output with check headers.
    Grouped,
    /// Machine-readable NDJSON streaming output.
    /// Each event is a JSON object on its own line.
    Jsonl,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

// Re-export key types for convenience
pub use config::{
    CheckCategory, CheckV3, ConfigV3, ConfigVersion, DefaultsV3, ExecutionContext, HookType,
    HooksV3, RunnerConfig, WorkflowV3, parse_runner_config,
};
pub use fileset::FilesetScope;
pub use validate::validate_v3_workflow;
