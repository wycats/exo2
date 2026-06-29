//! Integration tests for the update path from older project layouts.
//!
//! These tests verify that `exo update` preserves user content and applies
//! only the migrations still implemented by the current plugin set.

#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use exo::command::update::run_update;
use exo::context::{AgentContext, ExoState};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

fn dummy_context(root: &std::path::Path) -> AgentContext {
    let cache_dir = root.join(".cache");
    assert!(fs::create_dir_all(&cache_dir).is_ok());
    assert!(exosuit_storage::open_database(cache_dir.join("exo.db")).is_ok());

    AgentContext {
        root: root.to_path_buf(),
        project: None,
        plan: ExoState {
            meta: None,
            epochs: Vec::new(),
        },
    }
}

/// Helper to create a pre-v1 project fixture with all canonical TOML files.
fn setup_pre_v1_project(root: &std::path::Path) {
    // Create directory structure
    assert!(fs::create_dir_all(root.join("docs/agent-context/current")).is_ok());
    assert!(fs::create_dir_all(root.join("docs/agent-context")).is_ok());

    // 1. plan.toml
    assert!(
        fs::write(
            root.join("docs/agent-context/plan.toml"),
            r#"# Project Plan

[[epochs]]
id = "epoch-1"
title = "Epoch 1: Foundation"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Phase 1: Setup"
status = "active"

[[epochs.phases.tasks]]
id = "initial-setup"
label = "Initial Setup"
status = "pending"
"#,
        )
        .is_ok()
    );

    // 2. implementation-plan.toml
    assert!(
        fs::write(
            root.join("docs/agent-context/current/implementation-plan.toml"),
            r#"# READ-ONLY: Use 'exo' CLI to modify this file.
# Implementation Plan

[phase]
id = "phase-1"
title = "Phase 1: Setup"

[plan]

[[plan.goals]]
name = "First Step"
type = "feat"
details = "Initial setup"
files = []
tests = []

[verification]
automated = []
manual = []
"#,
        )
        .is_ok()
    );

    // 3. ideas.toml
    assert!(
        fs::write(
            root.join("docs/agent-context/ideas.toml"),
            r#"[[ideas]]
id = "test-idea-1"
title = "Test Idea"
description = "A test idea for upgrade testing"
status = "new"
created_at = "2025-01-01T00:00:00Z"
source = "user"
tags = []
related_tasks = []
"#,
        )
        .is_ok()
    );

    // 4. axioms.workflow.toml
    assert!(
        fs::write(
            root.join("docs/agent-context/axioms.workflow.toml"),
            r#"# READ-ONLY: Use 'exo' CLI to modify this file.
# Workflow Axioms

[[axioms]]
id = "context-is-king"
principle = "Context is King"
rationale = "AI agents need structured context"
implications = ["Read context before acting"]
tags = ["core", "workflow"]
"#,
        )
        .is_ok()
    );

    // 5. axioms.system.toml
    assert!(
        fs::write(
            root.join("docs/agent-context/axioms.system.toml"),
            r#"# READ-ONLY: Use 'exo' CLI to modify this file.
# System Axioms

[[axioms]]
id = "test-system-axiom"
principle = "Test System Axiom"
rationale = "For testing purposes"
implications = []
tags = []
"#,
        )
        .is_ok()
    );

    // 6. axioms.design.toml
    assert!(
        fs::write(
            root.join("docs/agent-context/axioms.design.toml"),
            r#"# READ-ONLY: Use 'exo' CLI to modify this file.
# Design Axioms

[[axioms]]
id = "test-design-axiom"
principle = "Test Design Axiom"
rationale = "For testing purposes"
implications = []
tags = []
"#,
        )
        .is_ok()
    );

    // 7. council.toml
    assert!(
        fs::write(
            root.join("docs/agent-context/council.toml"),
            r#"# Council Configuration

[[members]]
id = "member-1"
name = "Test Member"
role = "reviewer"
"#,
        )
        .is_ok()
    );

    // 8. modes.toml
    assert!(
        fs::write(
            root.join("docs/agent-context/modes.toml"),
            r#"# Collaboration Modes

[[modes]]
id = "thinking-partner"
name = "Thinking Partner"
description = "Exploration and design"
"#,
        )
        .is_ok()
    );

    // 9. decisions.toml
    assert!(
        fs::write(
            root.join("docs/agent-context/decisions.toml"),
            r#"# Decision Log

[[decisions]]
id = "decision-1"
title = "Test Decision"
status = "approved"
context = "Testing upgrade path"
implications = []
"#,
        )
        .is_ok()
    );

    // 10. prompts.toml
    assert!(
        fs::write(
            root.join("docs/agent-context/prompts.toml"),
            r#"# Prompt Catalog

[[prompts]]
id = "test-prompt"
name = "Test Prompt"
content = "Test prompt content"
tags = []
"#,
        )
        .is_ok()
    );
}

