import * as vscode from "vscode";

export function test() {
  const uri = vscode.Uri.file("/tmp/test");
  const edit = new vscode.TextEdit(new vscode.Range(0, 0, 0, 0), "test");
  // @ts-ignore
  const part = new vscode.ChatResponseTextEditPart(uri, edit);
}
