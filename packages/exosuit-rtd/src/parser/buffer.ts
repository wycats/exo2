/**
 * Identifies the "tail" of a chunk that might be an incomplete token.
 * Returns [safeChunk, tail].
 */
export function splitTail(chunk: string): [string, string] {
  if (chunk.length === 0) return ["", ""];

  // 1. Incomplete Line Check
  // If the chunk doesn't end with a newline, the last "line" is potentially incomplete.
  // However, we might be in the middle of a huge line.
  // For block parsing, we generally need complete lines to decide block type (e.g. heading, list item).
  // So buffering the last partial line is a safe default for block parsing.
  
  const lastNewline = chunk.lastIndexOf("\n");
  if (lastNewline === -1) {
    // No newline at all. The whole chunk is a partial line.
    return ["", chunk];
  }

  // We have at least one newline.
  // The content after the last newline is the partial line.
  const safePart = chunk.slice(0, lastNewline + 1); // Include the newline
  const tail = chunk.slice(lastNewline + 1);

  // 2. Incomplete Token Check (within the safe part? No, only at the very end of the stream)
  // But wait, if we split by line, the `tail` is already the incomplete line.
  // The `safePart` consists of complete lines.
  
  // However, what if a token spans across lines?
  // - Code blocks: handled by state machine (CODE_BLOCK_BODY).
  // - Multi-line blockquotes: handled by state machine.
  // - Paragraphs: handled by state machine.
  
  // The only tricky part is if a token *start* is split.
  // e.g. `\n` is the split point.
  // `\n` is a token separator usually.
  
  // What about inline tokens?
  // If we parse inlines *per line* (as we do now), then we don't need to worry about inline tokens spanning lines
  // EXCEPT for multi-line inline constructs if we supported them (like multi-line bold? CommonMark allows it).
  
  // If we support multi-line bold, then `**bold\ntext**` is valid.
  // Our current `parseInlines` is called on the accumulated paragraph text.
  // So if we have:
  // Chunk 1: "This is **bold" (no newline) -> Buffered as tail.
  // Chunk 2: " text**\n" -> Prepend tail -> "This is **bold text**\n".
  // Process line: "This is **bold text**".
  
  // So simply buffering the last partial line seems sufficient for most cases,
  // assuming we don't try to parse inlines until we have a full block (or at least a full line).
  
  // BUT, what if the chunk ends exactly on a newline, but that newline is part of a token?
  // e.g. `\n` inside a code block is fine.
  
  // What if the chunk ends with `\r` and the next is `\n`? (CRLF split)
  if (chunk.endsWith("\r")) {
      return [chunk.slice(0, -1), "\r"];
  }

  return [safePart, tail];
}
