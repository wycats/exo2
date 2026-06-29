//! Output formatting for analysis results.

#![allow(clippy::cast_precision_loss)] // Acceptable for display formatting

use anyhow::Result;
use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL};
use std::cmp::Reverse;

use crate::analysis::{AnalysisResults, InterventionPattern, LoopPattern, ToolStats};
use crate::session::{ChatSession, CodeBlock, SearchMatch, SessionRef};

pub struct OutputFormatter {
    json_mode: bool,
}

impl OutputFormatter {
    pub const fn new(json_mode: bool) -> Self {
        Self { json_mode }
    }

    pub fn print_session_index(&self, sessions: &[SessionRef]) -> Result<()> {
        if self.json_mode {
            println!("{}", serde_json::to_string_pretty(sessions)?);
            return Ok(());
        }

        println!("\n📚 Chat Session Index");
        println!("════════════════════════════════════════════════════════════════\n");
        println!("Found {} sessions\n", sessions.len());

        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_content_arrangement(ContentArrangement::Dynamic);
        table.set_header(vec!["Session ID", "Workspace", "Requests", "Last Active"]);

        for session in sessions.iter().take(50) {
            let workspace = session.workspace_path.as_ref().map_or_else(
                || session.workspace_id.chars().take(8).collect(),
                |p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string()
                },
            );

            let last_active = session.last_message_at.map_or_else(
                || "unknown".to_string(),
                |dt| dt.format("%Y-%m-%d %H:%M").to_string(),
            );

            table.add_row(vec![
                short_id(&session.id),
                workspace,
                session.request_count.to_string(),
                last_active,
            ]);
        }

        println!("{table}");

        if sessions.len() > 50 {
            println!("\n... and {} more sessions", sessions.len() - 50);
        }

