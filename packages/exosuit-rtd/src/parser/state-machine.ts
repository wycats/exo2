export enum BlockState {
  ROOT = "ROOT",
  HEADING = "HEADING",
  CODE_FENCE = "CODE_FENCE",
  CODE_BLOCK_META = "CODE_BLOCK_META",
  CODE_BLOCK_BODY = "CODE_BLOCK_BODY",
  CODE_BLOCK_INDENTED = "CODE_BLOCK_INDENTED",
  BLOCKQUOTE = "BLOCKQUOTE",
  PARAGRAPH = "PARAGRAPH",
  COMMENT = "COMMENT",
  XML_BLOCK = "XML_BLOCK",
  CONTAINER = "CONTAINER",
  TABLE = "TABLE",
}

export enum InlineState {
  TEXT = "TEXT",
  BOLD_START = "BOLD_START",
  BOLD_CONTENT = "BOLD_CONTENT",
  ITALIC_START = "ITALIC_START",
  ITALIC_CONTENT = "ITALIC_CONTENT",
  ICON_START = "ICON_START",
  ICON_NAME = "ICON_NAME",
}

export interface ParserState {
  blockState: BlockState;
  inlineState: InlineState;
  buffer: string[];
  tailBuffer: string;
  stack: any[]; // For nested blocks like blockquotes
}
