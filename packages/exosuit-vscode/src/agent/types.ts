import type * as vscode from "vscode";
import type { z } from "zod";

/**
 * Represents the context of a single turn in the conversation.
 */
export interface TurnContext {
  /** The VS Code Chat Context (history, etc.) */
  vscodeContext: vscode.ChatContext;
  /** The accumulated history of messages for the LLM */
  history: vscode.LanguageModelChatMessage[];
  /** Maximum number of turns allowed to prevent infinite loops */
  maxTurns: number;
  /** The model being used */
  model: vscode.LanguageModelChat;
}

/**
 * Represents the result of a tool execution.
 */
export interface ToolResult {
  /** The ID of the tool call (if provided by LLM) or generated */
  callId: string;
  /** The name of the tool executed */
  toolName: string;
  /** The string output of the tool */
  content: string;
  /** Whether the execution resulted in an error */
  isError?: boolean;
}

/**
 * A definition of a tool that can be used by the agent.
 */
export interface ToolDefinition<T = any> {
  /** The name of the tool (e.g., 'readFile') */
  name: string;
  /** A description of what the tool does */
  description: string;
  /** The Zod schema for the tool's arguments */
  schema: z.ZodType<T>;
  /** The function to execute when the tool is called */
  execute: (args: T, context: TurnContext) => Promise<ToolResult>;
}

/**
 * Represents a parsed tool call from the LLM.
 */
export interface ToolCall {
  callId: string;
  name: string;
  arguments: any;
}

/**
 * The states of the Agent Loop.
 */
export type AgentState =
  | { type: "Idle" }
  | { type: "Thinking"; attempt: number }
  | { type: "ExecutingTool"; toolName: string }
  | { type: "AwaitingLLM"; toolResults: ToolResult[] }
  | { type: "StreamingResponse" }
  | { type: "Done" };
