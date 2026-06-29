#![allow(missing_docs)]

use exo::command_guidance::{
    CommandGuidance, CommandGuidanceKind, CommandGuidanceStatus,
    representative_command_guidance_inventory, validate_command_guidance,
};
use exo::command_reference::ExoCommandReference;
use exo::command_text::parse_command_text;
use exo::router::compile_argv;
use exo::steering::{SuggestedAction, WorkIntent};

#[test]
fn representative_exo_guidance_compiles_through_command_spec() {
    let mut failures = Vec::new();

    for guidance in representative_command_guidance_inventory() {
        let status = validate_command_guidance(&guidance);
        match guidance.kind {
            CommandGuidanceKind::ExoCli | CommandGuidanceKind::ExoRun => {
                if !status.is_valid() {
                    failures.push(format!(
                        "{} ({}) `{}` => {:?}",
                        guidance.id, guidance.surface, guidance.command, status
                    ));
                }
            }
            CommandGuidanceKind::HumanAction
            | CommandGuidanceKind::ExternalShell
            | CommandGuidanceKind::LegacyExoSurface => {
                assert!(
                    status.is_skipped(),
                    "{} should be classified outside strict CommandSpec validation: {:?}",
                    guidance.id,
                    status
                );
            }
        }
    }

    assert!(
        failures.is_empty(),
        "Command guidance should compile through CommandSpec:\n{}",
        failures.join("\n")
    );
}

#[test]
fn validation_catches_task_complete_message_drift() {
    let guidance = CommandGuidance::exo_run(
        "regression.task-complete-message",
        "command-shape inbox report",
        "task complete <id> --message $1",
    );

    let status = validate_command_guidance(&guidance);
    match status {
        CommandGuidanceStatus::Invalid { message, .. } => {
            assert!(
                message.contains("Unknown flag '--message'"),
                "expected unknown --message diagnostic, got {message}"
            );
        }
        other => panic!("expected invalid guidance, got {other:?}"),
    }
}

#[test]
fn validation_rejects_numbered_shell_placeholders_in_cli_guidance() {
    let guidance = CommandGuidance::exo_cli(
        "regression.cli-shell-placeholder",
        "terminal guidance",
        "exo goal complete <id> --log $1",
    );

    let status = validate_command_guidance(&guidance);
    match status {
        CommandGuidanceStatus::Invalid { message, .. } => {
            assert!(
                message.contains("copyable terminal text"),
                "expected copyability diagnostic, got {message}"
            );
        }
        other => panic!("expected invalid CLI guidance, got {other:?}"),
    }
}

#[test]
fn completion_confirmation_actions_are_not_treated_as_exo_commands() {
    let guidance = CommandGuidance::human_action(
        "completion.present-outcome",
        "completion review",
        "Present the proposed outcome for review.",
    );

    let status = validate_command_guidance(&guidance);
    match status {
        CommandGuidanceStatus::Skipped { reason } => {
            assert!(reason.contains("human action"));
        }
        other => panic!("expected skipped human action, got {other:?}"),
    }
}

#[test]
fn command_reference_renderings_compile_through_same_spec_path() {
    let reference = ExoCommandReference::new(&["task", "complete"])
        .positional_placeholder("id", "sample-task")
        .option_placeholder("log", "summary", "finished the task");

    assert_eq!(
        reference.render_cli(),
        "exo task complete <id> --log <summary>"
    );
    let cli_guidance = CommandGuidance::exo_cli(
        "reference.task-complete.cli",
        "typed command reference",
        "exo task complete <id> --log <summary>",
    );
    assert!(
        validate_command_guidance(&cli_guidance).is_valid(),
        "CLI rendering should compile"
    );

    let rendered = reference.render_exo_run();
    let parsed =
        parse_command_text(&rendered.command, &rendered.args).expect("exo-run text parses");
    let spec = exo::command::command_spec::CommandSpec::from_registry(
        &exo::command::registry::default_registry(),
    );
    let compiled = compile_argv(&spec, &parsed.tokens);
    assert!(
        compiled.invocation.is_some(),
        "exo-run rendering should compile: {:?}",
        compiled.diagnostics
    );
}

#[test]
fn suggested_action_serializes_rendered_command_from_reference() {
    let action = SuggestedAction::exo(
        "Complete task",
        ExoCommandReference::new(&["task", "complete"])
            .positional_placeholder("id", "sample-task")
            .option_placeholder("log", "summary", "finished the task"),
        "Record the completed work.",
        WorkIntent::Record,
        Some(0.9),
    );

    let value = serde_json::to_value(action).expect("serialize suggested action");
    assert_eq!(
        value.get("command").and_then(serde_json::Value::as_str),
        Some("exo task complete <id> --log <summary>")
    );
    assert_eq!(
        value.get("intent").and_then(serde_json::Value::as_str),
        Some("record")
    );
}

#[test]
fn migrated_core_guidance_surfaces_do_not_hand_write_exo_actions() {
    for (path, source) in [
        ("steering.rs", include_str!("../src/steering.rs")),
        ("failure.rs", include_str!("../src/failure.rs")),
        ("verify.rs", include_str!("../src/verify.rs")),
        (
            "completion_confirmation.rs",
            include_str!("../src/command/completion_confirmation.rs"),
        ),
        (
            "unified_diagnostics.rs",
            include_str!("../src/command/unified_diagnostics.rs"),
        ),
    ] {
        assert!(
            !source.contains("command: \"exo"),
            "{path} should render Exo-authored SuggestedAction commands from typed references"
        );
        assert!(
            !source.contains("command: format!(\"exo"),
            "{path} should render dynamic Exo-authored SuggestedAction commands from typed references"
        );
    }
}
