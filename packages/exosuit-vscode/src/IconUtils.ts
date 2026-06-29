import * as vscode from "vscode";

export function getIconForFile(filename: string): vscode.ThemeIcon {
  const isAxiomFile =
    filename === "axioms.md" ||
    filename === "axioms.toml" ||
    (filename.startsWith("axioms.") &&
      (filename.endsWith(".toml") || filename.endsWith(".md")));

  if (isAxiomFile) {
    return new vscode.ThemeIcon("law");
  }

  switch (filename) {
    case "decisions.md":
      return new vscode.ThemeIcon("history");
    case "plan-outline.md":
      return new vscode.ThemeIcon("list-tree");
    case "task-list.md":
      return new vscode.ThemeIcon("checklist");
    case "ideas.md":
      return new vscode.ThemeIcon("lightbulb");
    case "walkthrough.md":
      return new vscode.ThemeIcon("map");
    case "implementation-plan.md":
      return new vscode.ThemeIcon("tools");
    default:
      return new vscode.ThemeIcon("file-text");
  }
}
