use crate::axiom::Axiom;
use crate::context::{AgentContext, SqliteWriter};
use crate::phase_owner;
use crate::project::Project;
use crate::templates;
use std::fs;
use std::io::IsTerminal;
use std::path::Path;

/// Represents an existing file that might contain agent context
#[derive(Debug)]
pub struct ExistingContextFile {
    /// Original path relative to repo root
    original_path: String,
    /// What kind of content this likely contains
    description: &'static str,
}

/// Scan for existing files that might contain agent context
fn detect_existing_context(root: &Path) -> Vec<ExistingContextFile> {
    let mut found = Vec::new();

    // Note: We intentionally don't touch .github/copilot-instructions.md
    // That's the user's space for project-specific instructions.
    // Exo installs its own instructions to .github/instructions/ instead.
    let patterns_with_desc: &[(&str, &str)] = &[
        ("AGENTS.md", "Agent instructions (Codex/Claude format)"),
        (
            "CONTRIBUTING.md",
            "Contribution guidelines (may have dev setup)",
        ),
        ("DEVELOPMENT.md", "Development documentation"),
        ("docs/CONTRIBUTING.md", "Contribution guidelines"),
        ("docs/DEVELOPMENT.md", "Development documentation"),
        (".cursor/rules", "Cursor AI rules"),
        (".cursorrules", "Cursor AI rules (legacy)"),
    ];

    for (pattern, desc) in patterns_with_desc {
        let path = root.join(pattern);
        if path.exists() {
            found.push(ExistingContextFile {
                original_path: pattern.to_string(),
                description: desc,
            });
        }
    }

    found
}

/// Move existing context files to bootstrap folder and create instructions
fn bootstrap_existing_context(
    root: &Path,
    existing: &[ExistingContextFile],
) -> Result<(), Box<dyn std::error::Error>> {
    if existing.is_empty() {
        return Ok(());
    }

    let bootstrap_dir = root.join("docs/agent-context/bootstrap");
    fs::create_dir_all(&bootstrap_dir)?;

    // Move each file
    for file in existing {
        let src = root.join(&file.original_path);
        // Flatten the path for the destination (replace / with -)
        let dest_name = file.original_path.replace('/', "-") + ".original";
        let dest = bootstrap_dir.join(&dest_name);

        fs::copy(&src, &dest)?;
        fs::remove_file(&src)?;

        println!("  Moved {} -> bootstrap/{}", file.original_path, dest_name);
    }

    // Create bootstrap instructions
    let mut instructions = String::from(
        r"# Bootstrap Context

This folder contains files that existed before `exo init` was run.
These files may contain valuable context that should be incorporated
into the new Exosuit structure.

## Original Files

",
    );

    for file in existing {
        let dest_name = file.original_path.replace('/', "-") + ".original";
        instructions.push_str(&format!(
            "- **{}** (was `{}`)\n  {}\n\n",
            dest_name, file.original_path, file.description
        ));
    }

    instructions.push_str(
        r"
## How to Incorporate

Review each file and consider:

1. **Project-specific instructions** → Add to `.github/copilot-instructions.md`
2. **Workflow/process rules** → Add as axioms via `exo axiom add`
3. **Development setup** → Keep in CONTRIBUTING.md or project README
4. **Design decisions** → Create RFCs via `exo rfc create`

Once you've incorporated the relevant content, you can delete this folder:
```
rm -rf docs/agent-context/bootstrap/
```
",
    );

    fs::write(bootstrap_dir.join("README.md"), instructions)?;

    Ok(())
}

pub fn run_init(
    context: &AgentContext,
    use_defaults: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = &context.root;

    // Detect existing context files
    let existing_context = detect_existing_context(cwd);

    // Check if directory has other content (beyond context files and .git)
    if !is_safe_to_initialize(cwd, &existing_context)? {
        return Err("Directory contains files beyond recognized context files. Please run 'exo init' in an empty directory or a directory with only agent context files.".into());
    }

    // Report existing context files if found
    if !existing_context.is_empty() {
        println!("\nFound existing agent context files:");
        for file in &existing_context {
            println!("  - {} ({})", file.original_path, file.description);
        }
        println!("\nThese will be moved to docs/agent-context/bootstrap/ for review.\n");
    }

    // Use defaults mode for testing/CI
    if use_defaults {
        println!(
            "Initializing Exosuit project with defaults in {}",
            cwd.display()
        );
        init_project(cwd, "Test", "strict", "Test", &existing_context)?;
        println!("Project initialized with defaults.");
        return Ok(());
    }

    if std::io::stdout().is_terminal() {
        let _theme = crate::ui::ThemeGuard::install_compact();

        let _ = cliclack::intro("Initialize Exosuit project");
        let _ = cliclack::log::info(format!("Directory: {}", cwd.display()));

        let mission: String = cliclack::input("Mission")
            .placeholder("What are we building?")
            .interact()?;

        let mode_idx = cliclack::select("Mode")
            .item("strict", "Strict", "TDD, high rigor")
            .item("loose", "Loose", "Prototyping, speed")
            .initial_value("strict")
            .interact()?;
        let mode = if mode_idx == "loose" {
            "loose"
        } else {
            "strict"
        };

        let first_step: String = cliclack::input("First Step")
            .placeholder("What is the first milestone?")
            .interact()?;

        let _ = cliclack::log::info("Generating project artifacts...");
        let _ = cliclack::log::remark(format!("Mission: {mission}"));
        let _ = cliclack::log::remark(format!("Mode: {mode}"));
        let _ = cliclack::log::remark(format!("First Step: {first_step}"));

        init_project(cwd, &mission, mode, &first_step, &existing_context)?;
        let _ = cliclack::outro("Project initialized.");
        return Ok(());
    }

    println!("Initializing Exosuit project in {}", cwd.display());

    // Fallback (non-tty): plain stdin/out.
    let mut mission = String::new();
    let mut first_step = String::new();
    let mut mode = String::new();

    println!("Mission: What are we building?");
    std::io::stdin().read_line(&mut mission)?;
    let mission = mission.trim().to_string();

    println!("Mode: How should I behave? (strict|loose)");
    std::io::stdin().read_line(&mut mode)?;
    let mode = mode.trim();
    let mode = if mode == "loose" { "loose" } else { "strict" };

    println!("First Step: What is the first milestone?");
    std::io::stdin().read_line(&mut first_step)?;
    let first_step = first_step.trim().to_string();

    println!("\nGenerating project artifacts...");
    println!("Mission: {mission}");
    println!("Mode: {mode}");
    println!("First Step: {first_step}");

    init_project(cwd, &mission, mode, &first_step, &existing_context)
}

