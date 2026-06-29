//! Analysis engine for chat sessions.

#![allow(clippy::cast_precision_loss)] // Acceptable for metrics/display

use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;

use crate::session::{ChatSession, SessionRef, ToolInvocation};
use crate::storage::ChatStorageLocator;

/// Configuration for session analysis.
#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    pub analyze_bugs: bool,
    pub analyze_patterns: bool,
    pub analyze_tools: bool,
}

/// Analyzer for chat sessions.
pub struct SessionAnalyzer {
    config: AnalysisConfig,
}

impl SessionAnalyzer {
    pub const fn new(config: AnalysisConfig) -> Self {
        Self { config }
    }

    pub fn analyze_sessions(&self, sessions: &[SessionRef]) -> Result<AnalysisResults> {
        let locator = ChatStorageLocator::new(None)?;
        let mut results = AnalysisResults::default();

        for session_ref in sessions {
            let Ok(session) = locator.load_session(session_ref) else {
                continue;
            };

            results.total_sessions += 1;
            results.total_requests += session.requests.len();

            if self.config.analyze_bugs {
                self.analyze_bugs(&session, &mut results);
            }

            if self.config.analyze_patterns {
                self.analyze_patterns(&session, &mut results);
            }

            if self.config.analyze_tools {
                self.analyze_tools(&session, &mut results);
            }

            // Classify session outcome
            let outcome = classify_session_outcome(&session);
            *results.session_outcomes.entry(outcome).or_default() += 1;
        }

        // Compute derived statistics
        results.compute_statistics();

        Ok(results)
    }

    #[allow(clippy::unused_self)] // Methods take &self for future extensibility
    fn analyze_bugs(&self, session: &ChatSession, results: &mut AnalysisResults) {
        for request in &session.requests {
            for part in &request.response {
                // Look for error-like patterns
                if let Some(ref kind) = part.kind
                    && (kind == "error" || kind == "errorDetails")
                {
                    results.bug_indicators.push(BugIndicator {
                        session_id: session.session_id.clone(),
                        indicator_type: BugType::ErrorResponse,
                        description: part
                            .value
                            .as_ref()
                            .map(std::string::ToString::to_string)
                            .unwrap_or_default(),
                    });
                }

                // Check for failed tool invocations
                if part.tool_name.is_some()
                    && part.past_tense_message.is_none()
                    && let Some(ref tool_name) = part.tool_name
                {
                    results.bug_indicators.push(BugIndicator {
                        session_id: session.session_id.clone(),
                        indicator_type: BugType::FailedToolCall,
                        description: format!("Tool '{tool_name}' may have failed"),
                    });
                }
            }
        }
    }

    #[allow(clippy::unused_self)]
    fn analyze_patterns(&self, session: &ChatSession, results: &mut AnalysisResults) {
        for request in &session.requests {
            // Analyze prompt patterns
            if let Some(ref msg) = request.message
                && let Some(ref text) = msg.text
            {
                // Count prompt length
                results.prompt_lengths.push(text.len());

                // Detect question patterns
                if text.contains('?') {
                    results
                        .pattern_counts
                        .entry("questions".to_string())
                        .and_modify(|c| *c += 1)
                        .or_insert(1);
                }

                // Detect commands/imperatives
                let lower = text.to_lowercase();
                for keyword in [
                    "fix",
                    "implement",
                    "create",
                    "add",
                    "remove",
                    "update",
                    "change",
                ] {
                    if lower.starts_with(keyword) || lower.contains(&format!(" {keyword} ")) {
                        results
                            .pattern_counts
                            .entry("imperative".to_string())
                            .and_modify(|c| *c += 1)
                            .or_insert(1);
                        break;
                    }
                }

                // Detect debugging patterns
                for keyword in [
                    "error",
                    "bug",
                    "fail",
                    "broken",
                    "not working",
                    "doesn't work",
                ] {
                    if lower.contains(keyword) {
                        results
                            .pattern_counts
                            .entry("debugging".to_string())
                            .and_modify(|c| *c += 1)
                            .or_insert(1);
                        break;
                    }
                }
            }

            // Analyze response patterns
            let mut has_thinking = false;
            let mut has_tool_use = false;
            let mut has_code = false;

            for part in &request.response {
                if let Some(ref kind) = part.kind {
                    if kind == "thinking" {
                        has_thinking = true;
                    }
                    if kind == "toolInvocationSerialized" || kind == "prepareToolInvocation" {
                        has_tool_use = true;
                    }
                }

                // Check for code blocks in response
                if let Some(serde_json::Value::String(ref text)) = part.value
                    && text.contains("```")
                {
                    has_code = true;
                }
            }

            if has_thinking {
                results
                    .pattern_counts
                    .entry("with_thinking".to_string())
                    .and_modify(|c| *c += 1)
                    .or_insert(1);
            }
            if has_tool_use {
                results
                    .pattern_counts
                    .entry("with_tool_use".to_string())
                    .and_modify(|c| *c += 1)
                    .or_insert(1);
            }
            if has_code {
                results
                    .pattern_counts
                    .entry("with_code".to_string())
                    .and_modify(|c| *c += 1)
                    .or_insert(1);
            }
        }
    }

