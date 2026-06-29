//! Marker inventories for detecting "source markers" in a repository.
//!
//! This file is intended to be *data-only* and can be refreshed from upstream.
//!
//! Upstream:
//! - repo: <https://github.com/moonrepo/moon>
//! - path: crates/toolchain/src/detect/languages.rs
//! - pinned: bc521162a3478aa9c8b5907324454bcc838a705d
//!
//! Update:
//! - run: `cargo run -p exo-workspace --bin sync_moon_language_markers` (optionally `--sha <sha>`)

pub type MarkerList = &'static [&'static str];

pub const UPSTREAM_REPO: &str = "https://github.com/moonrepo/moon";
pub const UPSTREAM_PATH: &str = "crates/toolchain/src/detect/languages.rs";
pub const UPSTREAM_SHA: &str = "bc521162a3478aa9c8b5907324454bcc838a705d";

pub const BUN: MarkerList = &["bunfig.toml", "bun.lock", "bun.lockb", ".bunrc"];

pub const DENO: MarkerList = &["deno.json", "deno.jsonc", "deno.lock", ".dvmrc"];

pub const GO: MarkerList = &[
    "go.mod",
    "go.sum",
    "go.work",
    "go.work.sum",
    "g.lock",
    ".gvmrc",
    ".go-version",
];

pub const NODE: MarkerList = &[
    "package.json",
    ".nvmrc",
    ".node-version",
    "package-lock.json",
    ".npmrc",
    ".pnpmfile.cjs",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "yarn.lock",
    ".yarn",
    ".yarnrc",
    ".yarnrc.yml",
];

pub const PHP: MarkerList = &[
    "composer.json",
    "composer.lock",
    ".phpenv-version",
    ".phpbrewrc",
];

pub const PYTHON: MarkerList = &[
    "requirements.txt",
    "constraints.txt",
    "pyproject.toml",
    ".pylock.toml",
    ".python-version",
    ".venv",
    "Pipfile",
    "Pipfile.lock",
    "poetry.toml",
    "poetry.lock",
    "uv.toml",
    "uv.lock",
];

pub const RUBY: MarkerList = &["Gemfile", "Gemfile.lock", ".bundle", ".ruby-version"];

pub const RUST: MarkerList = &[
    "Cargo.toml",
    "Cargo.lock",
    ".cargo",
    "rust-toolchain.toml",
    "rust-toolchain",
];

pub const TYPESCRIPT: MarkerList = &["tsconfig.json", "tsconfig.tsbuildinfo"];
