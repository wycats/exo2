<!-- exo:10169 ulid:01kmzxeffjjapa4v7ej0ebdfn0 -->



# RFC 10169: FileDecoration-Based Tree Item Styling

- **Superseded by**: RFC 10172


## Summary

Use VS Code's `FileDecorationProvider` + `resourceUri` to color tree item labels, replacing the current ThemeIcon-only approach for status indication. Combined with the zero-width icon trick (`iconPath = undefined`), this enables compact, colorful tree items without the 20px icon gutter.

## Motivation

The current Phase Details tree uses `ThemeIcon` with `ThemeColor` for status colors (green=completed, yellow=in-progress, etc.). This works but has a tradeoff:

- ThemeIcon occupies a ~20px gutter column even for simple status indicators
- The icon gutter adds visual weight and horizontal space consumption
- There's no way to color the label text itself — only the icon glyph

## Design

### Core Mechanism

1. **`resourceUri`**: Assign each TreeItem a URI with a custom scheme encoding its status:

   ```
   exosuit-tree://task/completed/some-task-id
   exosuit-tree://goal/in-progress/some-goal-id
   exosuit-tree://log/info/some-log-id
   ```

2. **`FileDecorationProvider`**: Register a single provider that parses the URI and returns:
   - `color`: ThemeColor for the label text (green, yellow, gray, etc.)
   - `badge`: Optional 1-2 char suffix (e.g., task counts on goals)
   - `propagate`: Optional parent-state bubbling

3. **Zero-width icon trick**: Set `iconPath = undefined` to remove the icon gutter, use Unicode symbols in the label text for status glyphs.

### What This Unlocks

| Capability           | Before (ThemeIcon only) | After (FileDecoration)                  |
| -------------------- | ----------------------- | --------------------------------------- |
| Colored label text   | No                      | Yes                                     |
| Badge suffix         | No                      | Yes (1-2 chars, truncates with content) |
| Parent propagation   | Manual computation      | Free via `propagate` flag               |
| Icon gutter          | Always 20px             | Optional (zero-width trick)             |
| Status color channel | Icon glyph only         | Entire label text                       |

### API Surface

```typescript
// FileDecoration properties available:
interface FileDecoration {
  badge?: string; // max 2 chars, rendered after description, truncates with content
  tooltip?: string; // hover text for the decoration
  color?: ThemeColor; // colors the ENTIRE label text
  propagate?: boolean; // bubbles decoration to parent items
}
```

### Key Constraints

- `badge` truncates with content (not pinned like action buttons) — unreliable for narrow panes
- `description` text is always theme-muted gray — cannot be colored
- `TreeItemLabel.highlights` provides bold ranges only, not color
- `color` applies to label text AND badge text

### Design Decision: When to Use Which

| Item Type        | Approach                                       | Rationale                                                |
| ---------------- | ---------------------------------------------- | -------------------------------------------------------- |
| Tasks, Goals     | FileDecoration color + Unicode symbol in label | Status color on text is more informative than icon color |
| Log items, notes | Zero-width icon + no decoration                | Color doesn't matter, save space                         |
| Section headers  | Zero-width icon + no decoration                | Structural, not status-bearing                           |

## Open Questions

- Should we define custom ThemeColor IDs (e.g., `exosuit.task.completed`) or reuse existing ones?
- How to handle the `onDidChangeFileDecorations` event efficiently when task status changes?
- Should `propagate` be used for goal→epoch status bubbling, or is explicit computation better?