        Ok(())
    }

    pub fn print_analysis(&self, results: &AnalysisResults) -> Result<()> {
        if self.json_mode {
            println!("{}", serde_json::to_string_pretty(results)?);
            return Ok(());
        }

        println!("\n🔬 Analysis Results");
        println!("════════════════════════════════════════════════════════════════\n");

        // Summary
        println!("📊 Summary");
        println!("  Sessions analyzed: {}", results.total_sessions);
        println!("  Total requests: {}", results.total_requests);
        println!(
            "  Avg prompt length: {:.0} chars",
            results.avg_prompt_length
        );
        println!(
            "  Median prompt length: {} chars\n",
            results.median_prompt_length
        );

        // Session outcomes
        if !results.session_outcomes.is_empty() {
            println!("📈 Session Outcomes");
            for (outcome, count) in &results.session_outcomes {
                let icon = match outcome {
                    crate::analysis::SessionOutcome::Successful => "✅",
                    crate::analysis::SessionOutcome::ErrorTerminated => "❌",
                    crate::analysis::SessionOutcome::Abandoned => "🚪",
                    crate::analysis::SessionOutcome::Indeterminate => "❓",
                };
                let pct = *count as f64 / results.total_sessions as f64 * 100.0;
                println!("  {icon} {outcome:?}: {count} ({pct:.1}%)");
            }
            println!();
        }

        // Bug indicators
        if !results.bug_indicators.is_empty() {
            println!("🐛 Bug Indicators ({})", results.bug_indicators.len());
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);
            table.set_header(vec!["Session", "Type", "Description"]);

            for bug in results.bug_indicators.iter().take(20) {
                table.add_row(vec![
                    short_id(&bug.session_id),
                    format!("{:?}", bug.indicator_type),
                    truncate(&bug.description, 50),
                ]);
            }
            println!("{table}\n");
        }

        // Pattern counts
        if !results.pattern_counts.is_empty() {
            println!("📈 Communication Patterns");
            let mut patterns: Vec<_> = results.pattern_counts.iter().collect();
            patterns.sort_by(|a, b| b.1.cmp(a.1));

            for (pattern, count) in patterns {
                #[allow(clippy::cast_precision_loss)]
                let pct = *count as f64 / results.total_requests as f64 * 100.0;
                println!("  {pattern:20} {count:5} ({pct:.1}%)");
            }
            println!();
        }

        // Tool usage
        if !results.tool_usage.is_empty() {
            println!("🔧 Tool Usage");
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);
            table.set_header(vec!["Tool", "Total", "Success", "Failure", "Rate"]);

            let mut tools: Vec<_> = results.tool_usage.iter().collect();
            tools.sort_by_key(|tool| Reverse(tool.1.total));

            for (name, usage) in tools.iter().take(20) {
                table.add_row(vec![
                    truncate(name, 30),
                    usage.total.to_string(),
                    usage.successes.to_string(),
                    usage.failures.to_string(),
                    format!("{:.1}%", usage.success_rate()),
                ]);
            }
            println!("{table}");
        }

        Ok(())
    }

    pub fn print_search_results(&self, results: &[SearchMatch]) -> Result<()> {
        if self.json_mode {
            println!("{}", serde_json::to_string_pretty(results)?);
            return Ok(());
        }

        println!("\n🔍 Search Results");
        println!("════════════════════════════════════════════════════════════════\n");
        println!("Found {} matches\n", results.len());

        for (idx, result) in results.iter().enumerate() {
            let match_type = match result.match_type {
                crate::session::MatchType::Prompt => "💬 Prompt",
                crate::session::MatchType::Response => "🤖 Response",
                crate::session::MatchType::ToolInvocation => "🔧 Tool",
            };

            println!(
                "{}. [{}] {} (request #{})",
                idx + 1,
                short_id(&result.session_id),
                match_type,
                result.request_index
            );
            println!("   Match: {}", result.matched_text);
            println!("   Context: {}\n", result.context);
        }

        Ok(())
    }

    pub fn print_tool_stats(&self, stats: &ToolStats) -> Result<()> {
        if self.json_mode {
            let output = serde_json::json!({
                "by_tool": stats.by_tool,
                "total_invocations": stats.invocations.len(),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
            return Ok(());
        }

        println!("\n🔧 Tool Statistics");
        println!("════════════════════════════════════════════════════════════════\n");
        println!("Total invocations: {}\n", stats.invocations.len());

        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(vec!["Tool", "Total", "Success", "Failure", "Success Rate"]);

        let mut tools: Vec<_> = stats.by_tool.iter().collect();
        tools.sort_by_key(|tool| Reverse(tool.1.total));

        for (name, usage) in tools {
            table.add_row(vec![
                truncate(name, 40),
                usage.total.to_string(),
                usage.successes.to_string(),
                usage.failures.to_string(),
                format!("{:.1}%", usage.success_rate()),
            ]);
        }

        println!("{table}");

        // Show recent failures
        let failures: Vec<_> = stats
            .invocations
            .iter()
            .filter(|i| !i.success)
            .take(10)
            .collect();

        if !failures.is_empty() {
            println!("\n⚠️  Recent Failures (up to 10):\n");
            for inv in failures {
                println!(
                    "  • {} in session {} (request #{})",
                    inv.tool_name,
                    short_id(&inv.session_id),
                    inv.request_index
                );
                println!("    {}\n", truncate(&inv.message, 60));
            }
        }

        Ok(())
    }

    pub fn print_session_detail(&self, session: &ChatSession) -> Result<()> {
        if self.json_mode {
            println!("{}", serde_json::to_string_pretty(session)?);
            return Ok(());
        }

        println!("\n📋 Session Details");
        println!("════════════════════════════════════════════════════════════════\n");

        println!("Session ID: {}", session.session_id);
        println!(
            "User: {}",
            session.requester_username.as_deref().unwrap_or("unknown")
        );
        println!(
            "Assistant: {}",
            session.responder_username.as_deref().unwrap_or("unknown")
        );
        println!(
            "Location: {}",
            session.initial_location.as_deref().unwrap_or("unknown")
        );
        println!("Requests: {}\n", session.requests.len());

        for (idx, request) in session.requests.iter().enumerate() {
            println!("─── Request #{idx} ───────────────────────────────────────────");

            // User message
            if let Some(ref msg) = request.message
                && let Some(ref text) = msg.text
            {
                println!("\n💬 User:\n{text}\n");
            }

            // Variables/context
            if let Some(ref var_data) = request.variable_data
                && !var_data.variables.is_empty()
            {
                println!("📎 Context ({} items):", var_data.variables.len());
                for var in &var_data.variables {
                    let kind = var.kind.as_deref().unwrap_or("unknown");
                    let name = var.name.as_deref().unwrap_or("unnamed");
                    println!("   [{kind}] {name}");
                }
                println!();
            }

            // Response summary
            println!("🤖 Response:");
            for part in &request.response {
                let kind = part.kind.as_deref().unwrap_or("text");
                match kind {
                    "thinking" => {
                        if let Some(serde_json::Value::String(ref text)) = part.value
                            && !text.is_empty()
                        {
                            println!("   💭 [thinking] {}", truncate(text, 80));
                        }
                    }
                    "toolInvocationSerialized" | "prepareToolInvocation" => {
                        let tool = part
                            .tool_name
                            .as_deref()
                            .or(part.tool_id.as_deref())
                            .unwrap_or("unknown");
                        let msg = extract_value_as_string(part.invocation_message.as_ref());
                        println!("   🔧 [tool] {} - {}", tool, truncate(&msg, 60));
                    }
                    _ => {
                        if let Some(serde_json::Value::String(ref text)) = part.value
                            && !text.is_empty()
                            && text.len() > 10
                        {
                            println!("   📝 {}", truncate(text, 100));
                        }
                    }
                }
            }
            println!();
        }

        Ok(())
    }

    pub fn print_loop_patterns(&self, patterns: &[LoopPattern]) -> Result<()> {
        if self.json_mode {
            println!("{}", serde_json::to_string_pretty(patterns)?);
            return Ok(());
        }

        println!("\n🔄 Loop/Retry Patterns");
        println!("════════════════════════════════════════════════════════════════\n");

        if patterns.is_empty() {
            println!("No loop patterns detected.");
            return Ok(());
        }

        println!("Found {} patterns\n", patterns.len());

        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_content_arrangement(ContentArrangement::Dynamic);
        table.set_header(vec![
            "Session", "Type", "Tool", "Requests", "Reps", "Failures",
        ]);

        for pattern in patterns {
            let loop_type = match pattern.loop_type {
                crate::analysis::LoopType::RepeatedToolFailure => "🔴 Repeated Failure",
                crate::analysis::LoopType::ToolSpam => "🟡 Tool Spam",
                crate::analysis::LoopType::EditRetry => "🟠 Edit Retry",
            };

            table.add_row(vec![
                short_id(&pattern.session_id),
                loop_type.to_string(),
                truncate(&pattern.tool_name, 25),
                format!("{}-{}", pattern.start_request, pattern.end_request),
                pattern.repetitions.to_string(),
                pattern.failure_count.to_string(),
            ]);
        }

        println!("{table}");

        // Summary by type
        let mut by_type: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for p in patterns {
            let key = format!("{:?}", p.loop_type);
            *by_type.entry(key).or_default() += 1;
        }

        println!("\n📊 Summary by type:");
        for (loop_type, count) in by_type {
            println!("  {loop_type}: {count}");
        }

        Ok(())
    }

    pub fn print_stats(&self, sessions: &[SessionRef]) -> Result<()> {
        if self.json_mode {
            #[derive(serde::Serialize)]
            struct Stats {
                total_sessions: usize,
                total_requests: usize,
                avg_requests_per_session: f64,
                median_requests: usize,
                request_distribution: std::collections::HashMap<String, usize>,
            }

            let request_counts: Vec<usize> = sessions.iter().map(|s| s.request_count).collect();
            let total: usize = request_counts.iter().sum();
            let avg = if sessions.is_empty() {
                0.0
            } else {
                total as f64 / sessions.len() as f64
            };

            let mut sorted = request_counts.clone();
            sorted.sort_unstable();
            let median = if sorted.is_empty() {
                0
            } else {
                sorted[sorted.len() / 2]
            };

            let mut distribution = std::collections::HashMap::new();
            for count in &request_counts {
                let bucket = match *count {
                    0 => "0",
                    1..=5 => "1-5",
                    6..=20 => "6-20",
                    21..=50 => "21-50",
                    _ => "50+",
                };
                *distribution.entry(bucket.to_string()).or_default() += 1;
            }

            let stats = Stats {
                total_sessions: sessions.len(),
                total_requests: total,
                avg_requests_per_session: avg,
                median_requests: median,
                request_distribution: distribution,
            };
            println!("{}", serde_json::to_string_pretty(&stats)?);
            return Ok(());
        }

        println!("\n📊 Session Statistics");
        println!("════════════════════════════════════════════════════════════════\n");

        let request_counts: Vec<usize> = sessions.iter().map(|s| s.request_count).collect();
        let total: usize = request_counts.iter().sum();
        let avg = if sessions.is_empty() {
            0.0
        } else {
            total as f64 / sessions.len() as f64
        };

        let mut sorted = request_counts.clone();
        sorted.sort_unstable();
        let median = if sorted.is_empty() {
            0
        } else {
            sorted[sorted.len() / 2]
        };
        let max = sorted.last().copied().unwrap_or(0);
        let min = sorted.first().copied().unwrap_or(0);

        println!("📈 Overview");
        println!("  Total sessions: {}", sessions.len());
        println!("  Total requests: {total}");
        println!("  Avg requests/session: {avg:.1}");
        println!("  Median requests: {median}");
        println!("  Range: {min} - {max}");
        println!();

        // Distribution histogram
        println!("📊 Request Count Distribution");
        let buckets = [
            ("0 requests", 0..=0),
            ("1-5 requests", 1..=5),
            ("6-20 requests", 6..=20),
            ("21-50 requests", 21..=50),
            ("50+ requests", 51..=usize::MAX),
        ];

        for (label, range) in buckets {
            let count = request_counts
                .iter()
                .filter(|&&c| range.contains(&c))
                .count();
            let bar_len = (count * 30) / sessions.len().max(1);
            let bar: String = "█".repeat(bar_len);
            let pct = count as f64 / sessions.len() as f64 * 100.0;
            println!("  {label:15} {bar:30} {count:3} ({pct:.1}%)");
        }

        Ok(())
    }

    pub fn print_code_blocks(&self, blocks: &[CodeBlock]) -> Result<()> {
        if self.json_mode {
            println!("{}", serde_json::to_string_pretty(blocks)?);
            return Ok(());
        }

        println!("\n💻 Code Blocks");
        println!("════════════════════════════════════════════════════════════════\n");

        if blocks.is_empty() {
            println!("No code blocks found matching criteria.");
            return Ok(());
        }

        println!("Found {} code blocks\n", blocks.len());

        // Summary by language
        let mut by_lang: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for block in blocks {
            let lang = block.language.as_deref().unwrap_or("unknown");
            *by_lang.entry(lang.to_string()).or_default() += 1;
        }

        println!("📊 By Language:");
        let mut langs: Vec<_> = by_lang.into_iter().collect();
        langs.sort_by_key(|lang| Reverse(lang.1));
        for (lang, count) in langs.iter().take(10) {
            println!("  {lang}: {count}");
        }
        println!();

        // Show first few blocks
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_content_arrangement(ContentArrangement::Dynamic);
        table.set_header(vec!["Session", "Req#", "Language", "Lines", "Preview"]);

        for block in blocks.iter().take(20) {
            let preview = block
                .code
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(40)
                .collect::<String>();
            table.add_row(vec![
                short_id(&block.session_id),
                block.request_index.to_string(),
                block.language.as_deref().unwrap_or("-").to_string(),
                block.line_count.to_string(),
                truncate(&preview, 40),
            ]);
        }

        println!("{table}");

        Ok(())
    }

    pub fn print_interventions(&self, patterns: &[InterventionPattern]) -> Result<()> {
        if self.json_mode {
            println!("{}", serde_json::to_string_pretty(patterns)?);
            return Ok(());
        }

        println!("\n🔔 User Intervention Patterns");
        println!("════════════════════════════════════════════════════════════════\n");

        if patterns.is_empty() {
            println!("No intervention patterns detected.");
            return Ok(());
        }

        println!("Found {} interventions\n", patterns.len());

        // Summary by type
        let mut by_type: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for p in patterns {
            let key = format!("{:?}", p.intervention_type);
            *by_type.entry(key).or_default() += 1;
        }

        println!("📊 By Type:");
        for (intervention_type, count) in &by_type {
            println!("  {intervention_type}: {count}");
        }
        println!();

        // Show patterns
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_content_arrangement(ContentArrangement::Dynamic);
        table.set_header(vec!["Session", "Req#", "Type", "Preview"]);

        for pattern in patterns.iter().take(30) {
            let type_icon = match pattern.intervention_type {
                crate::analysis::InterventionType::ShortFollowUp => "📝",
                crate::analysis::InterventionType::RapidFire => "⚡",
                crate::analysis::InterventionType::ExplicitCorrection => "🛑",
            };

            table.add_row(vec![
                short_id(&pattern.session_id),
                pattern.request_index.to_string(),
                format!("{} {:?}", type_icon, pattern.intervention_type),
                truncate(&pattern.message_preview, 35),
            ]);
        }

        println!("{table}");

        Ok(())
    }

    /// Print ambiguous session selection error for LM tool consumption.
    ///
    /// Called when multiple sessions have activity within a short time window
    /// and no `match-text` was provided to disambiguate.
    pub fn print_ambiguous_sessions(
        &self,
        candidates: &[&SessionRef],
        threshold_secs: i64,
    ) -> Result<()> {
        use serde::Serialize;

        #[derive(Serialize)]
        struct AmbiguousSessionsOutput {
            ambiguous: bool,
            message: String,
            threshold_seconds: i64,
            candidates: Vec<CandidateSession>,
            hint: String,
        }

        #[derive(Serialize)]
        struct CandidateSession {
            session_id: String,
            workspace: Option<String>,
            request_count: usize,
            last_active: Option<String>,
        }

        let candidate_list: Vec<CandidateSession> = candidates
            .iter()
            .map(|s| CandidateSession {
                session_id: s.id.clone(),
                workspace: s
                    .workspace_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .map(String::from),
                request_count: s.request_count,
                last_active: s.last_message_at.map(|dt| dt.to_rfc3339()),
            })
            .collect();

        let output = AmbiguousSessionsOutput {
            ambiguous: true,
            message: format!(
                "Found {} sessions active within the last {} seconds. Provide match-text to disambiguate.",
                candidates.len(),
                threshold_secs
            ),
            threshold_seconds: threshold_secs,
            candidates: candidate_list,
            hint: "Use the match-text parameter with a distinctive phrase from a recent user message to identify the correct session.".to_string(),
        };

        if self.json_mode {
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("\n⚠️  Ambiguous Session Selection");
            println!("════════════════════════════════════════════════════════════════\n");
            println!("{}", output.message);
            println!("\nCandidates:");
            for (i, c) in output.candidates.iter().enumerate() {
                println!(
                    "  {}. {} ({} requests, last active: {})",
                    i + 1,
                    c.workspace.as_deref().unwrap_or(&c.session_id),
                    c.request_count,
                    c.last_active.as_deref().unwrap_or("unknown")
                );
            }
            println!("\n💡 {}", output.hint);
        }

        Ok(())
    }

    /// Print recent conversation turns for LM tool consumption.
    pub fn print_recent_turns(
        &self,
        session: &ChatSession,
        session_ref: &SessionRef,
        num_turns: usize,
        include_thinking: bool,
        include_tools: bool,
    ) -> Result<()> {
        use serde::Serialize;

        #[derive(Serialize)]
        struct RecentTurnsOutput {
            session_id: String,
            workspace: Option<String>,
            total_turns: usize,
            retrieved_turns: usize,
            turns: Vec<Turn>,
            #[serde(skip_serializing_if = "Option::is_none")]
            note: Option<String>,
        }

        #[derive(Serialize)]
        #[allow(clippy::struct_field_names)]
        struct Turn {
            turn_index: usize,
            #[serde(skip_serializing_if = "Option::is_none")]
            timestamp: Option<String>,
            user: String,
            assistant: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            thinking: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            tools: Option<Vec<String>>,
        }

        let total = session.requests.len();
        let start_idx = total.saturating_sub(num_turns);
        let recent_requests = &session.requests[start_idx..];

        let mut turns = Vec::new();
        for (i, req) in recent_requests.iter().enumerate() {
            let turn_index = start_idx + i + 1;

            // Extract user message
            let user = req
                .message
                .as_ref()
                .and_then(|m| m.text.clone())
                .unwrap_or_default();

            // Extract assistant response (non-null kind items are content)
            let mut assistant_parts = Vec::new();
            let mut thinking_parts = Vec::new();
            let mut tool_invocations = Vec::new();

            for part in &req.response {
                match part.kind.as_deref() {
                    None => {
                        // Main content
                        if let Some(ref value) = part.value
                            && let Some(s) = value.as_str()
                        {
                            assistant_parts.push(s.to_string());
                        }
                    }
                    Some("thinking") if include_thinking => {
                        if let Some(ref value) = part.value
                            && let Some(s) = value.as_str()
                        {
                            thinking_parts.push(s.to_string());
                        }
                    }
                    Some("toolInvocationSerialized") if include_tools => {
                        if let Some(ref msg) = part.invocation_message {
                            let s = extract_value_as_string(Some(msg));
                            if !s.is_empty() {
                                tool_invocations.push(s);
                            }
                        }
                    }
                    _ => {}
                }
            }

            turns.push(Turn {
                turn_index,
                timestamp: None, // Session format doesn't store per-request timestamps reliably
                user,
                assistant: assistant_parts.join(""),
                thinking: if thinking_parts.is_empty() {
                    None
                } else {
                    Some(thinking_parts.join("\n"))
                },
                tools: if tool_invocations.is_empty() {
                    None
                } else {
                    Some(tool_invocations)
                },
            });
        }

        let workspace = session_ref
            .workspace_path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(String::from);

        let output = RecentTurnsOutput {
            session_id: session.session_id.clone(),
            workspace,
            total_turns: total,
            retrieved_turns: turns.len(),
            turns,
            note: if total > num_turns {
                Some(format!(
                    "Showing last {num_turns} of {total} total turns. Use --turns to retrieve more."
                ))
            } else {
                None
            },
        };

        if self.json_mode {
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            // Human-readable format
            println!("\n📜 Recent Conversation");
            println!("════════════════════════════════════════════════════════════════\n");
            println!(
                "Session: {} | Workspace: {}",
                short_id(&output.session_id),
                output.workspace.as_deref().unwrap_or("unknown")
            );
            println!("Showing turns {}-{} of {}\n", start_idx + 1, total, total);

            for turn in &output.turns {
                println!("─── Turn {} ───", turn.turn_index);
                println!("👤 User: {}", truncate(&turn.user, 500));
                println!("🤖 Assistant: {}", truncate(&turn.assistant, 500));
                if let Some(ref thinking) = turn.thinking {
                    println!("💭 Thinking: {}", truncate(thinking, 200));
                }
                if let Some(ref tools) = turn.tools {
                    println!("🔧 Tools: {}", tools.join(", "));
                }
                println!();
            }
        }

        Ok(())
    }

    /// Print conversation turns before the last summarization.
    ///
    /// Finds the most recent user message containing `<conversation-summary>`
    /// and returns N turns immediately before it. This recovers context that
    /// was just compacted by VS Code's summarization.
    pub fn print_turns_before_summary(
        &self,
        session: &ChatSession,
        session_ref: &SessionRef,
        num_turns: usize,
        include_thinking: bool,
        include_tools: bool,
    ) -> Result<()> {
        use serde::Serialize;

        const SUMMARY_MARKER: &str = "<conversation-summary>";

        // Find the index of the last message containing the summary marker
        let summary_idx = session
            .requests
            .iter()
            .enumerate()
            .rev()
            .find(|(_, req)| {
                req.message
                    .as_ref()
                    .and_then(|m| m.text.as_ref())
                    .is_some_and(|t| t.contains(SUMMARY_MARKER))
            })
            .map(|(idx, _)| idx);

        let Some(summary_idx) = summary_idx else {
            // No summary found - return an informative response
            #[derive(Serialize)]
            struct NoSummaryOutput {
                no_summary: bool,
                message: String,
                hint: String,
            }

            let output = NoSummaryOutput {
                no_summary: true,
                message: "No conversation summary found in this session.".to_string(),
                hint: "The session may not have been summarized yet, or this is a fresh session. Use the tool without --before-summary to get recent turns.".to_string(),
            };

            if self.json_mode {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("\n⚠️  No Summary Found");
                println!("════════════════════════════════════════════════════════════════\n");
                println!("{}", output.message);
                println!("\n💡 {}", output.hint);
            }
            return Ok(());
        };

        #[allow(clippy::items_after_statements)]
        #[derive(Serialize)]
        struct BeforeSummaryOutput {
            session_id: String,
            workspace: Option<String>,
            summary_at_turn: usize,
            total_turns: usize,
            retrieved_turns: usize,
            turns: Vec<Turn>,
            note: Option<String>,
        }

        #[derive(Serialize)]
        #[allow(clippy::struct_field_names)]
        struct Turn {
            turn_index: usize,
            #[serde(skip_serializing_if = "Option::is_none")]
            timestamp: Option<String>,
            user: String,
            assistant: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            thinking: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            tools: Option<Vec<String>>,
        }

        // Get turns before the summary (not including the summary message itself)
        let end_idx = summary_idx;
        let start_idx = end_idx.saturating_sub(num_turns);
        let pre_summary_requests = &session.requests[start_idx..end_idx];

        let mut turns = Vec::new();
        for (i, req) in pre_summary_requests.iter().enumerate() {
            let turn_index = start_idx + i + 1;

            let user = req
                .message
                .as_ref()
                .and_then(|m| m.text.clone())
                .unwrap_or_default();

            let mut assistant_parts = Vec::new();
            let mut thinking_parts = Vec::new();
            let mut tool_invocations = Vec::new();

            for part in &req.response {
                match part.kind.as_deref() {
                    None => {
                        if let Some(ref value) = part.value
                            && let Some(s) = value.as_str()
                        {
                            assistant_parts.push(s.to_string());
                        }
                    }
                    Some("thinking") if include_thinking => {
                        if let Some(ref value) = part.value
                            && let Some(s) = value.as_str()
                        {
                            thinking_parts.push(s.to_string());
                        }
                    }
                    Some("toolInvocationSerialized") if include_tools => {
                        if let Some(ref msg) = part.invocation_message {
                            let s = extract_value_as_string(Some(msg));
                            if !s.is_empty() {
                                tool_invocations.push(s);
                            }
                        }
                    }
                    _ => {}
                }
            }

            turns.push(Turn {
                turn_index,
                timestamp: None,
                user,
                assistant: assistant_parts.join(""),
                thinking: if thinking_parts.is_empty() {
                    None
                } else {
                    Some(thinking_parts.join("\n"))
                },
                tools: if tool_invocations.is_empty() {
                    None
                } else {
                    Some(tool_invocations)
                },
            });
        }

        let workspace = session_ref
            .workspace_path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(String::from);

        let total = session.requests.len();
        let output = BeforeSummaryOutput {
            session_id: session.session_id.clone(),
            workspace,
            summary_at_turn: summary_idx + 1,
            total_turns: total,
            retrieved_turns: turns.len(),
            turns,
            note: if end_idx > num_turns {
                Some(format!(
                    "Showing {} turns before summary (turns {}-{}). Use --turns to retrieve more.",
                    pre_summary_requests.len(),
                    start_idx + 1,
                    end_idx
                ))
            } else {
                None
            },
        };

        if self.json_mode {
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("\n📜 Conversation Before Summary");
            println!("════════════════════════════════════════════════════════════════\n");
            println!(
                "Session: {} | Workspace: {}",
                short_id(&output.session_id),
                output.workspace.as_deref().unwrap_or("unknown")
            );
            println!(
                "Summary at turn {} | Showing turns {}-{}\n",
                output.summary_at_turn,
                start_idx + 1,
                end_idx
            );

            for turn in &output.turns {
                println!("─── Turn {} ───", turn.turn_index);
                println!("👤 User: {}", truncate(&turn.user, 500));
                println!("🤖 Assistant: {}", truncate(&turn.assistant, 500));
                if let Some(ref thinking) = turn.thinking {
                    println!("💭 Thinking: {}", truncate(thinking, 200));
                }
                if let Some(ref tools) = turn.tools {
                    println!("🔧 Tools: {}", tools.join(", "));
                }
                println!();
            }
        }

        Ok(())
    }
}

fn short_id(id: &str) -> String {
    if id.len() <= 12 {
        return id.to_string();
    }
    // Find valid UTF-8 boundary at or before position 12
    let mut end = 12;
    while end > 0 && !id.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &id[..end])
}

fn truncate(s: &str, max_len: usize) -> String {
    let s = s.replace('\n', " ").replace('\r', "");
    if s.len() <= max_len {
        return s;
    }
    // Find valid UTF-8 boundary at or before max_len
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// Extract a string from a `serde_json::Value`
fn extract_value_as_string(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Object(obj)) => obj
            .get("value")
            .or_else(|| obj.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}
