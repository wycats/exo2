/**
 * Structured Object Model (SOM) — a document-oriented rendering tree.
 *
 * Currently unused: the RichEditorProvider that consumed SOM was removed
 * during the SQLite-only storage migration. These types are preserved for
 * future reintegration when reactive views over SQLite-backed state are
 * ready to replace the old TOML-backed custom editor.
 */

export type SOMNode = SOMContainer | SOMList | SOMField | SOMControl;

export interface SOMRoot {
  kind: "root";
  schemaVersion: "1.0";
  children: SOMNode[];
  meta: {
    title: string;
    readonly: boolean;
    type?: string;
    phase?: {
      id: string;
      title: string;
      status: string;
    };
    frontmatter?: Record<string, any>;
  };
}

export interface SOMContainer {
  kind: "section" | "group";
  id: string;
  label?: string;
  children: SOMNode[];
  collapsed?: boolean; // UI State
  variant?: string;
}

export interface SOMList {
  kind: "list";
  id: string;
  label?: string;
  children: SOMNode[];
  collapsed?: boolean; // UI State
  itemSchema: SOMNode[]; // Template for new items
  allowAdd: boolean;
  allowReorder: boolean;
}

export interface BaseField {
  id: string;
  kind: string;
  path: string[]; // JSON Pointer segments (e.g., ["axioms", "0", "content"])
  label: string;
  description?: string;
  value: any;
  readonly: boolean;
  errors?: string[]; // Validation feedback
  // feedback?: FeedbackSummary; // Active comments (omitted for now)
}

export interface TextField extends BaseField {
  kind: "text";
  value: string;
  multiline: boolean;
  format: "plain" | "markdown";
  variant?: "title" | "content" | "id" | "simple" | "bare" | "default";
  badge?: {
    status?: "success" | "warning" | "error" | "info" | "neutral";
    label: string;
  };
}

export interface EnumField extends BaseField {
  kind: "enum";
  value: string;
  options: { label: string; value: string }[];
  display: "dropdown" | "radio" | "checkbox";
  variant?: "title" | "content" | "id" | "bare" | "default";
}

export interface BooleanField extends BaseField {
  kind: "boolean";
  value: boolean;
}

export interface ReferenceField extends BaseField {
  kind: "reference";
  value: string; // Entity ID
  targetType: string; // EntityType
}

export type SOMField = TextField | EnumField | BooleanField | ReferenceField;

export interface SOMControl {
  kind: "control";
  id: string;
  label: string;
  action: string;
}
