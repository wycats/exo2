//! Command modules for the exo CLI.
//!
//! This module contains:
//! - **Trait architecture** (`traits`, `dispatcher`, `registry`): RFC 0085 command abstraction
//! - **Namespace modules**: Command implementations organized by namespace
//! - **Legacy modules** (`init`, `update`): Pre-trait implementations (to be migrated)

// Core trait architecture
pub mod command_spec;
pub(crate) mod completion_confirmation;
pub mod dispatcher;
pub mod lm_tool_metadata;
pub mod registry;
pub mod root;
pub mod router;
pub mod traits;
pub mod transport;
pub mod unified_diagnostics;
pub mod write;

// Namespace modules (Wave 1)
pub mod ai;
pub mod epoch;
pub mod json;
pub mod toml;

// Namespace modules (Wave 2 - simple)
pub mod axiom;
pub mod context;
pub mod docs;
pub mod dogfood;
pub mod idea;

// Namespace modules (Wave 2 - medium)
pub mod gc;
pub mod inbox;
pub mod strike;

// Namespace modules (Wave 3)
pub mod goal;
pub mod run;
pub mod task;

// Namespace modules (Wave 4)
pub mod rfc;

// Namespace modules (Wave 5)
pub mod plan;

// Namespace modules (Wave 6)
pub mod phase_cmd;

// Namespace modules (Project identity)
pub mod project;
pub mod sidecar;
pub mod storage;

// Namespace modules (Wave 8)
pub mod verify;

// Namespace modules (Wave 9 - Phase D)
pub mod commit;

// Legacy modules (pre-trait architecture)
pub mod init;
pub mod update;

// Re-exports for convenience
pub use command_spec::{
    ArgKind, ArgSpec, CommandSpec, LmToolMetadata, NamespaceSpec, OperationSpec, ValueType,
};
pub use dispatcher::CommandDispatcher;
pub use registry::{CommandMetadata, CommandRegistry, default_registry};
pub use router::{
    CommandFactory, CommandPath, DiagnosticCode, FactoryRegistry, FromInvocation, Frontend,
    Invocation, Router, RoutingDiagnostic, RoutingResult, SpecDispatcher, Suggestion, TypedValue,
};
pub use traits::CommandBox;
pub use traits::{
    Command, CommandContext, CommandInvokeResult, CommandOutput, MutableCommand,
    MutableCommandContext, OutputFormat, invoke_command_box_json,
};
pub use unified_diagnostics::{
    IntoDiagnosticSteering, format_diagnostic_human, format_diagnostic_plain,
    routing_diagnostic_to_steering, suggestion_to_action,
};
pub use update::UpdateCommand;
pub use write::Write;