/// Helper to verify a file exists and preserves expected content.
fn verify_file_contains(path: &std::path::Path, expected_content_substr: &str) {
    assert!(path.exists(), "File should exist: {}", path.display());

    let content = ok_or_return!(fs::read_to_string(path), "failed to read file");

    // Verify original content preserved
    assert!(
        content.contains(expected_content_substr),
        "Original content should be preserved in {}: looking for '{}' in:\n{}",
        path.display(),
        expected_content_substr,
        content
    );
}

#[test]
fn test_full_upgrade_path_pre_v1_to_v1() {
    let _backend = "toml";
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    // Setup: Create a complete pre-v1 project
    setup_pre_v1_project(root);

    // Execute: Run exo update
    let mut ctx = dummy_context(root);
    assert!(run_update(&mut ctx).is_ok(), "exo update should succeed");

    // Verify: All canonical files still exist and preserve their core content.

    // 1. plan.toml
    verify_file_contains(&root.join("docs/agent-context/plan.toml"), "[[epochs]]");
    verify_file_contains(
        &root.join("docs/agent-context/plan.toml"),
        "id = \"epoch-1\"",
    );

    // 2. implementation-plan.toml
    verify_file_contains(
        &root.join("docs/agent-context/current/implementation-plan.toml"),
        "[phase]",
    );
    verify_file_contains(
        &root.join("docs/agent-context/current/implementation-plan.toml"),
        "id = \"phase-1\"",
    );

    // 3. ideas.toml
    verify_file_contains(&root.join("docs/agent-context/ideas.toml"), "[[ideas]]");
    verify_file_contains(
        &root.join("docs/agent-context/ideas.toml"),
        "id = \"test-idea-1\"",
    );

    // 4. axioms.workflow.toml
    verify_file_contains(
        &root.join("docs/agent-context/axioms.workflow.toml"),
        "[[axioms]]",
    );
    verify_file_contains(
        &root.join("docs/agent-context/axioms.workflow.toml"),
        "id = \"context-is-king\"",
    );

    // 5. axioms.system.toml
    verify_file_contains(
        &root.join("docs/agent-context/axioms.system.toml"),
        "id = \"test-system-axiom\"",
    );

    // 6. axioms.design.toml
    verify_file_contains(
        &root.join("docs/agent-context/axioms.design.toml"),
        "id = \"test-design-axiom\"",
    );

    // 7. council.toml
    verify_file_contains(&root.join("docs/agent-context/council.toml"), "[[members]]");

    // 8. modes.toml
    verify_file_contains(&root.join("docs/agent-context/modes.toml"), "[[modes]]");

    // 9. decisions.toml
    verify_file_contains(
        &root.join("docs/agent-context/decisions.toml"),
        "[[decisions]]",
    );

    // 10. prompts.toml
    verify_file_contains(&root.join("docs/agent-context/prompts.toml"), "[[prompts]]");
}

#[test]
fn test_upgrade_is_idempotent() {
    let _backend = "toml";
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    setup_pre_v1_project(root);

    let mut ctx = dummy_context(root);

    // First run: should apply changes
    assert!(run_update(&mut ctx).is_ok());

    // Read content after first run
    let plan_after_first = ok_or_return!(
        fs::read_to_string(root.join("docs/agent-context/plan.toml")),
        "failed to read plan.toml after first update"
    );

    // Second run: should be no-op
    assert!(run_update(&mut ctx).is_ok());

    // Read content after second run
    let plan_after_second = ok_or_return!(
        fs::read_to_string(root.join("docs/agent-context/plan.toml")),
        "failed to read plan.toml after second update"
    );

    // Content should be identical
    assert_eq!(
        plan_after_first, plan_after_second,
        "Second update should not modify files"
    );
}

