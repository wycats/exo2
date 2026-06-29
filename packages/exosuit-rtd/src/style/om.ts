import type * as CSS from "csstype";
import { TokenRegistry, defaultRegistry } from "./registry.js";
import {
  RSLSuperProperty,
  LayoutSlots,
  SurfaceSlots,
  SpacingSlots,
  BorderSlots,
  TextSlots,
  TokenDefinition,
} from "./types.js";
import { getLogger } from "../logger.js";

export class RSLToken {
  constructor(public readonly def: TokenDefinition) {}

  get id() {
    return this.def.id;
  }
  get css() {
    return this.def.css;
  }
}

export class RSLProperty<Slots> {
  private activeTokens = new Map<string, RSLToken>();

  constructor(
    public readonly name: RSLSuperProperty,
    private registry: TokenRegistry
  ) {
    if (!registry) {
      throw new Error("TokenRegistry is required");
    }
  }

  /**
   * Set tokens for this property using a typed configuration object.
   * Keys correspond to slots, values correspond to token IDs.
   */
  set(config: Partial<Slots>): this {
    for (const [slot, tokenId] of Object.entries(config)) {
      if (tokenId) {
        this.addToken(tokenId as string, slot);
      }
    }
    return this;
  }

  /**
   * Low-level method to add a token by ID.
   * Requires explicit slot.
   */
  add(slot: string, tokenId: string): this {
    return this.addToken(tokenId, slot);
  }
  private addToken(tokenId: string, slot: string): this {
    const def = this.registry.get(this.name, slot, tokenId);
    if (!def) {
      getLogger().warn(
        `RSL Warning: Unknown token '${tokenId}' for property '${this.name}' in slot '${slot}'`
      );
      return this;
    }

    const token = new RSLToken(def);
    this.activeTokens.set(slot, token);
    return this;
  }

  compile(): CSS.Properties {
    const css: any = {};

    const allTokens = [...this.activeTokens.values()];

    for (const token of allTokens) {
      // Merge Policy Implementation
      for (const [prop, value] of Object.entries(token.css)) {
        // 3.3.3 Stackable Properties
        if (isStackable(prop)) {
          css[prop] = css[prop] ? `${css[prop]} ${value}` : value;
        } else {
          // 3.3.4 Default Replacement (Last Write Wins)
          css[prop] = value;
        }
      }
    }

    return css;
  }
}

function isStackable(prop: string): boolean {
  return ["transform", "filter", "backdropFilter", "transition"].includes(prop);
}

export class RSLStyle {
  readonly layout: RSLProperty<LayoutSlots>;
  readonly surface: RSLProperty<SurfaceSlots>;
  readonly spacing: RSLProperty<SpacingSlots>;
  readonly border: RSLProperty<BorderSlots>;
  readonly text: RSLProperty<TextSlots>;

  constructor(private registry: TokenRegistry = defaultRegistry) {
    this.layout = new RSLProperty<LayoutSlots>("layout", this.registry);
    this.surface = new RSLProperty<SurfaceSlots>("surface", this.registry);
    this.spacing = new RSLProperty<SpacingSlots>("spacing", this.registry);
    this.border = new RSLProperty<BorderSlots>("border", this.registry);
    this.text = new RSLProperty<TextSlots>("text", this.registry);
  }

  compile(): CSS.Properties {
    return {
      ...this.layout.compile(),
      ...this.surface.compile(),
      ...this.spacing.compile(),
      ...this.border.compile(),
      ...this.text.compile(),
      // containerType: "inline-size", // REMOVED: Causes size containment which collapses auto-width elements to 0
    };
  }
}
