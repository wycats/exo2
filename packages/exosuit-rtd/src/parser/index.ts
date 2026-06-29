import type { RTDBlock } from "../dom/types.js";
import { StreamingParser } from "./streaming.js";

export * from "./state-machine.js";
export * from "./streaming.js";

export function parseMarkdown(markdown: string): RTDBlock[] {
  const parser = new StreamingParser();
  parser.parse(markdown);
  return parser.flush();
}
