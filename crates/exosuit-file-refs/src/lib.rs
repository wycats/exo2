#![forbid(unsafe_code)]

mod model;
mod parse;
mod present;

pub use model::{FileRef, ParseError, PresentationTokens, Surface};
pub use parse::{normalize_slashes, parse_file_ref, workspace_relative_path};
pub use present::{present_file_ref, present_paths};

#[cfg(feature = "wasm")]
mod wasm;
