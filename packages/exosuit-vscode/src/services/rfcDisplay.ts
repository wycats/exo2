/**
 * Shared RFC display utilities for TreeView, CLI, and Svelte components.
 *
 * Layer 1: Pure data model with no UI framework dependencies.
 * See RFC 00239 for the visual language specification.
 */

// === Types ===

export type LifecycleStatus = "idle" | "in-progress" | "validating" | "done";

export interface RfcDisplayState {
  id: string; // "00225"
  title: string;
  currentStage: number; // 0-4
  targetStages?: number[]; // [2, 3] for multi-stage advancement
  lifecycleStatus?: LifecycleStatus;
  isInMotion: boolean; // Derived from active phase linkage
  role?: "driving" | "related" | "blocked";
}

export type DotStatus =
  | "completed" // ●
  | "future" // ○
  | "idle" // ◌ (only when isInMotion && isTarget)
  | "in-progress" // ◔ (only when isInMotion && isTarget)
  | "validating"; // ◕ (only when isInMotion && isTarget)

export interface DotData {
  /** The Unicode glyph to render: "●", "○", "◌", "◔", "◕" */
  glyph: string;
  /** Stage number (1-4) */
  stage: number;
  /**
   * Combined status for rendering:
   * - "completed": Stage reached (●)
   * - "future": Stage not yet reached, not a target (○)
   * - "idle" | "in-progress" | "validating": Lifecycle status at target position
   */
  status: DotStatus;
  /** True if this position is a target for advancement */
  isTarget: boolean;
}

// === Glyph Constants ===

export const GLYPHS = {
  completed: "●", // U+25CF BLACK CIRCLE
  future: "○", // U+25CB WHITE CIRCLE
  idle: "◌", // U+25CC DOTTED CIRCLE
  inProgress: "◔", // U+25D4 CIRCLE WITH UPPER RIGHT QUADRANT BLACK
  validating: "◕", // U+25D5 CIRCLE WITH ALL BUT UPPER LEFT QUADRANT BLACK
} as const;

// === Functions ===

/**
 * Format an RFC ID for display.
 *
 * @param id - The RFC ID (e.g., "00225", "225", "0225")
 * @param format - "short" for "#225", "full" for "00225"
 * @returns Formatted ID string
 */
export function formatRfcId(
  id: string,
  format: "short" | "full" = "short"
): string {
  // Normalize to 5-digit string
  const normalized = id.replace(/^0+/, "").padStart(5, "0");

  if (format === "full") {
    return normalized; // "00225"
  }

  // Short format: strip leading zeros, add # prefix
  return `#${parseInt(normalized, 10)}`; // "#225"
}

/**
 * Compute structured dot data for Svelte components.
 *
 * Returns an array of 4 DotData objects representing stages 1-4.
 * When isInMotion=true, target positions show lifecycle glyphs.
 * When isInMotion=false, all non-completed stages show as "future".
 *
 * Multi-stage advancement: Only the first pending target shows the active
 * lifecycle status (idle/in-progress/validating). Later targets show as
 * queued (◌ idle).
 *
 * Examples (S1→3 advancement):
 * - ●◌◌○  Both S2 and S3 queued (idle)
 * - ●◔◌○  Working on S2, S3 still queued
 * - ●●◌○  S2 done, S3 now the active target (idle)
 * - ●●◔○  Working on S3
 * - ●●●○  S3 reached
 *
 * @param state - The RFC display state
 * @returns Array of 4 DotData objects
 */
export function computeStageDots(state: RfcDisplayState): DotData[] {
  const { currentStage, targetStages = [], isInMotion, lifecycleStatus } = state;
  const dots: DotData[] = [];

  // Find the first pending target (the one we're actively working on)
  const sortedTargets = [...targetStages].sort((a, b) => a - b);
  const activeTarget = sortedTargets.find((t) => t > currentStage);

  for (let stage = 1; stage <= 4; stage++) {
    const isCompleted = stage <= currentStage;
    const isTarget = targetStages.includes(stage);

    let status: DotStatus;
    let glyph: string;

    if (isCompleted) {
      status = "completed";
      glyph = GLYPHS.completed;
    } else if (isTarget && isInMotion) {
      // Only the active target shows the lifecycle status
      // Later targets show as queued (idle)
      const isActiveTarget = stage === activeTarget;
      const lifecycle = isActiveTarget ? (lifecycleStatus ?? "idle") : "idle";

      if (lifecycle === "done") {
        // "done" means this stage is complete
        status = "completed";
        glyph = GLYPHS.completed;
      } else {
        status = lifecycle;
        glyph =
          lifecycle === "idle"
            ? GLYPHS.idle
            : lifecycle === "in-progress"
              ? GLYPHS.inProgress
              : GLYPHS.validating;
      }
    } else {
      status = "future";
      glyph = GLYPHS.future;
    }

    dots.push({ glyph, stage, status, isTarget });
  }

  return dots;
}

/**
 * Render stage dots as a string for TreeView labels and CLI output.
 *
 * @param state - The RFC display state
 * @returns 4-character string like "●●○○" or "●◔○○"
 */
export function renderStageDots(state: RfcDisplayState): string {
  return computeStageDots(state)
    .map((d) => d.glyph)
    .join("");
}
