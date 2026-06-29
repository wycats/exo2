//! Integration test: `exo update` migrates legacy unanchored RFC metadata.

#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use exo::command::update::run_update;
use exo::context::{AgentContext, ExoState, SqliteLoader, SqliteWriter};
use std::path::Path;
use std::process::Command;
use test_support::fs;

fn git_init(root: &Path) {
    let output = Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .expect("run git init");

    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn update_migrates_unanchored_rfc_after_tolerant_reconciliation() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    assert!(
        fs::write(
            root.join("exosuit.toml"),
            "[storage]\nbackend = \"sqlite\"\n"
        )
        .is_ok()
    );

    let rfc_path = root.join("docs/rfcs/stage-1/0001-legacy-rfc.md");
    assert!(fs::create_dir_all(rfc_path.parent().unwrap()).is_ok());
    assert!(
        fs::write(
            &rfc_path,
            "---\ntitle: Legacy RFC\nulid: 01H00000000000000000000000\n---\n\n# RFC 0001: Legacy RFC\n\nA legacy RFC without an anchor.\n",
        )
        .is_ok()
    );

    let pre_update = AgentContext::load(root.to_path_buf())
        .expect("unanchored legacy RFC should load as repair debt before update");
    assert_eq!(pre_update.plan.epochs.len(), 0);

    let mut ctx = AgentContext {
        root: root.to_path_buf(),
        project: None,
        plan: ExoState::default(),
    };

    run_update(&mut ctx).expect("update should migrate legacy RFC metadata");

    let migrated = ok_or_return!(fs::read_to_string(&rfc_path), "failed to read migrated RFC");
    assert!(migrated.starts_with("<!-- exo:1 ulid:01h00000000000000000000000 -->"));

    let loaded = AgentContext::load(root.to_path_buf()).expect("context should load after update");
    assert_eq!(loaded.plan.epochs.len(), 0);
}

#[test]
fn update_refuses_non_workspace_without_creating_database() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    let assert = assert_cmd::cargo::cargo_bin_cmd!("exo")
        .current_dir(root)
        .args(["--direct", "--format", "json", "update"])
        .assert()
        .failure();

    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("no exosuit.toml") || stderr.contains("no exosuit.toml"),
        "expected workspace guard error, stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!root.join(".cache/exo.db").exists());
}

#[test]
fn run_update_resolves_project_before_choosing_database() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);

    assert!(
        fs::write(
            root.join("exosuit.toml"),
            "[storage]\nbackend = \"sqlite\"\n"
        )
        .is_ok()
    );

    let rfc_path = root.join("docs/rfcs/stage-1/0001-legacy-rfc.md");
    assert!(fs::create_dir_all(rfc_path.parent().unwrap()).is_ok());
    assert!(
        fs::write(
            &rfc_path,
            "---\ntitle: Legacy RFC\nulid: 01H00000000000000000000000\n---\n\n# RFC 0001: Legacy RFC\n\nA legacy RFC without an anchor.\n",
        )
        .is_ok()
    );

    let mut ctx = AgentContext {
        root: root.to_path_buf(),
        project: None,
        plan: ExoState::default(),
    };

    run_update(&mut ctx).expect("update should use resolved project storage");

    assert!(root.join(".exo/cache/exo.db").exists());
    assert!(!root.join(".cache/exo.db").exists());
}

#[test]
fn update_imports_sql_projection_before_upgrades() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    assert!(
        fs::write(
            root.join("exosuit.toml"),
            "[storage]\nbackend = \"sqlite\"\n"
        )
        .is_ok()
    );

    let db_path = root.join(".cache/exo.db");
    assert!(fs::create_dir_all(db_path.parent().unwrap()).is_ok());
    let writer = ok_or_return!(SqliteWriter::open(&db_path), "failed to create source db");
    assert!(writer.add_epoch("Dump Projection Epoch", None, &[]).is_ok());
    exo::context::write_sql_dump_with_project(root, None);
    assert!(fs::metadata(root.join("docs/agent-context/epochs.sql")).is_ok());
    assert!(std::fs::remove_file(&db_path).is_ok());

    let mut ctx = AgentContext {
        root: root.to_path_buf(),
        project: None,
        plan: ExoState::default(),
    };

    run_update(&mut ctx).expect("update should import SQL projection before upgrades");

    assert!(
        ctx.plan
            .epochs
            .iter()
            .any(|epoch| epoch.title == "Dump Projection Epoch")
    );
}

#[test]
fn update_canonicalizes_frontmatter_ulids_before_upsert() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    assert!(
        fs::write(
            root.join("exosuit.toml"),
            "[storage]\nbackend = \"sqlite\"\n"
        )
        .is_ok()
    );

    let db_path = root.join(".cache/exo.db");
    assert!(fs::create_dir_all(db_path.parent().unwrap()).is_ok());
    let writer = ok_or_return!(SqliteWriter::open(&db_path), "failed to create source db");
    assert!(
        writer
            .upsert_rfc(
                "01h00000000000000000000000",
                1,
                "Legacy RFC",
                1,
                "active",
                Some("preserved-feature"),
                "legacy-rfc",
                "docs/rfcs/stage-1/0001-legacy-rfc.md",
                None,
                Some("0000"),
                None,
                None,
                None,
            )
            .is_ok()
    );

    let rfc_path = root.join("docs/rfcs/stage-1/0001-legacy-rfc.md");
    assert!(fs::create_dir_all(rfc_path.parent().unwrap()).is_ok());
    assert!(
            fs::write(
                &rfc_path,
                "---\ntitle: Legacy RFC\nulid: 01H00000000000000000000000\n---\n\n# RFC 0001: Legacy RFC\n\nA legacy RFC without an anchor.\n",
            )
            .is_ok()
        );

    let mut ctx = AgentContext {
        root: root.to_path_buf(),
        project: None,
        plan: ExoState::default(),
    };

    run_update(&mut ctx).expect("update should canonicalize frontmatter ULIDs");

    let migrated = ok_or_return!(fs::read_to_string(&rfc_path), "failed to read migrated RFC");
    assert!(migrated.starts_with("<!-- exo:1 ulid:01h00000000000000000000000 -->"));
    assert!(migrated.contains("- **Supersedes**: RFC 0000"));

    let loader = ok_or_return!(SqliteLoader::open(&db_path), "failed to open migrated db");
    let rfcs = ok_or_return!(loader.load_rfcs(), "failed to load migrated RFCs");
    assert_eq!(rfcs.len(), 1);
    let rfc = &rfcs[0];
    assert_eq!(rfc.text_id, "01h00000000000000000000000");
    assert_eq!(rfc.feature.as_deref(), Some("preserved-feature"));
    assert_eq!(rfc.supersedes.as_deref(), Some("0000"));
}
