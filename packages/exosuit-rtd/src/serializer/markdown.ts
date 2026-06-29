import type { RTDBlock, RTDInline } from "../dom/types.js";

export function serializeBlock(block: RTDBlock): string {
  switch (block.kind) {
    case "paragraph":
      return serializeInlines(block.children);
    case "heading":
      return "#".repeat(block.level) + " " + serializeInlines(block.children);
    case "code-block":
      return (
        "```" +
        (block.language || "") +
        "\n" +
        block.value +
        (block.value.endsWith("\n") ? "" : "\n") +
        "```"
      );
    case "blockquote":
      return (
        "> " +
        block.children.map(serializeBlock).join("\n> ").replace(/\n> $/, "")
      );
    case "list":
      return block.items
        .map((item, i) => {
          const prefix = block.ordered ? `${i + 1}. ` : "- ";
          const checkbox =
            item.checked !== undefined ? (item.checked ? "[x] " : "[ ] ") : "";
          return (
            prefix + checkbox + item.children.map(serializeBlock).join("\n  ")
          );
        })
        .join("\n");
    case "container":
      return (
        `::: ${block.variant}\n` +
        block.children.map(serializeBlock).join("\n") +
        "\n:::"
      );
    case "xml-block":
      return `<exo-${block.tagName} ${Object.entries(block.attributes)
        .map(([k, v]) => `${k}="${v}"`)
        .join(" ")}>${block.content}</exo-${block.tagName}>`;
    case "comment":
      return `<!--${block.value}-->`;
    default:
      return "";
  }
}

export function serializeInlines(inlines: RTDInline[]): string {
  return inlines.map(serializeInline).join("");
}

function serializeInline(inline: RTDInline): string {
  switch (inline.kind) {
    case "text":
      return inline.value;
    case "strong":
      return "**" + serializeInlines(inline.children) + "**";
    case "emphasis":
      return "_" + serializeInlines(inline.children) + "_";
    case "code-span":
      return "`" + inline.value + "`";
    case "link":
      return `[${serializeInlines(inline.children)}](${inline.href})`;
    case "image":
      return `![${inline.alt}](${inline.src})`;
    case "math-inline":
      return `$${inline.value}$`;
    case "icon":
      return `$(${inline.name})`;
    case "citation":
      return `【${inline.value}】`; // Or normalize back?
    case "comment":
      return `<!--${inline.value}-->`;
    default:
      return "";
  }
}