    #[allow(clippy::unused_self)]
    fn analyze_tools(&self, session: &ChatSession, results: &mut AnalysisResults) {
        let invocations = session.extract_tool_invocations();

        for inv in invocations {
            let entry = results.tool_usage.entry(inv.tool_name.clone()).or_default();
            entry.total += 1;
            if inv.success {
                entry.successes += 1;
            } else {
                entry.failures += 1;
            }
        }
    }
}

/// Results from analyzing chat sessions.
#[derive(Debug, Default, Serialize)]
pub struct AnalysisResults {
    pub total_sessions: usize,
    pub total_requests: usize,
    pub bug_indicators: Vec<BugIndicator>,
    pub pattern_counts: HashMap<String, usize>,
    pub tool_usage: HashMap<String, ToolUsage>,
    pub prompt_lengths: Vec<usize>,
    pub session_outcomes: HashMap<SessionOutcome, usize>,

    // Computed statistics
    pub avg_prompt_length: f64,
    pub median_prompt_length: usize,
}

/// Classification of how a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum SessionOutcome {
    /// Session completed successfully (task done, user acknowledged)
    Successful,
    /// Session ended with errors or repeated failures  
    ErrorTerminated,
    /// Session appears abandoned (incomplete task, no user follow-up)
    Abandoned,
    /// Session is ongoing or outcome unclear
    Indeterminate,
}

impl AnalysisResults {
    #[allow(clippy::cast_precision_loss)] // Acceptable for statistics
    fn compute_statistics(&mut self) {
        if !self.prompt_lengths.is_empty() {
            let sum: usize = self.prompt_lengths.iter().sum();
            self.avg_prompt_length = sum as f64 / self.prompt_lengths.len() as f64;

            let mut sorted = self.prompt_lengths.clone();
            sorted.sort_unstable();
            self.median_prompt_length = sorted[sorted.len() / 2];
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BugIndicator {
    pub session_id: String,
    pub indicator_type: BugType,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)] // ProtocolViolation reserved for future use
pub enum BugType {
    ErrorResponse,
    FailedToolCall,
    ProtocolViolation,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ToolUsage {
    pub total: usize,
    pub successes: usize,
    pub failures: usize,
}

impl ToolUsage {
    #[allow(clippy::cast_precision_loss)] // Acceptable for statistics
    pub fn success_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.successes as f64 / self.total as f64 * 100.0
        }
    }
}

/// Tool statistics collector (for dedicated tool analysis).
#[derive(Debug, Default)]
pub struct ToolStats {
    pub by_tool: HashMap<String, ToolUsage>,
    pub invocations: Vec<ToolInvocation>,
}

impl ToolStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn collect_from_session(
        &mut self,
        session: &ChatSession,
        tool_filter: Option<&str>,
        failures_only: bool,
    ) {
        let invocations = session.extract_tool_invocations();

        for inv in invocations {
            // Apply filters
            if let Some(filter) = tool_filter
                && !inv.tool_name.contains(filter)
            {
                continue;
            }
            if failures_only && inv.success {
                continue;
            }

            // Update stats
            let entry = self.by_tool.entry(inv.tool_name.clone()).or_default();
            entry.total += 1;
            if inv.success {
                entry.successes += 1;
            } else {
                entry.failures += 1;
            }

            self.invocations.push(inv);
        }
    }
}

// ============================================================================
// Loop/Retry Detection
// ============================================================================

/// Type of loop pattern detected.
#[derive(Debug, Clone, Serialize)]
pub enum LoopType {
    /// Same tool called repeatedly with failures
    RepeatedToolFailure,
    /// Same tool called many times in quick succession
    ToolSpam,
    /// Edit/replace attempts that keep failing
    EditRetry,
}

/// A detected loop pattern in a session.
#[derive(Debug, Clone, Serialize)]
pub struct LoopPattern {
    pub session_id: String,
    pub loop_type: LoopType,
    pub tool_name: String,
    pub start_request: usize,
    pub end_request: usize,
    pub repetitions: usize,
    pub failure_count: usize,
}

