import * as vscode from "vscode";

export function run() {
  return Object.keys(vscode).filter((k) => k.startsWith("ChatResponse"));
}
