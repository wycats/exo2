import * as vscode from "vscode";

export function formatHiddenToolOutput(
  toolOutputs: { name: string; output: string }[]
): vscode.MarkdownString {
  const toolMessageContent = toolOutputs
    .map((t) => `Tool '${t.name}' output:\n${t.output}`)
    .join("\n\n");

  // PERSISTENCE: Write tool output as a hidden comment so it survives in history
  // We must escape double hyphens to ensure valid HTML comments
  const safeToolMessageContent = toolMessageContent
    .replace(/--/g, "&#45;&#45;")
    .replace(/>/g, "&gt;");

  const hiddenComment = new vscode.MarkdownString(
    `\n<!-- EXO_TOOL_OUTPUT\n${safeToolMessageContent}\n-->`
  );
  hiddenComment.supportHtml = true;
  return hiddenComment;
}

export function createToolResponseMessages(
  toolOutputs: { name: string; output: string }[]
): vscode.LanguageModelChatMessage[] {
  const toolMessageContent = toolOutputs
    .map((t) => `Tool '${t.name}' output:\n${t.output}`)
    .join("\n\n");

  const messages: vscode.LanguageModelChatMessage[] = [];

  // Use the 'name' property to distinguish this from a human user
  messages.push(
    vscode.LanguageModelChatMessage.User(toolMessageContent, "Tool")
  );

  // System Kick (Binding: Named User Message "system")
  messages.push(
    vscode.LanguageModelChatMessage.User(
      "Tool execution completed. The user has viewed the result in a rich UI. Do NOT summarize the data. Proceed immediately to analysis or the next step.",
      "system"
    )
  );

  return messages;
}