/// Seed initial axioms into the `SQLite` database.
///
/// Creates the `.cache/` directory and database, inserts the core workflow
/// axioms plus the mode-specific axiom (strict or loose), then writes the
/// SQL dump for git persistence.
fn seed_axioms(
    cwd: &Path,
    project: Option<&Project>,
    mode: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Ensure .cache directory exists
    let db_path = crate::context::db_path(cwd, project);
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Core workflow axioms (always seeded)
    let mut axioms: Vec<(&str, Axiom)> = vec![
        (
            "workflow",
            Axiom {
                id: "context-is-king".into(),
                principle: "Context is King".into(),
                rationale: Some("We cannot build what we do not understand.".into()),
                implications: vec!["Always read the context before acting.".into()],
                notes: None,
                tags: vec!["core".into(), "workflow".into()],
            },
        ),
        (
            "workflow",
            Axiom {
                id: "phased-execution".into(),
                principle: "Phased Execution".into(),
                rationale: Some("Jumping ahead leads to chaos.".into()),
                implications: vec!["Finish the current phase before starting the next.".into()],
                notes: None,
                tags: vec!["workflow".into()],
            },
        ),
    ];

    // Mode-specific axiom
    if mode == "strict" {
        axioms.push((
            "workflow",
            Axiom {
                id: "strict-mode".into(),
                principle: "Strict Mode".into(),
                rationale: Some("We value correctness over speed.".into()),
                implications: vec!["Always write tests before code.".into()],
                notes: None,
                tags: vec!["workflow".into(), "mode".into(), "strict".into()],
            },
        ));
    } else {
        axioms.push((
            "workflow",
            Axiom {
                id: "loose-mode".into(),
                principle: "Loose Mode".into(),
                rationale: Some("We value speed and exploration.".into()),
                implications: vec!["It is okay to break things in the name of progress.".into()],
                notes: None,
                tags: vec!["workflow".into(), "mode".into(), "loose".into()],
            },
        ));
    }

    let writer = SqliteWriter::open(&db_path)?;
    for (scope, axiom) in axioms {
        writer.add_axiom(
            &axiom.id,
            scope,
            &axiom.principle,
            axiom.rationale.as_deref(),
            axiom.notes.as_deref(),
            &axiom.implications,
            &axiom.tags,
        )?;
    }
    crate::context::write_sql_dump_with_project(cwd, project);

    Ok(())
}

fn bootstrap_plan(cwd: &Path, project: Option<&Project>) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = crate::context::db_path(cwd, project);
    let writer = SqliteWriter::open(&db_path)?;

    let epoch_id = writer.add_epoch("Getting Started", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "Bootstrap", "regular", None, &[])?;
    writer.update_phase_status(&phase_id, "in-progress")?;
    if let Some(workspace_root) = project.and_then(|project| project.workspace_root.as_ref()) {
        writer.set_workspace_active_phase(&workspace_root.to_string_lossy(), &phase_id)?;
    }
    phase_owner::claim_phase_for_current_owner(cwd, project, &db_path, &phase_id, false)?;

    Ok(())
}

