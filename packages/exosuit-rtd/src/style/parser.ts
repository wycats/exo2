import { RSLStyle } from "./om.js";
import { TokenRegistry, defaultRegistry } from "./registry.js";
import { RSLSuperProperty } from "./types.js";
import { getLogger } from "../logger.js";

export class RSLParser {
  constructor(private registry: TokenRegistry = defaultRegistry) {}

  /**
   * Parses a block of RSL declarations (contents of a rule).
   * Example:
   *   layout: mode stack, gap md;
   *   spacing: x sm, y lg;
   */
  parse(rslText: string): RSLStyle {
    const style = new RSLStyle(this.registry);

    // Remove comments
    const cleanText = rslText.replace(/\/\*[\s\S]*?\*\//g, "");

    const declarations = cleanText
      .split(";")
      .map((d) => d.trim())
      .filter(Boolean);

    for (const decl of declarations) {
      const separatorIndex = decl.indexOf(":");
      if (separatorIndex === -1) continue;

      const propName = decl.slice(0, separatorIndex).trim();
      const value = decl.slice(separatorIndex + 1).trim();

      if (this.isSuperProperty(propName)) {
        this.parseProperty(style, propName, value);
      } else {
        getLogger().warn(`RSL Parser: Unknown property '${propName}'`);
      }
    }

    return style;
  }

  private parseProperty(
    style: RSLStyle,
    property: RSLSuperProperty,
    value: string
  ) {
    // Value format: "slot token, slot token"
    const assignments = value.split(",").map((s) => s.trim());

    for (const assignment of assignments) {
      // Format: "slot token"
      const parts = assignment.split(/\s+/);
      if (parts.length !== 2) {
        getLogger().warn(
          `RSL Parser: Invalid assignment '${assignment}' for property '${property}'. Expected 'slot token'.`
        );
        continue;
      }

      const [slot, token] = parts;
      // @ts-ignore - Dynamic access to typed properties
      style[property].add(slot, token);
    }
  }

  private isSuperProperty(name: string): name is RSLSuperProperty {
    return ["layout", "surface", "spacing", "border", "text"].includes(name);
  }
}
