//! `ExoSpec` derive macro for unified command definition.
//!
//! This crate provides the `#[derive(ExoSpec)]` proc macro that generates
//! command metadata from `#[exo(...)]` attributes. It replaces the manual
//! `Command::args()` implementations and eliminates dual-source drift
//! between Clap definitions and `CommandSpec`.
//!
//! See RFC 00233 for the full design.
//!
//! # Usage
//!
//! ```ignore
//! #[derive(ExoSpec)]
//! #[exo(namespace = "tdd")]
//! enum TddCommands {
//!     /// Start a new TDD cycle
//!     #[exo(effect = "exec")]
//!     Start {
//!         /// The task to start TDD for
//!         #[exo(long)]
//!         name: String,
//!
//!         /// The test command or file
//!         #[exo(long)]
//!         test: String,
//!     },
//!
//!     /// Confirm the test is failing (red phase)
//!     #[exo(effect = "write")]
//!     Red,
//!
//!     /// Confirm the test is passing (green phase)
//!     #[exo(effect = "write")]
//!     Green,
//! }
//! ```

use proc_macro::TokenStream;

mod parse;

/// Derive macro for `ExoSpec` command definitions.
///
/// Generates:
/// - `HasExoSpec` trait implementation (returns `NamespaceSpec`)
/// - `args()` bridge method (for migration compatibility)
/// - `from_invocation()` constructor (future: typed struct from `Invocation`)
///
/// # Attributes
///
/// ## Namespace-level (on the enum)
/// - `#[exo(namespace = "...")]` — The command namespace (required)
///
/// ## Operation-level (on enum variants)
/// - `#[exo(effect = "pure|write|exec")]` — Side-effect classification (required)
/// - `#[exo(upgrade_gate)]` — Requires upgrade gate check
///
/// ## Argument-level (on struct fields)
/// - `#[exo(long)]` — Expose as `--field-name`
/// - `#[exo(short = 'x')]` — Add short flag alias
/// - `#[exo(positional)]` — Positional argument
/// - `#[exo(default_value = "...")]` — Default value
#[proc_macro_derive(ExoSpec, attributes(exo))]
pub fn derive_exo_spec(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    match parse::expand_exo_spec(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
