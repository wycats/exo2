<!-- exo:94 ulid:01kg5kp2fntpdb5b70qmf0emd8 -->

# RFC 94: Sidebar-First UI Design



# RFC 0094: Sidebar-First UI Design

- **Superseded by**: RFC 10172


## Summary

A design philosophy for Exosuit's VS Code extension UI, prioritizing density, native integration, and glanceability within the sidebar constraints.

## Motivation

The initial dashboard implementation felt like a "web page stuffed into a sidebar" — large headers, card-based layouts with heavy padding, and low information density. This RFC establishes a "Sidebar-First" design standard to ensure all future UI components feel like native extensions of the VS Code environment.

## Core Principles

### 1. The Sidebar is the Stage

Assume a default width of ~300px. UI must be responsive but optimized for this narrow column. Horizontal scrolling is a failure state.

### 2. Density is Data

In a coding environment, screen real estate is precious.

- **Avoid**: Large padding (e.g., `20px`), card containers with drop shadows, `<h1>` page titles.
- **Prefer**: Compact lists, 4-8px padding, 1px borders for separation.

### 3. Native Camouflage

The UI should not look like a "website" embedded in VS Code. It should look like VS Code itself.

- Use `var(--vscode-...)` CSS variables for all colors (backgrounds, foregrounds, borders, inputs).
- Match the typography and font sizes of the editor (usually 13px).
- Use standard VS Code icons (Codicons).

## Design Patterns

### Section Headers

Instead of `<h2>` tags, use the "Section Header" pattern:

- Uppercase text.
- Small font size (~11px).
- Bold weight.
- Background color: `--vscode-sideBarSectionHeader-background`.
- Foreground color: `--vscode-sideBarSectionHeader-foreground`.

### Lists over Cards

Data should be presented in flat lists rather than individual cards.

- **Separators**: Use 1px solid borders (`--vscode-tree-tableOddRowsBackground` or `--vscode-widget-border`) to separate items.
- **Hover States**: Highlight items on hover using `--vscode-list-hoverBackground`.

### Status Indicators

Use color-coded badges or dots, but ensure they use accessible VS Code theme colors:

- Info/Open: `--vscode-charts-blue`
- Success/Resolved: `--vscode-charts-green`
- Warning: `--vscode-charts-yellow`
- Error: `--vscode-charts-red`

## Implementation Checklist

- [ ] Remove all `box-shadow`.
- [ ] Ensure no font size exceeds `1.2em`.
- [ ] Verify high-contrast theme support (by relying solely on VS Code variables).
- [ ] Test at minimum sidebar width.
