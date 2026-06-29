<!-- exo:91 ulid:01kg5kp2fg8gvcr7qz50czff86 -->

# RFC 91: Protected File Watcher with Revert and Notice System


> **Status**: Withdrawn  
> **Reason**: Superseded by RFC 0111: Agent Guidance Architecture  
> **Date**: 2026-01-15
>
> The reactive watcher approach has been replaced by proactive file-scoped
> instructions. See [RFC 0111](../stage-4/0111-agent-guidance-architecture.md)
> for the new Agent Guidance Architecture.

# RFC 0091: Protected File Watcher with Revert and Notice System

## Summary

This RFC proposes a Protected File Watcher system that monitors CLI-managed files (plan.toml, implementation-plan.toml, etc.) in VS Code, automatically reverts unauthorized edits, and queues notices to remind agents to use the proper CLI commands. This prevents silent edit failures and maintains invariant integrity.

## Motivation

### The Problem

Exosuit has "protected files" that must be modified through the CLI (e.g., `exo task add`, `exo phase finish`) to maintain invariants:

- `docs/agent-context/plan.toml`
- `docs/agent-context/current/implementation-plan.toml`
- `docs/agent-context/ideas.toml`
- `docs/agent-context/decisions.toml`

When an AI agent (or user) directly edits these files:

1. **Silent failure**: The file may be marked read-only, causing empty writes
2. **Invariant violation**: Direct edits bypass validation (e.g., ULID generation, status transitions)
3. **Teaching failure**: The agent learns bad habits instead of using proper CLI commands

### Real-World Failure Mode

This session demonstrated the problem: attempting to create RFC files via VS Code's write API resulted in empty files due to `files.readonlyInclude`. While we fixed the RFC case (RFCs should be editable), TOML context files genuinely need protection.

### Why Now

This is Phase 5 work (Exohook Integration). The exohook system already handles git hook validation; extending it to watch protected files completes the "safe workflow" guarantee.

## Detailed Design

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    VS Code Extension                        │
│  ┌─────────────────────────────────────────────────────┐   │
│  │           ProtectedFileWatcher Service              │   │
│  │  - Watches protected file patterns                  │   │
│  │  - On change: git restore + queue notice            │   │
│  │  - Debounces rapid edits                           │   │
│  └─────────────────────────────────────────────────────┘   │
│                           │                                 │
│                           ▼                                 │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              Notice UI (non-blocking)               │   │
│  │  - Toast: "Use exo task add instead of editing"     │   │
│  │  - Quick-pick: suggested CLI command               │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼ Machine Channel
┌─────────────────────────────────────────────────────────────┐
│                      CLI (exo)                              │
│  ┌─────────────────────────────────────────────────────┐   │
│  │           Protected File Configuration              │   │
│  │  - exosuit.toml [protected-files] section          │   │
│  │  - Glob patterns + suggested commands              │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### Protected File Configuration

In `exosuit.toml`:

```toml
[protected-files]
patterns = [
    "docs/agent-context/**/*.toml",
]

[protected-files.commands]
"docs/agent-context/plan.toml" = "exo plan, exo task, exo phase"
"docs/agent-context/current/implementation-plan.toml" = "exo impl"
"docs/agent-context/ideas.toml" = "exo idea"
"docs/agent-context/decisions.toml" = "exo decision"
```

### Rust Types

```rust
// In exosuit-core/src/protected.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedFileConfig {
    pub patterns: Vec<String>,
    pub commands: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectionNotice {
    pub id: String,
    pub timestamp: String,
    pub file_path: String,
    pub action_taken: ProtectionAction,
    pub suggested_command: Option<String>,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtectionAction {
    Reverted,       // File was restored via git
    Blocked,        // Write was prevented (readonly)
    Warned,         // File was new/untracked, just warned
}
```

### VS Code Implementation

