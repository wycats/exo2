export type ProjectionFormat = "text" | "markdown" | "json";

export interface TruncateOptions {
  ellipsis?: string;
  notice?: (meta: {
    originalLength: number;
    maxChars: number;
    truncatedLength: number;
  }) => string;
}

export interface TruncateResult {
  text: string;
  truncated: boolean;
  originalLength: number;
  maxChars: number;
}

export function truncateWithNotice(
  text: string,
  maxChars: number,
  options: TruncateOptions = {}
): TruncateResult {
  const input = String(text ?? "");
  const limit = Number.isFinite(maxChars) ? Math.max(0, maxChars) : 0;

  if (input.length <= limit) {
    return {
      text: input,
      truncated: false,
      originalLength: input.length,
      maxChars: limit,
    };
  }

  const ellipsis = options.ellipsis ?? "…";
  const truncatedBase = input.slice(0, Math.max(0, limit - ellipsis.length));
  const truncatedText = `${truncatedBase}${ellipsis}`;

  const notice =
    options.notice?.({
      originalLength: input.length,
      maxChars: limit,
      truncatedLength: truncatedText.length,
    }) ??
    `\n\n[TRUNCATED: ${input.length} chars → ${limit} char budget]`;

  return {
    text: `${truncatedText}${notice}`,
    truncated: true,
    originalLength: input.length,
    maxChars: limit,
  };
}

export interface ProjectionSection {
  title: string;
  format?: ProjectionFormat;
  content: string;
}

export function formatProjectionSection(section: ProjectionSection): string {
  const title = String(section.title ?? "");
  const content = String(section.content ?? "");
  const format: ProjectionFormat = section.format ?? "text";

  if (format === "markdown") {
    return `## ${title}\n\n${content}\n`;
  }

  if (format === "json") {
    return `=== ${title} (JSON) ===\n${content}\n`;
  }

  return `=== ${title} ===\n${content}\n`;
}

function sortKeysDeep(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(sortKeysDeep);
  }

  if (value && typeof value === "object") {
    const record = value as Record<string, unknown>;
    const keys = Object.keys(record).sort((a, b) => a.localeCompare(b));
    const out: Record<string, unknown> = {};
    for (const key of keys) {
      out[key] = sortKeysDeep(record[key]);
    }
    return out;
  }

  return value;
}

export function stableJsonStringify(value: unknown, indent: number = 2): string {
  return JSON.stringify(sortKeysDeep(value), null, indent);
}