pub fn init_project(
    cwd: &Path,
    mission: &str,
    mode: &str,
    first_step: &str,
    existing_context: &[ExistingContextFile],
) -> Result<(), Box<dyn std::error::Error>> {
    if mode != "strict" && mode != "loose" {
        return Err("Invalid mode. Expected 'strict' or 'loose'.".into());
    }

    // Create durable document and configuration structure first. Exo state
    // projections are created later according to the active state policy.
    let dirs = vec![
        "docs/rfcs/stage-0",
        "docs/rfcs/stage-1",
        "docs/rfcs/stage-2",
        "docs/rfcs/stage-3",
        "docs/rfcs/stage-4",
        "docs/research",
        "docs/design",
        "docs/manual",
        ".github/prompts",
        ".config/exo",
    ];

    for dir in dirs {
        fs::create_dir_all(cwd.join(dir))?;
    }

    // Bootstrap existing context files (move to bootstrap folder)
    if !existing_context.is_empty() {
        bootstrap_existing_context(cwd, existing_context)?;
    }

    // Generate AGENTS.md
    let agents_md = templates::AGENTS_MD.replace("{{MISSION}}", mission);
    fs::write(cwd.join("AGENTS.md"), agents_md)?;

    // Seed axioms into SQLite
    let project = Project::resolve(cwd).ok();
    seed_axioms(cwd, project.as_ref(), mode)?;

    // Bootstrap plan: create a starter epoch and phase so the workspace
    // is immediately usable. The names are intentionally generic —
    // the agent should rename them once it understands the project.
    bootstrap_plan(cwd, project.as_ref())?;

    let _ = first_step;

    // RFC 0111: No readonly enforcement - agents are collaborators

    // Install .gitignore with exosuit patterns
    templates::install_gitignore(cwd)?;
    println!("Generated .gitignore with exosuit patterns");

    // Install .gitattributes with generated SQL dump merge policy.
    templates::install_gitattributes(cwd)?;
    let configured_sql_merge_driver = templates::configure_sql_dump_merge_driver(cwd)?;
    if configured_sql_merge_driver {
        println!("Configured SQL dump merge driver");
    }
    println!("Generated .gitattributes with SQL dump merge attributes");

    // Install default hooks.toml
    templates::install_hooks_toml(cwd)?;
    println!("Generated .config/exo/hooks.toml with sensible defaults");

    // Install project prompts
    let prompts_written = templates::install_project_prompts(cwd)?;
    println!("Installed/updated {prompts_written} prompt files into .github/prompts");

    // Install exo instruction files (separate from user's copilot-instructions.md)
    let instructions_written = templates::install_instructions(cwd)?;
    println!(
        "Installed/updated {instructions_written} instruction files into .github/instructions"
    );

    // Install global prompts to user config
    match templates::install_global_prompts() {
        Ok(global_written) if global_written > 0 => {
            println!(
                "Installed/updated {global_written} global prompts to ~/.config/Code/User/prompts/exo/"
            );
        }
        Ok(_) => {
            println!("Global prompts are up to date");
        }
        Err(e) => {
            eprintln!("Warning: Could not install global prompts: {e}");
        }
    }

    println!("\nProject initialized successfully!");

    if !existing_context.is_empty() {
        println!("\n📋 Bootstrap files are in docs/agent-context/bootstrap/");
        println!("   Ask your agent to review and incorporate them into the new structure.");
    }

    println!("Run 'exo context restore' to verify the setup.");

    Ok(())
}

/// Files that are allowed to exist but won't be moved (user's space)
const ALLOWED_USER_FILES: &[&str] = &[".github/copilot-instructions.md", "exosuit.toml"];

fn is_safe_to_initialize(
    path: &Path,
    existing_context: &[ExistingContextFile],
) -> Result<bool, std::io::Error> {
    // Build a set of allowed paths:
    // 1. Context files we'll move to bootstrap
    // 2. User files we'll leave alone
    let mut allowed: std::collections::HashSet<&str> = existing_context
        .iter()
        .map(|f| f.original_path.as_str())
        .collect();

    for user_file in ALLOWED_USER_FILES {
        allowed.insert(user_file);
    }

    // Also allow parent directories of allowed files
    let mut allowed_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();
    for path_str in &allowed {
        let mut current = std::path::Path::new(path_str);
        while let Some(parent) = current.parent() {
            if !parent.as_os_str().is_empty() {
                allowed_dirs.insert(parent.to_string_lossy().to_string());
            }
            current = parent;
        }
    }

    fn check_dir(
        dir: &Path,
        base: &Path,
        allowed: &std::collections::HashSet<&str>,
        allowed_dirs: &std::collections::HashSet<String>,
    ) -> Result<bool, std::io::Error> {
        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            // Always allow these
            if name == ".git" || name == "." || name == ".." {
                continue;
            }

            // Get relative path from base
            let rel_path = entry
                .path()
                .strip_prefix(base)
                .unwrap_or(&entry.path())
                .to_string_lossy()
                .to_string();

            // Check if this is an allowed file
            if allowed.contains(rel_path.as_str()) {
                continue;
            }

            // Check if this is a directory that contains allowed files
            if entry.file_type()?.is_dir() && allowed_dirs.contains(&rel_path) {
                // Recursively check this directory
                if !check_dir(&entry.path(), base, allowed, allowed_dirs)? {
                    return Ok(false);
                }
                continue;
            }

            // Found an unexpected file/directory
            return Ok(false);
        }
        Ok(true)
    }

    check_dir(path, path, &allowed, &allowed_dirs)
}
