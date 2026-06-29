# exohistory: Chat History Analysis Report

**Date:** 2026-01-28  
**Sessions Analyzed:** 128  
**Total Requests:** 3,058  
**Tool Invocations:** 129,547

## Executive Summary

Analysis of VSCode Copilot chat history reveals significant opportunities to improve agent reliability. Key findings:

- **Only 16.4% of sessions clearly succeed** - most are abandoned or indeterminate
- **Edit tools have ~50% failure rate** - major friction point
- **Agents frequently loop** - 5,588 detected patterns of repeated tool calls
- **Short prompts dominate follow-ups** - users often redirect with brief messages

---

## Key Findings

### 1. Session Outcomes

| Outcome             | Count | Percentage |
| ------------------- | ----- | ---------- |
| ✅ Successful       | 21    | 16.4%      |
| 🚪 Abandoned        | 46    | 35.9%      |
| ❓ Indeterminate    | 60    | 46.9%      |
| ❌ Error Terminated | 1     | 0.8%       |

**Interpretation:** The majority of sessions end without clear completion. This suggests either:

- Tasks are exploratory (no clear "done" state)
- Agent struggles to complete tasks autonomously
- Users abandon sessions due to friction

### 2. Tool Reliability (Critical)

Tools with lowest success rates:

| Tool                         | Total Calls | Success Rate | Impact                        |
| ---------------------------- | ----------- | ------------ | ----------------------------- |
| `copilot_applyPatch`         | 3,727       | **49.5%**    | Half of patch operations fail |
| `manage_todo_list`           | 2,094       | 50.3%        | Todo management unreliable    |
| `copilot_multiReplaceString` | 1,018       | 50.9%        | Multi-replace often fails     |
| `run_in_terminal`            | 36,180      | 52.2%        | Terminal commands flaky       |
| `copilot_replaceString`      | 6,731       | 52.3%        | String replacement unreliable |
| `copilot_createFile`         | 1,612       | 54.7%        | File creation issues          |

Tools with best success rates:

| Tool                     | Total Calls | Success Rate |
| ------------------------ | ----------- | ------------ |
| `copilot_searchCodebase` | 706         | **85.4%**    |
| `copilot_listDirectory`  | 3,674       | 70.3%        |
| `copilot_findFiles`      | 2,472       | 68.9%        |

**Key Insight:** Discovery tools work well; modification tools struggle.

### 3. Loop/Retry Patterns

Detected **5,588 patterns** where agents repeat the same tool 5+ times.

Common patterns:

- `copilot_readFile`: Up to 42 consecutive calls with 50% failure rate
- `copilot_findTextInFiles`: Up to 38 consecutive calls
- `run_in_terminal`: Repeated attempts, ~50% failing

**Root Cause Hypothesis:** Agent receives errors but doesn't adapt strategy. Instead of trying alternative approaches, it retries the same operation.

### 4. User Intervention Patterns

| Type                   | Count | Example                            |
| ---------------------- | ----- | ---------------------------------- |
| 📝 Short Follow-up     | 24    | "proceed", "please do", "go ahead" |
| ⚡ Rapid Fire          | 4     | 3+ short messages in sequence      |
| 🛑 Explicit Correction | 2     | "no", "wrong", "stop"              |

**Interpretation:** Most interventions are confirmations or brief redirections rather than corrections. This suggests users are comfortable with agent direction but want to maintain control.

---

## Recommendations

### High Priority

1. **Improve Edit Reliability**
   - Add fuzzy matching for `replaceString` when exact match fails
   - Validate file state before attempting edits
   - Provide diff preview before applying patches

2. **Add Loop Detection/Breaking**
   - Detect when same tool called 3+ times with failures
   - Automatically suggest alternative approach
   - Escalate to user after N failures

3. **File Path Validation**
   - Verify paths exist before `readFile` attempts
   - Cache valid file list per workspace
   - Suggest similar paths when not found

### Medium Priority

4. **Terminal Command Batching**
   - Combine related commands with `&&`
   - Use `--quiet` flags to reduce output noise
   - Add timeout handling for hung commands

5. **Better Error Messages**
   - Parse tool error output for actionable guidance
   - Suggest fixes for common failure patterns
   - Track error patterns across sessions

### Lower Priority

6. **Session Continuity**
   - Improve handoff summaries between sessions
   - Track incomplete tasks across sessions
   - Suggest resuming abandoned work

---

## Reproducing This Analysis

### Prerequisites

```bash
# Build the exohistory tool
cargo build -p exohistory --release
```

### Commands Used

#### 1. Full Analysis Summary

```bash
exohistory analyze
```

Shows session outcomes, bug indicators, and pattern summary.

#### 2. Tool Usage Statistics

```bash
exohistory tools
```

Shows success/failure rates by tool.

#### 3. Loop/Retry Detection

```bash
exohistory loops --min-repetitions 5
```

Finds patterns where tools are called repeatedly.

#### 4. User Intervention Patterns

```bash
exohistory interventions --limit 50
```

Detects short follow-ups, rapid-fire messages, and explicit corrections.

#### 5. Session Statistics

```bash
exohistory stats
```

Shows request distribution histogram and prompt length statistics.

#### 6. Compare Two Sessions

```bash
exohistory diff <session_a_id> <session_b_id>
```

Side-by-side comparison of tool usage between sessions.

#### 7. Extract Code Blocks

```bash
exohistory code --language rust --min-lines 10
```

Extract code blocks from agent responses.

### Filtering Options

All commands support:

- `--workspace <path>` - Filter by workspace path (partial match)
- `--format json` - Output as JSON for further processing
- `--storage-path <path>` - Use custom VSCode storage location

### Example: Analyze Specific Workspace

```bash
exohistory analyze --workspace exo2 --format json | jq '.session_outcomes'
```

### Example: Find High-Failure Sessions

```bash
exohistory loops --min-repetitions 10 --format json | \
  jq '[.[] | select(.failure_count > 5)]'
```

---

## Data Location

VSCode Copilot chat history is stored at:

```
~/.config/Code/User/workspaceStorage/*/GitHub.copilot-chat/chatSessions/*.json
```

Each session file contains:

- `sessionId` - Unique identifier
- `requests[]` - User prompts and agent responses
- `response[]` - Individual response parts including tool invocations
- Timestamps, workspace info, user context

---

## Next Steps

1. **Implement exohook improvements** based on findings
2. **Add automated loop-breaking** to agent prompts
3. **Track metrics over time** to measure improvement
4. **Correlate with AGENTS.md** - see if instruction changes improve outcomes

---

_Generated by exohistory analysis tool_