#[test]
fn test_bootstrap_scaffolding_upgrade_is_idempotent() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    let global_git_config = root.join("empty-global-gitconfig");
    assert!(fs::write(&global_git_config, "").is_ok());

    let output = ok_or_return!(
        git_command(root, &global_git_config, ["init"]),
        "run git init"
    );
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    setup_pre_v1_project(root);
    assert!(fs::write(root.join("exosuit.toml"), "# Exosuit project marker").is_ok());

    let first = ok_or_return!(
        exo_update_json(root, &global_git_config),
        "first update command should run"
    );
    let first_stdout = String::from_utf8_lossy(&first.stdout);
    assert!(
        first.status.success(),
        "first update should succeed\nstdout: {first_stdout}\nstderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_json = ok_or_return!(
        serde_json::from_slice::<serde_json::Value>(&first.stdout),
        "first update stdout should be JSON"
    );
    let first_applied = applied_plugin_ids(&first_json);
    assert!(
        first_applied.contains(&"ensure-gitattributes-v1")
            && first_applied.contains(&"ensure-gitignore-v1"),
        "first update should ensure bootstrap scaffolding\nstdout: {first_stdout}"
    );
    let gitattributes = ok_or_return!(
        fs::read_to_string(root.join(".gitattributes")),
        "read generated .gitattributes"
    );
    assert!(
        gitattributes
            .lines()
            .any(|line| line.trim() == "docs/agent-context/*.sql merge=exo-sql-dump"),
        "first update should add SQL dump merge attribute\n{gitattributes}"
    );
    let merge_driver = ok_or_return!(
        git_command(
            root,
            &global_git_config,
            ["config", "--local", "--get", "merge.exo-sql-dump.driver"]
        ),
        "read SQL dump merge driver config"
    );
    assert!(
        merge_driver.status.success()
            && String::from_utf8_lossy(&merge_driver.stdout).trim() == "true",
        "first update should configure SQL dump merge driver\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&merge_driver.stdout),
        String::from_utf8_lossy(&merge_driver.stderr)
    );

    let second = ok_or_return!(
        exo_update_json(root, &global_git_config),
        "second update command should run"
    );
    let second_stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        second.status.success(),
        "second update should succeed\nstdout: {second_stdout}\nstderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_json = ok_or_return!(
        serde_json::from_slice::<serde_json::Value>(&second.stdout),
        "second update stdout should be JSON"
    );
    let second_applied_count = second_json
        .pointer("/result/applied_count")
        .and_then(serde_json::Value::as_u64);
    let second_applied = applied_plugin_ids(&second_json);
    assert!(
        second_applied_count == Some(0)
            && !second_applied.contains(&"ensure-gitattributes-v1")
            && !second_applied.contains(&"ensure-gitignore-v1"),
        "second update should not reapply bootstrap scaffolding\nstdout: {second_stdout}"
    );
}

fn git_command<const N: usize>(
    root: &Path,
    global_git_config: &Path,
    args: [&str; N],
) -> std::io::Result<std::process::Output> {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", global_git_config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
}

fn exo_update_json(root: &Path, global_git_config: &Path) -> std::io::Result<std::process::Output> {
    Command::new(env!("CARGO_BIN_EXE_exo"))
        .args(["--direct", "--format", "json", "update"])
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", global_git_config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
}

fn applied_plugin_ids(update_json: &serde_json::Value) -> Vec<&str> {
    update_json
        .pointer("/result/applied")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|report| report.get("plugin_id"))
        .filter_map(serde_json::Value::as_str)
        .collect()
}

#[test]
fn test_upgrade_preserves_comments_in_ideas() {
    let _backend = "toml";
    // Test that AddSchemaVersionsPlugin preserves comments.
    // We use ideas.toml because it's not affected by migrate-plan-ids or other
    // plugins that rewrite from in-memory structs.
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    assert!(fs::create_dir_all(root.join("docs/agent-context")).is_ok());

    // Create a file with important comments
    let ideas_with_comments = r#"# Ideas Backlog
# This is an important comment

# First idea section
[[ideas]]
id = "test-idea"
title = "Test Idea"
description = "A test idea"
status = "new"
created_at = "2025-01-01T00:00:00Z"
source = "user"
tags = []
related_tasks = []
"#;

    assert!(
        fs::write(
            root.join("docs/agent-context/ideas.toml"),
            ideas_with_comments
        )
        .is_ok()
    );

    let mut ctx = dummy_context(root);
    assert!(run_update(&mut ctx).is_ok());

    let updated_content = ok_or_return!(
        fs::read_to_string(root.join("docs/agent-context/ideas.toml")),
        "failed to read updated ideas.toml"
    );

    // Comments should be preserved (toml_edit preserves them)
    assert!(
        updated_content.contains("# This is an important comment"),
        "Important comment should be preserved in:\n{}",
        updated_content
    );
    assert!(
        updated_content.contains("# First idea section"),
        "First idea section comment should be preserved"
    );
}

#[test]
fn test_upgrade_handles_minimal_files() {
    let _backend = "toml";
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    assert!(fs::create_dir_all(root.join("docs/agent-context")).is_ok());

    // Create a minimal file with just an empty array
    assert!(fs::write(root.join("docs/agent-context/ideas.toml"), "ideas = []\n",).is_ok());

    let mut ctx = dummy_context(root);
    assert!(run_update(&mut ctx).is_ok());

    let content = ok_or_return!(
        fs::read_to_string(root.join("docs/agent-context/ideas.toml")),
        "failed to read updated ideas.toml"
    );

    assert!(content.contains("ideas = []"));
}
