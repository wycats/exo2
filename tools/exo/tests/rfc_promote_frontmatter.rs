//! Regression test for promoting anchor-format RFCs without reintroducing frontmatter.

#![allow(clippy::assertions_on_constants)]
#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage, exo_rfc_create, fs};

#[test_matrix(["sqlite"])]
fn rfc_promote_preserves_anchor_format_without_frontmatter(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_rfc_create(
        root,
        "Anchor Format RFC",
        "0002",
        "1",
        "Core",
        Some("Body."),
    );

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "promote", "0002", "--stage", "2"])
        .assert()
        .success();

    let promoted_path = root.join("docs/rfcs/stage-2/0002-anchor-format-rfc.md");
    let promoted = ok_or_return!(
        fs::read_to_string(&promoted_path),
        "failed to read promoted"
    );

    assert!(
        promoted.starts_with("<!-- exo:2 ulid:"),
        "expected anchor comment"
    );
    assert!(
        !promoted.starts_with("---"),
        "frontmatter should not be reintroduced"
    );
    assert!(
        !promoted.contains("stage:"),
        "stage should stay directory-authoritative"
    );
}
