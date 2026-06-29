import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkStringify from "remark-stringify";
import remarkGfm from "remark-gfm";
import { visit } from "unist-util-visit";
import type { PlanItem, Task, PlanTree, TaskList } from "./types.js";
import type { Node } from "unist";

interface ListItem extends Node {
  type: "listItem";
  checked?: boolean | null;
  children: Node[];
}

interface Paragraph extends Node {
  type: "paragraph";
  children: TextNode[];
}

interface TextNode extends Node {
  type: "text";
  value: string;
}

export class ExosuitParser {
  private processor = unified()
    .use(remarkParse)
    .use(remarkGfm)
    .use(remarkStringify);

  async parsePlan(markdown: string): Promise<PlanTree> {
    const tree = this.processor.parse(markdown);
    const items: PlanItem[] = [];
    let currentEpoch: PlanItem | null = null;
    let currentPhase: PlanItem | null = null;
    let inTemplate = false;

    for (const child of (tree as any).children) {
      if (child.type === "html") {
        if (child.value.includes("agent-template start")) {
          inTemplate = true;
          continue;
        }
        if (child.value.includes("agent-template end")) {
          inTemplate = false;
          continue;
        }
      }

      if (inTemplate) {
        continue;
      }

      if (child.type === "heading") {
        const title = this.extractText(child);
        const id = this.generateId(title);
        const status = this.extractStatus(title);

        if (child.depth === 2) {
          // Epoch
          currentEpoch = {
            id,
            title,
            status,
            type: "epoch",
            children: [],
          };
          items.push(currentEpoch);
          currentPhase = null; // Reset phase when new epoch starts
        } else if (child.depth === 3 && currentEpoch) {
          // Phase
          currentPhase = {
            id,
            title,
            status,
            type: "phase",
            children: [],
          };
          currentEpoch.children.push(currentPhase);
        }
      } else if (child.type === "list" && currentPhase) {
        // Tasks within a phase
        currentPhase.children.push(...this.parsePhaseTasks(child));
      } else if (child.type === "list" && !currentEpoch && !currentPhase) {
        // Fallback for list-based plans (legacy support)
        items.push(...this.parseListItems(child));
      }
    }

    return { items };
  }

  async parseTaskTree(markdown: string): Promise<PlanTree> {
    const tree = this.processor.parse(markdown);
    const items: PlanItem[] = [];
    const stack: { item: PlanItem; depth: number }[] = [];

    const getParent = (depth: number) => {
      while (stack.length > 0 && stack[stack.length - 1].depth >= depth) {
        stack.pop();
      }
      return stack.length > 0 ? stack[stack.length - 1].item : null;
    };

    for (const child of (tree as any).children) {
      if (child.type === "html") {
        if (child.value.includes("agent-template start")) {
          continue;
        }
        if (child.value.includes("agent-template end")) {
          continue;
        }
      }

      if (child.type === "heading") {
        const title = this.extractText(child);
        const id = this.generateId(title);
        const item: PlanItem = {
          id,
          title,
          status: "todo",
          type: "section",
          children: [],
        };

        const parent = getParent(child.depth);
        if (parent) {
          parent.children!.push(item);
        } else {
          items.push(item);
        }
        stack.push({ item, depth: child.depth });
      } else if (child.type === "list") {
        const parent = stack.length > 0 ? stack[stack.length - 1].item : null;
        const listItems = this.parseListItemsRecursive(child);
        if (parent) {
          parent.children!.push(...listItems);
        } else {
          items.push(...listItems);
        }
      }
    }

    return { items };
  }

  private parseListItemsRecursive(listNode: any): PlanItem[] {
    const items: PlanItem[] = [];
    for (const child of listNode.children) {
      if (child.type === "listItem") {
        const item = this.extractPlanItemFromNode(child);
        if (item) {
          items.push(item);
        }
      }
    }
    return items;
  }

  private extractText(node: any): string {
    if (!node.children) return "";
    return node.children
      .map((c: any) => {
        if (c.type === "text" || c.type === "inlineCode") return c.value;
        if (c.children) return this.extractText(c);
        return "";
      })
      .join("")
      .trim();
  }

