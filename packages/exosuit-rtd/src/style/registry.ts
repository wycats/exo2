import type * as CSS from "csstype";
import { TokenDefinition, RSLSuperProperty } from "./types.js";

export class TokenRegistry {
  private tokens = new Map<string, TokenDefinition>();

  constructor() {}

  register(slot: string, def: TokenDefinition) {
    const key = this.getKey(def.property, slot, def.id);
    this.tokens.set(key, def);
  }

  get(
    property: RSLSuperProperty,
    slot: string,
    id: string
  ): TokenDefinition | undefined {
    return this.tokens.get(this.getKey(property, slot, id));
  }

  private getKey(property: RSLSuperProperty, slot: string, id: string): string {
    return `${property}:${slot}:${id}`;
  }
}

export const defaultRegistry = new TokenRegistry();

// --- Default Tokens (Spec Implementation) ---

// 4.1 Layout
const layoutModes = {
  stack: { display: "flex", flexDirection: "column" },
  row: { display: "flex", flexDirection: "row", alignItems: "center" },
  grid: {
    display: "grid",
    gridTemplateColumns: "repeat(auto-fit, minmax(var(--rtd-min, 200px), 1fr))",
  },
  flow: { display: "block" },
} as const;

for (const [mode, css] of Object.entries(layoutModes)) {
  defaultRegistry.register("mode", {
    id: mode,
    property: "layout",
    css: css as CSS.Properties,
  });
}

// Layout Wrap
defaultRegistry.register("wrap", {
  id: "yes",
  property: "layout",
  css: { flexWrap: "wrap" },
});
defaultRegistry.register("wrap", {
  id: "no",
  property: "layout",
  css: { flexWrap: "nowrap" },
});

const spaces = ["xs", "sm", "md", "lg", "xl"] as const;
for (const space of spaces) {
  // Layout Gap
  defaultRegistry.register("gap", {
    id: space,
    property: "layout",
    css: { gap: `var(--rtd-space-${space})` },
  });

  // Spacing (Padding)
  defaultRegistry.register("all", {
    id: space,
    property: "spacing",
    css: { padding: `var(--rtd-space-${space})` },
  });
  defaultRegistry.register("x", {
    id: space,
    property: "spacing",
    css: { paddingInline: `var(--rtd-space-${space})` },
  });
  defaultRegistry.register("y", {
    id: space,
    property: "spacing",
    css: { paddingBlock: `var(--rtd-space-${space})` },
  });

  // Border Radius
  defaultRegistry.register("radius", {
    id: space,
    property: "border",
    css: { borderRadius: `var(--rtd-radius-${space})` },
  });

  // Text Size
  defaultRegistry.register("size", {
    id: space,
    property: "text",
    css: { fontSize: `var(--rtd-font-size-${space})` },
  });
}

// 4.2 Surface
const surfaces = ["surface-1", "surface-2", "surface-3"] as const;
for (const surface of surfaces) {
  defaultRegistry.register("base", {
    id: surface,
    property: "surface",
    css: {
      backgroundColor: `var(--rtd-color-${surface})`,
      color: "var(--rtd-color-text-primary)",
    },
  });
}
defaultRegistry.register("base", {
  id: "accent",
  property: "surface",
  css: {
    backgroundColor: "var(--rtd-color-accent)",
    color: "var(--rtd-color-text-inverse)",
  },
});
defaultRegistry.register("base", {
  id: "transparent",
  property: "surface",
  css: { backgroundColor: "transparent" },
});

// 4.4 Border
defaultRegistry.register("style", {
  id: "subtle",
  property: "border",
  css: { border: "1px solid var(--rtd-color-border-subtle)" },
});
defaultRegistry.register("style", {
  id: "bold",
  property: "border",
  css: { border: "2px solid var(--rtd-color-border-bold)" },
});

// 4.5 Text
defaultRegistry.register("weight", {
  id: "bold",
  property: "text",
  css: { fontWeight: 700 },
});
defaultRegistry.register("style", {
  id: "italic",
  property: "text",
  css: { fontStyle: "italic" },
});
defaultRegistry.register("family", {
  id: "mono",
  property: "text",
  css: { fontFamily: "var(--rtd-font-mono)" },
});
