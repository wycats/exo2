import { z } from "zod";

export const InterpolationKeySchema = z
  .string()
  .trim()
  .regex(/^[A-Za-z_][A-Za-z0-9_]*$/, {
    message:
      "Interpolation key must be an identifier (letters, numbers, underscore) and cannot be nested.",
  });

export type PromptSpecError = {
  path: string;
  message: string;
};

export type PromptSpecValidationResult = {
  ok: boolean;
  errors: PromptSpecError[];
};

export function validateTemplateInterpolation(
  template: string,
  pathForErrors: string
): PromptSpecValidationResult {
  const errors: PromptSpecError[] = [];

  // Matches "{ ... }" but does not allow nested braces.
  // We validate the interior using InterpolationKeySchema after trimming.
  const tokenPattern = /\{([^{}]+)\}/g;
  for (const match of template.matchAll(tokenPattern)) {
    const raw = match[1];
    const key = raw.trim();

    const parsed = InterpolationKeySchema.safeParse(key);
    if (!parsed.success) {
      errors.push({
        path: pathForErrors,
        message: `Invalid interpolation token "{${raw}}"`,
      });
    }
  }

  return { ok: errors.length === 0, errors };
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.getPrototypeOf(value) === Object.prototype
  );
}

export function validatePromptSpec(spec: unknown): PromptSpecValidationResult {
  const errors: PromptSpecError[] = [];

  function visit(value: unknown, pathParts: string[]): void {
    const path = pathParts.length === 0 ? "<root>" : pathParts.join(".");

    if (typeof value === "string") {
      const interpolation = validateTemplateInterpolation(value, path);
      errors.push(...interpolation.errors);
      return;
    }

    if (Array.isArray(value)) {
      value.forEach((item, index) => visit(item, [...pathParts, String(index)]));
      return;
    }

    if (isPlainObject(value)) {
      for (const [key, child] of Object.entries(value)) {
        visit(child, [...pathParts, key]);
      }
      return;
    }

    // For prompt configs, leaf values should generally be strings. Treat other
    // primitives as validation errors so user mistakes are caught early.
    if (value !== undefined && value !== null) {
      errors.push({
        path,
        message: `Invalid prompt value type: ${typeof value} (expected string or table)`,
      });
    }
  }

  if (!isPlainObject(spec)) {
    return {
      ok: false,
      errors: [{ path: "<root>", message: "PromptSpec must be a TOML table" }],
    };
  }

  visit(spec, []);
  return { ok: errors.length === 0, errors };
}

export const ResourceSpecSchema = z.object({
  type_id: z.string().min(1),
  args: z.unknown(),
});

export type ResourceSpec = z.infer<typeof ResourceSpecSchema>;