```typescript
// In src/services/ProtectedFileWatcher.ts

import * as vscode from "vscode";
import { exec } from "child_process";
import { promisify } from "util";

const execAsync = promisify(exec);

export class ProtectedFileWatcher implements vscode.Disposable {
  private watcher: vscode.FileSystemWatcher | undefined;
  private debounceTimers = new Map<string, NodeJS.Timeout>();
  private patterns: string[];
  private commands: Map<string, string>;

  constructor(private context: vscode.ExtensionContext) {
    this.patterns = ["docs/agent-context/**/*.toml"];
    this.commands = new Map([
      ["plan.toml", "exo plan, exo task, exo phase"],
      ["implementation-plan.toml", "exo impl"],
      ["ideas.toml", "exo idea"],
    ]);
  }

  activate(): void {
    // Watch for changes to protected files
    this.watcher = vscode.workspace.createFileSystemWatcher(
      new vscode.RelativePattern(
        vscode.workspace.workspaceFolders![0],
        "docs/agent-context/**/*.toml"
      )
    );

    this.watcher.onDidChange((uri) => this.handleChange(uri));
    this.watcher.onDidCreate((uri) => this.handleChange(uri));
  }

  private async handleChange(uri: vscode.Uri): Promise<void> {
    const relativePath = vscode.workspace.asRelativePath(uri);

    // Debounce rapid changes
    const existing = this.debounceTimers.get(relativePath);
    if (existing) clearTimeout(existing);

    this.debounceTimers.set(
      relativePath,
      setTimeout(async () => {
        await this.revertAndNotify(uri, relativePath);
        this.debounceTimers.delete(relativePath);
      }, 500)
    );
  }

  private async revertAndNotify(
    uri: vscode.Uri,
    relativePath: string
  ): Promise<void> {
    const workspaceRoot = vscode.workspace.workspaceFolders![0].uri.fsPath;

    try {
      // Restore from git (use spawn to avoid shell injection)
      await new Promise<void>((resolve, reject) => {
        const proc = spawn("git", ["restore", "--", relativePath], {
          cwd: workspaceRoot,
        });
        proc.on("close", (code) =>
          code === 0
            ? resolve()
            : reject(new Error(`git restore failed: ${code}`))
        );
        proc.on("error", reject);
      });

      // Find suggested command
      const fileName = relativePath.split("/").pop() || "";
      const suggestedCmd = this.commands.get(fileName) || "the exo CLI";

      // Show non-blocking notice
      const action = await vscode.window.showWarningMessage(
        `Protected file reverted: ${fileName}. Use ${suggestedCmd} instead.`,
        "Copy Command"
      );

      if (action === "Copy Command") {
        const cmd = suggestedCmd.split(",")[0].trim();
        await vscode.env.clipboard.writeText(cmd);
      }
    } catch (error) {
      // File might be untracked - just warn
      const fileName = relativePath.split("/").pop() || "";
      vscode.window.showWarningMessage(
        `Protected file modified: ${fileName}. Consider using the exo CLI.`
      );
    }
  }

  dispose(): void {
    this.watcher?.dispose();
    this.debounceTimers.forEach((timer) => clearTimeout(timer));
  }
}
```

### Edge Cases

1. **Untracked files**: Warn only, don't attempt git restore
2. **Git restore fails**: Show softer warning, log error
3. **Rapid successive edits**: Debounce to prevent thrashing
4. **VS Code restart**: Watcher reactivates on extension load
5. **Multi-root workspaces**: Watch each workspace root independently

### Integration with Existing Systems

- **files.readonlyInclude**: Works alongside (belt + suspenders)
- **exohook**: Shares configuration patterns from exosuit.toml
- **Machine Channel**: Can query protection status via `exo json status`
- **Steering**: Protection violations could surface in next_actions

## Implementation Plan

### Step 1: Configuration Schema (0.5 days)

- Add `[protected-files]` section to exosuit.toml schema
- Parse in exosuit-core

### Step 2: VS Code Watcher (1 day)

- Implement ProtectedFileWatcher service
- Wire to extension activation
- Add to extension.ts dispose chain

### Step 3: Notice UI (0.5 days)

- Toast notifications with action buttons
- Quick-pick for command selection

### Step 4: Testing (0.5 days)

- Unit tests for watcher logic
- E2E test: simulate agent edit, verify revert

## Testing Strategy

1. **Unit tests**: Mock file system events, verify debouncing
2. **Integration tests**: Real git restore on temp repo
3. **E2E tests**: Agent simulation - attempt direct edit, verify:
   - File is reverted
   - Notice is shown
   - Agent learns to use CLI

## Open Questions

1. **Diff logging**: Should we log the attempted diff before reverting?
2. **Soft protection**: Should some files warn-only (no revert)?
3. **Rate limiting**: If agent repeatedly violates, escalate severity?
4. **Undo support**: Allow user to undo the revert if intentional?

## References

- RFC 0081: Exohook File Expansion
- RFC 0022: Exohook Declarative Validation Lanes
- exosuit.toml configuration schema