/// Detector for loop/retry patterns in sessions.
#[derive(Debug, Default)]
pub struct LoopDetector {
    pub min_repetitions: usize,
}

impl LoopDetector {
    pub const fn new(min_repetitions: usize) -> Self {
        Self { min_repetitions }
    }

    /// Detect loop patterns in a session.
    pub fn detect(&self, session: &ChatSession) -> Vec<LoopPattern> {
        let mut patterns = Vec::new();
        let invocations = session.extract_tool_invocations();

        if invocations.is_empty() {
            return patterns;
        }

        // Group consecutive invocations of the same tool
        let mut i = 0;
        while i < invocations.len() {
            let tool_name = &invocations[i].tool_name;
            let start_request = invocations[i].request_index;
            let mut count = 1;
            let mut failures = usize::from(!invocations[i].success);

            // Count consecutive same-tool invocations
            while i + 1 < invocations.len() && invocations[i + 1].tool_name == *tool_name {
                i += 1;
                count += 1;
                if !invocations[i].success {
                    failures += 1;
                }
            }

            let end_request = invocations[i].request_index;

            // Check if this qualifies as a loop
            if count >= self.min_repetitions {
                let loop_type = Self::classify_loop(tool_name, count, failures);
                patterns.push(LoopPattern {
                    session_id: session.session_id.clone(),
                    loop_type,
                    tool_name: tool_name.clone(),
                    start_request,
                    end_request,
                    repetitions: count,
                    failure_count: failures,
                });
            }

            i += 1;
        }

        // Also detect edit retry patterns (alternating edit attempts)
        patterns.extend(self.detect_edit_retries(session, &invocations));

        patterns
    }

    #[allow(clippy::cast_precision_loss)] // Acceptable for rate calculation
    fn classify_loop(tool_name: &str, count: usize, failures: usize) -> LoopType {
        let failure_rate = failures as f64 / count as f64;

        // Edit tools with high failure rate = edit retry
        if (tool_name.contains("replace")
            || tool_name.contains("edit")
            || tool_name.contains("patch"))
            && failure_rate > 0.5
        {
            return LoopType::EditRetry;
        }

        // High failure rate = repeated failure
        if failure_rate > 0.5 {
            return LoopType::RepeatedToolFailure;
        }

        // Otherwise it's just spam (many calls, but successful)
        LoopType::ToolSpam
    }

    fn detect_edit_retries(
        &self,
        session: &ChatSession,
        invocations: &[ToolInvocation],
    ) -> Vec<LoopPattern> {
        let mut patterns = Vec::new();

        // Look for patterns like: read -> edit(fail) -> read -> edit(fail)
        let edit_tools = ["replace", "edit", "patch", "create"];

        let mut consecutive_edit_failures = 0;
        let mut start_request = None;
        let mut last_request = 0;

        for inv in invocations {
            let is_edit_tool = edit_tools
                .iter()
                .any(|t| inv.tool_name.to_lowercase().contains(t));

            if is_edit_tool && !inv.success {
                if start_request.is_none() {
                    start_request = Some(inv.request_index);
                }
                consecutive_edit_failures += 1;
                last_request = inv.request_index;
            } else if is_edit_tool && inv.success {
                // Success breaks the chain - check if we found a pattern
                if consecutive_edit_failures >= self.min_repetitions {
                    patterns.push(LoopPattern {
                        session_id: session.session_id.clone(),
                        loop_type: LoopType::EditRetry,
                        tool_name: "edit_tools".to_string(),
                        start_request: start_request.unwrap_or(0),
                        end_request: last_request,
                        repetitions: consecutive_edit_failures,
                        failure_count: consecutive_edit_failures,
                    });
                }
                consecutive_edit_failures = 0;
                start_request = None;
            }
        }

        // Check trailing pattern
        if consecutive_edit_failures >= self.min_repetitions {
            patterns.push(LoopPattern {
                session_id: session.session_id.clone(),
                loop_type: LoopType::EditRetry,
                tool_name: "edit_tools".to_string(),
                start_request: start_request.unwrap_or(0),
                end_request: last_request,
                repetitions: consecutive_edit_failures,
                failure_count: consecutive_edit_failures,
            });
        }

        patterns
    }
}

// ============================================================================
// Session Outcome Classification
// ============================================================================

