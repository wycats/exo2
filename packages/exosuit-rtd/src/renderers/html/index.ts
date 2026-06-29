import { RTDBlock, RTDInline } from "../../dom/types.js";
import { RSLStyle } from "../../style/om.js";
import { getLogger } from "../../logger.js";
import { TokenRegistry, defaultRegistry } from "../../style/registry.js";

export interface HTMLRendererOptions {
  registry?: TokenRegistry;
  /**
   * Optional callback to resolve styles for a given variant.
   * If provided, the renderer will inject a `style` attribute with the compiled CSS variables.
   */
  resolveStyle?: (variant: string) => RSLStyle | undefined;
  /**
   * List of allowed URL schemes (protocols).
   * Defaults to ["http", "https", "mailto"].
   * Relative URLs (starting with /, ./, ../) are always allowed.
   */
  allowedSchemes?: string[];
}

export class HTMLRenderer {
  private resolveStyle?: (variant: string) => RSLStyle | undefined;
  private allowedSchemes: Set<string>;

  constructor(options: HTMLRendererOptions = {}) {
    void (options.registry || defaultRegistry); // Registry reserved for future use
    this.resolveStyle = options.resolveStyle;
    this.allowedSchemes = new Set(
      options.allowedSchemes || ["http", "https", "mailto"],
    );
  }

  public render(blocks: RTDBlock[]): string {
    return blocks.map((block) => this.renderBlock(block)).join("");
  }

  protected isSafeUrl(url: string): boolean {
    if (!url) return false;

    // Allow relative URLs
    if (url.startsWith("/") || url.startsWith("./") || url.startsWith("../")) {
      return true;
    }

    try {
      const parsed = new URL(url);
      // Remove trailing colon from protocol
      const scheme = parsed.protocol.slice(0, -1);
      return this.allowedSchemes.has(scheme);
    } catch (e) {
      // Invalid URL, treat as unsafe if it doesn't look like a relative path
      return false;
    }
  }

  protected renderBlock(block: RTDBlock): string {
    switch (block.kind) {
      case "paragraph":
        return `<p>${this.renderInlines(block.children)}</p>`;
      case "heading":
        return `<h${block.level}>${this.renderInlines(block.children)}</h${
          block.level
        }>`;
      case "code-block":
        const langClass = block.language
          ? ` class="language-${block.language}"`
          : "";
        return `<pre><code${langClass}>${this.escapeHtml(
          block.value,
        )}</code></pre>`;
      case "blockquote":
        return `<blockquote>${this.render(block.children)}</blockquote>`;
      case "list":
        const tag = block.ordered ? "ol" : "ul";
        const items = block.items
          .map((item) => {
            const checkbox =
              item.checked !== undefined
                ? `<input type="checkbox" disabled${
                    item.checked ? " checked" : ""
                  } /> `
                : "";
            return `<li>${checkbox}${this.render(item.children)}</li>`;
          })
          .join("");
        return `<${tag}>${items}</${tag}>`;
      case "thematic-break":
        return "<hr />";
      case "table":
        const header = block.header.cells
          .map(
            (cell, i) =>
              `<th${this.getAlignAttr(
                block.alignments[i],
              )}>${this.renderInlines(cell.children)}</th>`,
          )
          .join("");
        const rows = block.rows
          .map(
            (row) =>
              `<tr>${row.cells
                .map(
                  (cell, i) =>
                    `<td${this.getAlignAttr(
                      block.alignments[i],
                    )}>${this.renderInlines(cell.children)}</td>`,
                )
                .join("")}</tr>`,
          )
          .join("");
        return `<table><thead><tr>${header}</tr></thead><tbody>${rows}</tbody></table>`;
      case "math-block":
        return `<div class="math-block">$$${this.escapeHtml(
          block.value,
        )}$$</div>`;
      case "alert":
        return this.renderContainer(block.variant, block.children);
      case "callout":
        return this.renderContainer(block.variant, block.children);
      case "container":
        return this.renderContainer(block.variant, block.children);
      default:
        getLogger().warn(`Unknown block kind: ${(block as any).kind}`);
        return "";
    }
  }

  protected getAlignAttr(align: "left" | "center" | "right" | null): string {
    return align ? ` align="${align}"` : "";
  }

  protected renderContainer(variant: string, children: RTDBlock[]): string {
    let styleAttr = "";

    if (this.resolveStyle) {
      const style = this.resolveStyle(variant);
      if (style) {
        const css = style.compile();
        const styleString = Object.entries(css)
          .map(([prop, value]) => `${this.kebabCase(prop)}: ${value}`)
          .join("; ");
        styleAttr = ` style="${styleString}"`;
      }
    }

    return `<div class="rtd-container rtd-variant-${variant}"${styleAttr}>${this.render(
      children,
    )}</div>`;
  }

  protected renderInlines(inlines: RTDInline[]): string {
    return inlines.map((inline) => this.renderInline(inline)).join("");
  }

  protected renderInline(inline: RTDInline): string {
    switch (inline.kind) {
      case "text":
        return this.escapeHtml(inline.value);
      case "strong":
        return `<strong>${this.renderInlines(inline.children)}</strong>`;
      case "emphasis":
        return `<em>${this.renderInlines(inline.children)}</em>`;
      case "strikethrough":
        return `<s>${this.renderInlines(inline.children)}</s>`;
      case "code-span":
        return `<code>${this.escapeHtml(inline.value)}</code>`;
      case "math-inline":
        return `<span class="math-inline">$${this.escapeHtml(
          inline.value,
        )}$</span>`;
      case "citation":
        return `<span class="citation">【${this.escapeHtml(
          inline.value,
        )}】</span>`;
      case "link":
        if (!this.isSafeUrl(inline.href)) {
          // Render as plain text (children only)
          return this.renderInlines(inline.children);
        }
        const titleAttr = inline.title
          ? ` title="${this.escapeHtml(inline.title)}"`
          : "";
        return `<a href="${this.escapeHtml(
          inline.href,
        )}"${titleAttr}>${this.renderInlines(inline.children)}</a>`;
      case "image":
        const imgTitle = inline.title
          ? ` title="${this.escapeHtml(inline.title)}"`
          : "";
        return `<img src="${this.escapeHtml(
          inline.src,
        )}" alt="${this.escapeHtml(inline.alt)}"${imgTitle} />`;
      case "icon":
        // Render as a span with codicon class for VS Code compatibility
        return `<span class="codicon codicon-${inline.name}"></span>`;
      case "command":
        // Render as a link with command: scheme
        return `<a href="command:${inline.id}">${this.renderInlines(
          inline.children,
        )}</a>`;
      default:
        return "";
    }
  }

  protected escapeHtml(text: string): string {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#039;");
  }

  protected kebabCase(str: string): string {
    return str.replace(/([a-z0-9])([A-Z])/g, "$1-$2").toLowerCase();
  }
}
