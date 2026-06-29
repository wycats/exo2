export type InterpolationContext = Record<string, unknown>;

const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_]*$/;
const TOKEN_PATTERN = /\{([^{}]+)\}/g;

/**
 * Strict interpolation following docs/specs/tooling-interface.md.
 *
 * - Tokens look like `{key}` (whitespace inside braces is ignored).
 * - If the key is invalid (e.g. nested) the token is preserved verbatim.
 * - If the key is missing from the context, the token is preserved verbatim.
 */
export function interpolateStrict(
  template: string,
  context: InterpolationContext
): string {
  return template.replace(TOKEN_PATTERN, (full: string, inner: string) => {
    const key = String(inner ?? "").trim();
    if (!IDENTIFIER.test(key)) return full;
    const value = context[key];
    return value !== undefined && value !== null ? String(value) : full;
  });
}
