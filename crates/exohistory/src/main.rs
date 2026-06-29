#![allow(missing_docs)]
#![allow(clippy::print_stdout, clippy::print_stderr)]
#![allow(clippy::disallowed_methods)] // CLI tool uses blocking I/O
#![allow(clippy::similar_names)] // session_a_id/session_b_id are intentionally similar
#![allow(clippy::cast_possible_wrap)] // CLI tool, safe casts
#![allow(clippy::cast_precision_loss)] // Acceptable for display purposes

mod analysis;
mod output;
mod session;
mod storage;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::analysis::{AnalysisConfig, SessionAnalyzer};
use crate::output::OutputFormatter;
use crate::storage::ChatStorageLocator;

#[derive(Parser)]
#[command(
    author,
    version,
    about = "Index and analyze VSCode Copilot chat history"
)]
#[command(long_about = "
exohistory - A tool for mining insights from VSCode Copilot chat sessions.

Dimensions of analysis:
  - Bug Detection: Find patterns in failed tool calls, error responses
  - Communication Mining: Analyze prompt patterns, response quality
  - Protocol Robustness: Track tool invocation success/failure rates
")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index chat sessions from `VSCode` workspace storage
    Index {
        /// Custom storage path (defaults to `VSCode` workspace storage)
        #[arg(long)]
        storage_path: Option<PathBuf>,

        /// Filter by workspace (partial match on path)
        #[arg(long)]
        workspace: Option<String>,

        /// Output format
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },

    /// Show session statistics (duration, request counts)
    Stats {
        /// Custom storage path
        #[arg(long)]
        storage_path: Option<PathBuf>,

        /// Filter by workspace (partial match on path)
        #[arg(long)]
        workspace: Option<String>,

        /// Output format
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },

    /// Analyze sessions for patterns
    Analyze {
        /// Session ID or 'all' for all sessions
        #[arg(default_value = "all")]
        session: String,

        /// Custom storage path
        #[arg(long)]
        storage_path: Option<PathBuf>,

        /// Analysis focus: bugs, patterns, tools, or all
        #[arg(long, value_enum, default_value = "all")]
        focus: AnalysisFocus,

        /// Output format
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },

    /// Search sessions by content
    Search {
        /// Search pattern (regex)
        pattern: String,

        /// Custom storage path
        #[arg(long)]
        storage_path: Option<PathBuf>,

        /// Search in user prompts only
        #[arg(long)]
        prompts_only: bool,

        /// Search in AI responses only
        #[arg(long)]
        responses_only: bool,

        /// Output format
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,

        /// Maximum number of results
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    /// Show tool usage statistics
    Tools {
        /// Custom storage path
        #[arg(long)]
        storage_path: Option<PathBuf>,

        /// Filter by tool name
        #[arg(long)]
        tool: Option<String>,

        /// Show failed invocations only
        #[arg(long)]
        failures_only: bool,

        /// Output format
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },

    /// Show session details
    Show {
        /// Session ID
        session_id: String,

        /// Custom storage path
        #[arg(long)]
        storage_path: Option<PathBuf>,

        /// Output format
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },

    /// Detect loop/retry patterns in sessions
    Loops {
        /// Custom storage path
        #[arg(long)]
        storage_path: Option<PathBuf>,

        /// Filter by workspace (partial match on path)
        #[arg(long)]
        workspace: Option<String>,

        /// Minimum repetitions to report as a loop
        #[arg(long, default_value = "3")]
        min_repetitions: usize,

        /// Output format
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },

    /// Extract code blocks from sessions
    Code {
        /// Custom storage path
        #[arg(long)]
        storage_path: Option<PathBuf>,

        /// Filter by workspace (partial match on path)
        #[arg(long)]
        workspace: Option<String>,

        /// Filter by language (e.g., rust, typescript)
        #[arg(long)]
        language: Option<String>,

        /// Minimum lines in code block
        #[arg(long, default_value = "5")]
        min_lines: usize,

        /// Maximum results
        #[arg(long, default_value = "50")]
        limit: usize,

        /// Output format
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },

    /// Get recent conversation turns from the most recent session
    Recent {
        /// Number of recent turns to retrieve (default: 10, max: 50)
        #[arg(long, default_value = "10")]
        turns: usize,

        /// Custom storage path
        #[arg(long)]
        storage_path: Option<PathBuf>,

        /// Filter by workspace name (partial match on final path component)
        #[arg(long)]
        workspace: Option<String>,

        /// Exact workspace URI (overrides --workspace; matches VS Code storage exactly)
        #[arg(long)]
        workspace_uri: Option<String>,

        /// Text snippet to match in user messages (identifies the correct session)
        #[arg(long)]
        match_text: Option<String>,

        /// Include extended thinking content
        #[arg(long)]
        include_thinking: bool,

        /// Include tool invocations
        #[arg(long)]
        include_tools: bool,

        /// Get turns before the last summarization (context that was just compacted)
        #[arg(long)]
        before_summary: bool,

        /// Output format (always json for LM tool consumption)
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },

    /// Compare two sessions side-by-side
    Diff {
        /// First session ID
        session_a: String,

        /// Second session ID
        session_b: String,

        /// Custom storage path
        #[arg(long)]
        storage_path: Option<PathBuf>,
    },

    /// Detect user intervention patterns (corrections, redirections)
    Interventions {
        /// Custom storage path
        #[arg(long)]
        storage_path: Option<PathBuf>,

        /// Filter by workspace (partial match on path)
        #[arg(long)]
        workspace: Option<String>,

        /// Maximum results
        #[arg(long, default_value = "50")]
        limit: usize,

        /// Output format
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum AnalysisFocus {
    /// Detect bugs and errors in sessions
    Bugs,
    /// Analyze communication patterns
    Patterns,
    /// Tool invocation analysis
    Tools,
    /// All analysis types
    All,
}

fn main() -> Result<()> {
    exo_reexec::maybe_reexec();

    let cli = Cli::parse();

    match cli.command {
        Commands::Index {
            storage_path,
            workspace,
            format,
        } => run_index(storage_path, workspace.as_deref(), format),

        Commands::Stats {
            storage_path,
            workspace,
            format,
        } => run_stats(storage_path, workspace.as_deref(), format),

        Commands::Analyze {
            session,
            storage_path,
            focus,
            format,
        } => run_analyze(&session, storage_path, focus, format),

        Commands::Search {
            pattern,
            storage_path,
            prompts_only,
            responses_only,
            format,
            limit,
        } => run_search(
            &pattern,
            storage_path,
            prompts_only,
            responses_only,
            format,
            limit,
        ),

        Commands::Tools {
            storage_path,
            tool,
            failures_only,
            format,
        } => run_tools(storage_path, tool.as_deref(), failures_only, format),

        Commands::Show {
            session_id,
            storage_path,
            format,
        } => run_show(&session_id, storage_path, format),

        Commands::Loops {
            storage_path,
            workspace,
            min_repetitions,
            format,
        } => run_loops(storage_path, workspace.as_deref(), min_repetitions, format),

        Commands::Code {
            storage_path,
            workspace,
            language,
            min_lines,
            limit,
            format,
        } => run_code(
            storage_path,
            workspace.as_deref(),
            language.as_deref(),
            min_lines,
            limit,
            format,
        ),

        Commands::Diff {
            session_a,
            session_b,
            storage_path,
        } => run_diff(&session_a, &session_b, storage_path),

        Commands::Interventions {
            storage_path,
            workspace,
            limit,
            format,
        } => run_interventions(storage_path, workspace.as_deref(), limit, format),

        Commands::Recent {
            turns,
            storage_path,
            workspace,
            workspace_uri,
            match_text,
            include_thinking,
            include_tools,
            before_summary,
            format,
        } => run_recent(
            turns,
            storage_path,
            workspace.as_deref(),
            workspace_uri.as_deref(),
            match_text.as_deref(),
            include_thinking,
            include_tools,
            before_summary,
            format,
        ),
    }
}

fn run_index(
    storage_path: Option<PathBuf>,
    workspace_filter: Option<&str>,
    format: OutputFormat,
) -> Result<()> {
    let locator = ChatStorageLocator::new(storage_path)?;
    let sessions = locator.discover_sessions(workspace_filter)?;

    let formatter = OutputFormatter::new(format == OutputFormat::Json);
    formatter.print_session_index(&sessions)?;

    Ok(())
}

fn run_analyze(
    session_filter: &str,
    storage_path: Option<PathBuf>,
    focus: AnalysisFocus,
    format: OutputFormat,
) -> Result<()> {
    let locator = ChatStorageLocator::new(storage_path)?;
    let sessions = if session_filter == "all" {
        locator.discover_sessions(None)?
    } else {
        locator.find_session(session_filter)?
    };

    let config = AnalysisConfig {
        analyze_bugs: matches!(focus, AnalysisFocus::Bugs | AnalysisFocus::All),
        analyze_patterns: matches!(focus, AnalysisFocus::Patterns | AnalysisFocus::All),
        analyze_tools: matches!(focus, AnalysisFocus::Tools | AnalysisFocus::All),
    };

    let analyzer = SessionAnalyzer::new(config);
    let results = analyzer.analyze_sessions(&sessions)?;

    let formatter = OutputFormatter::new(format == OutputFormat::Json);
    formatter.print_analysis(&results)?;

    Ok(())
}

fn run_search(
    pattern: &str,
    storage_path: Option<PathBuf>,
    prompts_only: bool,
    responses_only: bool,
    format: OutputFormat,
    limit: usize,
) -> Result<()> {
    let locator = ChatStorageLocator::new(storage_path)?;
    let sessions = locator.discover_sessions(None)?;

    let regex =
        regex::Regex::new(pattern).with_context(|| format!("Invalid regex pattern: {pattern}"))?;

    let mut results = Vec::new();
    for session_ref in &sessions {
        let session = locator.load_session(session_ref)?;
        let matches = session.search(&regex, prompts_only, responses_only);
        results.extend(matches);
        if results.len() >= limit {
            results.truncate(limit);
            break;
        }
    }

    let formatter = OutputFormatter::new(format == OutputFormat::Json);
    formatter.print_search_results(&results)?;

    Ok(())
}

fn run_tools(
    storage_path: Option<PathBuf>,
    tool_filter: Option<&str>,
    failures_only: bool,
    format: OutputFormat,
) -> Result<()> {
    let locator = ChatStorageLocator::new(storage_path)?;
    let sessions = locator.discover_sessions(None)?;

    let mut tool_stats = analysis::ToolStats::new();
    for session_ref in &sessions {
        let session = locator.load_session(session_ref)?;
        tool_stats.collect_from_session(&session, tool_filter, failures_only);
    }

    let formatter = OutputFormatter::new(format == OutputFormat::Json);
    formatter.print_tool_stats(&tool_stats)?;

    Ok(())
}

fn run_show(session_id: &str, storage_path: Option<PathBuf>, format: OutputFormat) -> Result<()> {
    let locator = ChatStorageLocator::new(storage_path)?;
    let session_refs = locator.find_session(session_id)?;

    if session_refs.is_empty() {
        anyhow::bail!("Session not found: {session_id}");
    }

    let session = locator.load_session(&session_refs[0])?;

    let formatter = OutputFormatter::new(format == OutputFormat::Json);
    formatter.print_session_detail(&session)?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_recent(
    turns: usize,
    storage_path: Option<PathBuf>,
    workspace_filter: Option<&str>,
    workspace_uri: Option<&str>,
    match_text: Option<&str>,
    include_thinking: bool,
    include_tools: bool,
    before_summary: bool,
    format: OutputFormat,
) -> Result<()> {
    let locator = ChatStorageLocator::new(storage_path)?;

    // If exact workspace URI is provided, use it; otherwise fall back to partial name match
    let sessions = if let Some(uri) = workspace_uri {
        locator.discover_sessions_by_uri(uri)?
    } else {
        locator.discover_sessions(workspace_filter)?
    };

    // Find the matching session
    let target_session = if let Some(text) = match_text {
        // If match_text is provided, find the session containing that text
        let mut found = None;
        for session_ref in &sessions {
            if let Ok(session) = locator.load_session(session_ref) {
                // Search user messages for the match text
                let contains_text = session.requests.iter().any(|req| {
                    req.message
                        .as_ref()
                        .and_then(|m| m.text.as_ref())
                        .is_some_and(|t| t.contains(text))
                });
                if contains_text {
                    found = Some((session_ref.clone(), session));
                    break;
                }
            }
        }
        found.context("No session found containing the specified text")?
    } else {
        // Fall back to most recently active session, but check for ambiguity first
        #[allow(clippy::items_after_statements)]
        let formatter = OutputFormatter::new(format == OutputFormat::Json);

        // Check for ambiguous sessions (multiple sessions active within threshold)
        #[allow(clippy::items_after_statements)]
        const AMBIGUITY_THRESHOLD_SECS: i64 = 5;

        if sessions.len() > 1 {
            // Sort by last_message_at descending
            let mut sorted: Vec<_> = sessions.iter().collect();
            sorted.sort_by_key(|session| std::cmp::Reverse(session.last_message_at));

            if let (Some(first), Some(second)) = (
                sorted.first().and_then(|s| s.last_message_at),
                sorted.get(1).and_then(|s| s.last_message_at),
            ) {
                let diff_secs = (first - second).num_seconds().abs();
                if diff_secs <= AMBIGUITY_THRESHOLD_SECS {
                    // Find all sessions within the threshold of the most recent
                    let candidates: Vec<_> = sorted
                        .iter()
                        .filter(|s| {
                            s.last_message_at.is_some_and(|t| {
                                (first - t).num_seconds().abs() <= AMBIGUITY_THRESHOLD_SECS
                            })
                        })
                        .copied()
                        .collect();

                    formatter.print_ambiguous_sessions(&candidates, AMBIGUITY_THRESHOLD_SECS)?;
                    return Ok(());
                }
            }
        }

        let most_recent = sessions
            .into_iter()
            .max_by_key(|s| s.last_message_at)
            .context("No sessions found")?;
        let session = locator.load_session(&most_recent)?;
        (most_recent, session)
    };

    let (session_ref, session) = target_session;

    // Cap turns at 50
    let turns = turns.min(50);

    let formatter = OutputFormatter::new(format == OutputFormat::Json);

    if before_summary {
        // Find the last message containing <conversation-summary> and return turns before it
        formatter.print_turns_before_summary(
            &session,
            &session_ref,
            turns,
            include_thinking,
            include_tools,
        )?;
    } else {
        formatter.print_recent_turns(
            &session,
            &session_ref,
            turns,
            include_thinking,
            include_tools,
        )?;
    }

    Ok(())
}

fn run_loops(
    storage_path: Option<PathBuf>,
    workspace_filter: Option<&str>,
    min_repetitions: usize,
    format: OutputFormat,
) -> Result<()> {
    use crate::analysis::LoopDetector;

    let locator = ChatStorageLocator::new(storage_path)?;
    let sessions = locator.discover_sessions(workspace_filter)?;

    let detector = LoopDetector::new(min_repetitions);
    let mut all_patterns = Vec::new();

    for session_ref in &sessions {
        if let Ok(session) = locator.load_session(session_ref) {
            let patterns = detector.detect(&session);
            all_patterns.extend(patterns);
        }
    }

    let formatter = OutputFormatter::new(format == OutputFormat::Json);
    formatter.print_loop_patterns(&all_patterns)?;

    Ok(())
}

fn run_stats(
    storage_path: Option<PathBuf>,
    workspace_filter: Option<&str>,
    format: OutputFormat,
) -> Result<()> {
    let locator = ChatStorageLocator::new(storage_path)?;
    let sessions = locator.discover_sessions(workspace_filter)?;

    let formatter = OutputFormatter::new(format == OutputFormat::Json);
    formatter.print_stats(&sessions)?;

    Ok(())
}

fn run_code(
    storage_path: Option<PathBuf>,
    workspace_filter: Option<&str>,
    language_filter: Option<&str>,
    min_lines: usize,
    limit: usize,
    format: OutputFormat,
) -> Result<()> {
    use crate::session::CodeBlock;

    let locator = ChatStorageLocator::new(storage_path)?;
    let sessions = locator.discover_sessions(workspace_filter)?;

    let mut all_blocks: Vec<CodeBlock> = Vec::new();

    for session_ref in &sessions {
        if let Ok(session) = locator.load_session(session_ref) {
            let blocks = session.extract_code_blocks();
            for block in blocks {
                // Apply filters
                if block.line_count < min_lines {
                    continue;
                }
                if let Some(lang) = language_filter
                    && block.language.as_deref() != Some(lang)
                {
                    continue;
                }
                all_blocks.push(block);
            }
        }
    }

    // Sort by line count descending
    all_blocks.sort_by_key(|block| std::cmp::Reverse(block.line_count));
    all_blocks.truncate(limit);

    let formatter = OutputFormatter::new(format == OutputFormat::Json);
    formatter.print_code_blocks(&all_blocks)?;

    Ok(())
}

fn run_diff(session_a_id: &str, session_b_id: &str, storage_path: Option<PathBuf>) -> Result<()> {
    use crate::analysis::classify_session_outcome;

    let locator = ChatStorageLocator::new(storage_path)?;

    let refs_a = locator.find_session(session_a_id)?;
    let refs_b = locator.find_session(session_b_id)?;

    if refs_a.is_empty() {
        anyhow::bail!("Session A not found: {session_a_id}");
    }
    if refs_b.is_empty() {
        anyhow::bail!("Session B not found: {session_b_id}");
    }

    let session_a = locator.load_session(&refs_a[0])?;
    let session_b = locator.load_session(&refs_b[0])?;

    let invocations_a = session_a.extract_tool_invocations();
    let invocations_b = session_b.extract_tool_invocations();

    let outcome_a = classify_session_outcome(&session_a);
    let outcome_b = classify_session_outcome(&session_b);

    // Count tool usage
    let mut tools_a: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut tools_b: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for inv in &invocations_a {
        *tools_a.entry(inv.tool_name.clone()).or_default() += 1;
    }
    for inv in &invocations_b {
        *tools_b.entry(inv.tool_name.clone()).or_default() += 1;
    }

    println!("\n🔄 Session Comparison");
    println!("════════════════════════════════════════════════════════════════\n");

    println!("                    Session A        Session B");
    println!("                    ─────────        ─────────");
    println!(
        "ID:                 {:16} {:16}",
        &session_a.session_id[..12],
        &session_b.session_id[..12]
    );
    println!(
        "Requests:           {:16} {:16}",
        session_a.requests.len(),
        session_b.requests.len()
    );
    println!(
        "Tool calls:         {:16} {:16}",
        invocations_a.len(),
        invocations_b.len()
    );
    println!("Outcome:            {outcome_a:16?} {outcome_b:?}");

    let success_a = invocations_a.iter().filter(|i| i.success).count();
    let success_b = invocations_b.iter().filter(|i| i.success).count();
    #[allow(clippy::cast_precision_loss)]
    let rate_a = if invocations_a.is_empty() {
        0.0
    } else {
        success_a as f64 / invocations_a.len() as f64 * 100.0
    };
    #[allow(clippy::cast_precision_loss)]
    let rate_b = if invocations_b.is_empty() {
        0.0
    } else {
        success_b as f64 / invocations_b.len() as f64 * 100.0
    };
    println!("Success rate:       {rate_a:15.1}% {rate_b:15.1}%");

    // Tool comparison
    println!("\n📊 Tool Usage Comparison");
    let all_tools: std::collections::HashSet<_> = tools_a.keys().chain(tools_b.keys()).collect();
    let mut all_tools: Vec<_> = all_tools.into_iter().collect();
    all_tools.sort();

    for tool in all_tools.iter().take(15) {
        let count_a = tools_a.get(*tool).copied().unwrap_or(0);
        let count_b = tools_b.get(*tool).copied().unwrap_or(0);
        if count_a > 0 || count_b > 0 {
            let diff = count_b as i64 - count_a as i64;
            let diff_str = if diff > 0 {
                format!("+{diff}")
            } else {
                diff.to_string()
            };
            println!("  {tool:30} {count_a:5} {count_b:5} ({diff_str})");
        }
    }

    Ok(())
}

fn run_interventions(
    storage_path: Option<PathBuf>,
    workspace_filter: Option<&str>,
    limit: usize,
    format: OutputFormat,
) -> Result<()> {
    use crate::analysis::{InterventionPattern, detect_interventions};

    let locator = ChatStorageLocator::new(storage_path)?;
    let sessions = locator.discover_sessions(workspace_filter)?;

    let mut all_patterns: Vec<InterventionPattern> = Vec::new();

    for session_ref in &sessions {
        // Skip very large files for performance
        let file_size = std::fs::metadata(&session_ref.path).map_or(0, |m| m.len());
        if file_size > 50 * 1024 * 1024 {
            continue;
        }

        if let Ok(session) = locator.load_session(session_ref) {
            let patterns = detect_interventions(&session);
            all_patterns.extend(patterns);
        }

        // Early exit if we have enough
        if all_patterns.len() >= limit {
            break;
        }
    }

    all_patterns.truncate(limit);

    let formatter = OutputFormatter::new(format == OutputFormat::Json);
    formatter.print_interventions(&all_patterns)?;

    Ok(())
}
