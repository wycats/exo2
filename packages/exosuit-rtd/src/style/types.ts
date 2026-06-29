import type * as CSS from "csstype";

export type RSLSuperProperty =
  | "layout"
  | "surface"
  | "spacing"
  | "border"
  | "text";

export interface TokenDefinition {
  id: string;
  property: RSLSuperProperty;
  css: CSS.Properties;
}

// --- Slot Definitions for Strong Typing ---

export type SpaceScale = "xs" | "sm" | "md" | "lg" | "xl";
export type ColorScale =
  | "surface-1"
  | "surface-2"
  | "surface-3"
  | "accent"
  | "transparent";
export type BorderStyle = "subtle" | "bold";
export type TextSize = "xs" | "sm" | "md" | "lg" | "xl";
export type TextWeight = "bold" | "normal";
export type TextFamily = "mono" | "sans";

export interface LayoutSlots {
  mode?: "stack" | "row" | "grid" | "flow";
  gap?: SpaceScale;
  wrap?: "yes" | "no";
}

export interface SurfaceSlots {
  base?: ColorScale;
}

export interface SpacingSlots {
  all?: SpaceScale;
  x?: SpaceScale;
  y?: SpaceScale;
}

export interface BorderSlots {
  style?: BorderStyle;
  radius?: SpaceScale;
}

export interface TextSlots {
  size?: TextSize;
  weight?: TextWeight;
  family?: TextFamily;
  style?: "italic" | "normal";
}