  private generateId(text: string): string {
    return text
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "");
  }

  private extractStatus(title: string): "todo" | "in-progress" | "done" {
    if (title.includes("(Completed)")) return "done";
    if (title.includes("(Active)")) return "in-progress";
    return "todo";
  }

  private parsePhaseTasks(listNode: any): PlanItem[] {
    const items: PlanItem[] = [];
    for (const child of listNode.children) {
      if (child.type === "listItem") {
        const task = this.extractTaskFromNode(child);
        if (task) {
          items.push({
            id: task.id,
            title: task.title,
            status: task.status,
            type: "task",
            children: [],
          });
        }
      }
    }
    return items;
  }

  private parseListItems(listNode: any): PlanItem[] {
    const items: PlanItem[] = [];
    for (const child of listNode.children) {
      if (child.type === "listItem") {
        const item = this.extractPlanItemFromNode(child);
        if (item) {
          items.push(item);
        }
      }
    }
    return items;
  }

  private extractPlanItemFromNode(node: ListItem): PlanItem | null {
    let title = "";
    let id = "";
    let status: "todo" | "in-progress" | "done" = "todo";
    const children: PlanItem[] = [];

    // Process children of the list item
    for (const child of node.children) {
      if (child.type === "paragraph") {
        // Extract title and ID from paragraph
        for (const textNode of (child as any).children) {
          if (textNode.type === "text") {
            title += textNode.value;
          } else if (
            textNode.type === "strong" ||
            textNode.type === "emphasis"
          ) {
            if ((textNode as any).children) {
              for (const subChild of (textNode as any).children) {
                if (subChild.type === "text") {
                  title += subChild.value;
                }
              }
            }
          } else if (textNode.type === "inlineCode") {
            title += textNode.value;
          } else if (textNode.type === "html") {
            const match = textNode.value.match(/<!--\s*id:\s*"([^"]+)"\s*-->/);
            if (match) {
              id = match[1];
            }
          }
        }
      } else if (child.type === "list") {
        // Recurse for children
        children.push(...this.parseListItemsRecursive(child));
      }
    }

    if (node.checked) {
      status = "done";
    }

    if (!id) {
      id = this.generateId(title);
    }

    return {
      id,
      title: title.trim(),
      status,
      type: "task",
      children,
    };
  }

  async parseTasks(markdown: string): Promise<TaskList> {
    const tree = this.processor.parse(markdown);
    const tasks: Task[] = [];

    visit(tree, "listItem", (node: ListItem) => {
      if (node.checked !== null && node.checked !== undefined) {
        const task = this.extractTaskFromNode(node);
        if (task) {
          tasks.push(task);
        }
      }
    });

    return { tasks };
  }

  private extractTaskFromNode(node: ListItem): Task | null {
    // Extract text and ID from the list item
    // This is a simplified implementation
    let title = "";
    let id = "";
    let relatesTo = undefined;

    const firstChild = node.children[0] as Paragraph;
    if (firstChild && firstChild.type === "paragraph") {
      for (const child of firstChild.children) {
        if (child.type === "text") {
          title += child.value;
        } else if (child.type === "strong" || child.type === "emphasis") {
          // Handle bold/italic text by extracting its value
          // This is crucial for tasks like "**Category**"
          if ((child as any).children) {
            for (const subChild of (child as any).children) {
              if (subChild.type === "text") {
                title += subChild.value;
              }
            }
          }
        } else if (child.type === "inlineCode") {
          title += child.value;
        } else if (child.type === "html") {
          // Parse HTML comment for ID
          const match = child.value.match(/<!--\s*id:\s*"([^"]+)"\s*-->/);
          if (match) {
            id = match[1];
          }
          const relatesMatch = child.value.match(
            /<!--\s*relates-to:\s*"([^"]+)"\s*-->/
          );
          if (relatesMatch) {
            relatesTo = relatesMatch[1];
          }
        }
      }
    }

    if (!id) {
      // Generate a temporary ID if none exists
      id = this.generateId(title);
    }

    return {
      id,
      title: title.trim(),
      status: node.checked ? "done" : "todo", // Simplified status mapping
      relatesTo,
    };
  }
}