/// Classify how a session ended based on heuristics.
pub fn classify_session_outcome(session: &ChatSession) -> SessionOutcome {
    if session.requests.is_empty() {
        return SessionOutcome::Indeterminate;
    }

    let last_request = &session.requests[session.requests.len() - 1];
    let invocations = session.extract_tool_invocations();

    // Check for error patterns in last request
    let has_error_in_last = last_request.response.iter().any(|part| {
        part.kind.as_deref() == Some("error") || part.kind.as_deref() == Some("errorDetails")
    });

    if has_error_in_last {
        return SessionOutcome::ErrorTerminated;
    }

    // Check if last few tool calls failed
    let last_invocations: Vec<_> = invocations
        .iter()
        .filter(|inv| inv.request_index == session.requests.len() - 1)
        .collect();

    let failure_count = last_invocations.iter().filter(|inv| !inv.success).count();
    if last_invocations.len() >= 3 && failure_count as f64 / last_invocations.len() as f64 > 0.5 {
        return SessionOutcome::ErrorTerminated;
    }

    // Check for success indicators in user's last message
    if let Some(ref msg) = last_request.message
        && let Some(ref text) = msg.text
    {
        let text_lower = text.to_lowercase();
        // User acknowledgments suggest success
        if text_lower.contains("thanks")
            || text_lower.contains("perfect")
            || text_lower.contains("great")
            || text_lower.contains("lgtm")
            || text_lower.contains("ship it")
            || text_lower.contains("merge")
        {
            return SessionOutcome::Successful;
        }
    }

    // Check if session ended with successful tool completions
    let has_successful_tools = last_invocations.iter().any(|inv| inv.success);
    if has_successful_tools && failure_count == 0 {
        return SessionOutcome::Successful;
    }

    // Short sessions with no clear outcome
    if session.requests.len() <= 2 {
        return SessionOutcome::Indeterminate;
    }

    // Default: assume abandoned if no clear success/error signal
    SessionOutcome::Abandoned
}

// ============================================================================
// Intervention Pattern Detection
// ============================================================================

/// Types of user intervention patterns.
#[derive(Debug, Clone, Serialize)]
pub enum InterventionType {
    /// Short follow-up message (likely correction/redirection)
    ShortFollowUp,
    /// Multiple rapid messages (user taking over)
    RapidFire,
    /// Explicit correction ("no", "stop", "wrong")
    ExplicitCorrection,
}

/// A detected user intervention.
#[derive(Debug, Clone, Serialize)]
pub struct InterventionPattern {
    pub session_id: String,
    pub request_index: usize,
    pub intervention_type: InterventionType,
    pub message_preview: String,
}

/// Detect patterns where user intervenes (corrections, redirections).
pub fn detect_interventions(session: &ChatSession) -> Vec<InterventionPattern> {
    let mut patterns = Vec::new();

    let correction_words = [
        "no",
        "stop",
        "wrong",
        "incorrect",
        "actually",
        "wait",
        "don't",
        "not what",
    ];

    for (idx, request) in session.requests.iter().enumerate() {
        if let Some(ref msg) = request.message
            && let Some(ref text) = msg.text
        {
            let text_lower = text.to_lowercase();
            let word_count = text.split_whitespace().count();

            // Short follow-up (< 20 words after first request)
            if idx > 0 && word_count < 20 && word_count > 0 {
                patterns.push(InterventionPattern {
                    session_id: session.session_id.clone(),
                    request_index: idx,
                    intervention_type: InterventionType::ShortFollowUp,
                    message_preview: text.chars().take(50).collect(),
                });
            }

            // Explicit correction
            for word in &correction_words {
                if text_lower.starts_with(word) || text_lower.contains(&format!(" {word} ")) {
                    patterns.push(InterventionPattern {
                        session_id: session.session_id.clone(),
                        request_index: idx,
                        intervention_type: InterventionType::ExplicitCorrection,
                        message_preview: text.chars().take(50).collect(),
                    });
                    break;
                }
            }
        }
    }

    // Detect rapid-fire: 3+ short messages in a row
    let mut consecutive_short = 0;
    for (idx, request) in session.requests.iter().enumerate() {
        let is_short = request
            .message
            .as_ref()
            .and_then(|m| m.text.as_ref())
            .is_some_and(|t| t.split_whitespace().count() < 15);

        if is_short {
            consecutive_short += 1;
            if consecutive_short >= 3 {
                patterns.push(InterventionPattern {
                    session_id: session.session_id.clone(),
                    request_index: idx,
                    intervention_type: InterventionType::RapidFire,
                    message_preview: "3+ short messages in sequence".to_string(),
                });
            }
        } else {
            consecutive_short = 0;
        }
    }

    patterns
}
