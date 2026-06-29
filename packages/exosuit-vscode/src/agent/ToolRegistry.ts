import * as vscode from "vscode";
import * as fs from "node:fs";
import * as path from "node:path";
import { interpolateStrict, truncateWithNotice } from "@exosuit/core";
import { parse as parseToml } from "smol-toml";

export interface Tool {
  name: string;
  description: string;
  execute: (
    args: any,
    stream: vscode.ChatResponseStream,
  ) => Promise<string | undefined>;
}

export class ToolRegistry {
  private tools: Map<string, Tool> = new Map();
  private presentations: Map<string, string> = new Map();

  constructor(private rootPath?: string) {
    this.loadPresentations();
    this.registerBuiltInTools();
  }

  private loadPresentations() {
    if (!this.rootPath) {
      return;
    }
    const candidates = [
      path.join(this.rootPath, ".config", "exo", "tool-presentation.toml"),
      path.join(
        this.rootPath,
        "docs",
        "agent-context",
        "tool-presentation.toml",
      ),
    ];
    for (const candidate of candidates) {
      try {
        const content = fs.readFileSync(candidate, "utf8");
        const data = parseToml(content) as any;
        const tools = data?.tools;
        if (tools && typeof tools === "object") {
          for (const [name, presentation] of Object.entries(tools)) {
            if (presentation && typeof presentation === "object") {
              this.presentations.set(name, presentation as any);
            }
          }
        }
        return;
      } catch {
        // Try next candidate
      }
    }
  }

  private formatProgress(
    toolName: string,
    args: any,
    defaultMsg: string,
  ): string {
    const template = this.presentations.get(toolName);
    if (!template) {
      return defaultMsg;
    }
    // Follow the strict {key} interpolation standard and preserve missing tokens.
    return interpolateStrict(template, args ?? {});
  }

  register(tool: Tool) {
    this.tools.set(tool.name, tool);
  }

  get(name: string): Tool | undefined {
    // Alias resolution
    const aliases: Record<string, string> = {
      list_files: "listDirectory",
      read_file: "readFile",
      open_file: "openFile",
      edit_file: "editFile",
    };
    const resolvedName = aliases[name] || name;
    return this.tools.get(resolvedName);
  }

  getToolDefinitions(): string {
    return Array.from(this.tools.values())
      .map((tool) => `- **${tool.name}**: ${tool.description}`)
      .join("\n");
  }

  private registerBuiltInTools() {
    this.register({
      name: "sys.help",
      description: "Returns documentation for available tools. Args: {}",
      execute: async (args: any, stream) => {
        stream.progress(
          this.formatProgress(
            "sys.help",
            args,
            "Fetching tool documentation...",
          ),
        );
        const docs = this.getToolDefinitions();
        return docs;
      },
    });

    this.register({
      name: "readFile",
      description: "Read the contents of a file. Args: { path: string }",
      execute: async (args: { path: string }, stream) => {
        const filePath = args.path;
        stream.progress(
          this.formatProgress("readFile", args, `Reading file: ${filePath}`),
        );

        try {
          let targetPath = filePath;
          if (!path.isAbsolute(filePath) && vscode.workspace.workspaceFolders) {
            targetPath = path.join(
              vscode.workspace.workspaceFolders[0].uri.fsPath,
              filePath,
            );
          }

          const content = await fs.promises.readFile(targetPath, "utf-8");

          // Update progress and add a reference, but do NOT print the full content to the chat.
          // This prevents "spitting out" large files into the UI.
          stream.progress(`Read ${path.basename(targetPath)}`);
          stream.push(
            new vscode.ChatResponseReferencePart(vscode.Uri.file(targetPath)),
          );

          const MAX_FILE_CHARS = 200_000;
          const maybeTruncated = truncateWithNotice(content, MAX_FILE_CHARS, {
            notice: ({ originalLength, maxChars }) =>
              `\n\n[TRUNCATED readFile output: ${originalLength} → ${maxChars} chars]`,
          }).text;

          return maybeTruncated;
        } catch (e) {
          const errorMsg = `Error reading file: ${e}`;
          stream.markdown(`\n> ${errorMsg}\n`);
          return errorMsg;
        }
      },
    });

    this.register({
      name: "openFile",
      description:
        "Open a file in the editor for the user to see. Args: { path: string }",
      execute: async (args: { path: string }, stream) => {
        const filePath = args.path;
        stream.progress(
          this.formatProgress("openFile", args, `Opening file: ${filePath}`),
        );

        try {
          let targetPath = filePath;
          if (!path.isAbsolute(filePath) && vscode.workspace.workspaceFolders) {
            targetPath = path.join(
              vscode.workspace.workspaceFolders[0].uri.fsPath,
              filePath,
            );
          }

          const uri = vscode.Uri.file(targetPath);
          await vscode.window.showTextDocument(uri);
          return `Opened ${filePath}`;
        } catch (e) {
          const errorMsg = `Error opening file: ${e}`;
          stream.markdown(`\n> ${errorMsg}\n`);
          return errorMsg;
        }
      },
    });

    this.register({
      name: "editFile",
      description:
        "Propose an edit to a file. Args: { path: string, diff: string }",
      execute: async (args: { path: string; diff: string }, stream) => {
        const filePath = args.path;
        stream.progress(
          this.formatProgress(
            "editFile",
            args,
            `Proposing edit for: ${filePath}`,
          ),
        );

        // In a real implementation, we would apply the diff and show a preview.
        // For now, we render the diff in the chat and return the pending status.
        stream.markdown(`\n**Proposed Edit for ${filePath}**\n`);
        stream.markdown(`\`\`\`diff\n${args.diff}\n\`\`\`\n`);

        return JSON.stringify({
          status: "proposed",
          message: "Waiting for user acceptance...",
        });
      },
    });

    this.register({
      name: "listDirectory",
      description:
        "List files in a directory. Args: { path: string, recursive?: boolean }",
      execute: async (args: { path: string; recursive?: boolean }, stream) => {
        const dirPath = args.path;
        const recursive = args.recursive ?? false;
        stream.progress(
          this.formatProgress(
            "listDirectory",
            args,
            `Listing directory: ${dirPath}`,
          ),
        );

        try {
          // Resolve path relative to workspace root if needed
          let targetPath = dirPath;
          if (!path.isAbsolute(dirPath) && vscode.workspace.workspaceFolders) {
            targetPath = path.join(
              vscode.workspace.workspaceFolders[0].uri.fsPath,
              dirPath,
            );
          }

          if (recursive) {
            // Use findFiles to respect .gitignore and exclusions
            // RelativePattern(base, pattern) restricts search to that base
            const pattern = new vscode.RelativePattern(targetPath, "**");
            const files = await vscode.workspace.findFiles(pattern);

            // Build Tree Structure
            const rootChildren = new Map<string, any>();

            for (const file of files) {
              const relPath = path.relative(targetPath, file.fsPath);
              if (relPath.startsWith("..") || relPath === "") {
                continue;
              }

              const parts = relPath.split(path.sep);
              let currentLevel = rootChildren;

              for (let i = 0; i < parts.length; i++) {
                const part = parts[i];
                const isFile = i === parts.length - 1;

                if (!currentLevel.has(part)) {
                  currentLevel.set(part, {
                    name: part,
                    type: isFile
                      ? vscode.FileType.File
                      : vscode.FileType.Directory,
                    children: new Map<string, any>(),
                  });
                }

                if (!isFile) {
                  currentLevel = currentLevel.get(part).children;
                }
              }
            }

            const convertToTreeItem = (map: Map<string, any>): any[] => {
              return Array.from(map.values())
                .map((node) => ({
                  name: node.name,
                  type: node.type,
                  children:
                    node.type === vscode.FileType.Directory
                      ? convertToTreeItem(node.children)
                      : undefined,
                }))
                .sort((a, b) => {
                  // Sort directories first
                  if (a.type !== b.type) {
                    return a.type === vscode.FileType.Directory ? -1 : 1;
                  }
                  return a.name.localeCompare(b.name);
                });
            };

            const treeItems = convertToTreeItem(rootChildren);
            stream.push(
              new vscode.ChatResponseFileTreePart(
                treeItems,
                vscode.Uri.file(targetPath),
              ),
            );
            return JSON.stringify(treeItems, null, 2);
          } else {
            // Shallow list using fs
            const entries = await fs.promises.readdir(targetPath, {
              withFileTypes: true,
            });
            const treeItems = entries
              .map((entry) => ({
                name: entry.name,
                type: entry.isDirectory()
                  ? vscode.FileType.Directory
                  : vscode.FileType.File,
              }))
              .sort((a, b) => {
                if (a.type !== b.type) {
                  return a.type === vscode.FileType.Directory ? -1 : 1;
                }
                return a.name.localeCompare(b.name);
              });

            stream.push(
              new vscode.ChatResponseFileTreePart(
                treeItems,
                vscode.Uri.file(targetPath),
              ),
            );
            return JSON.stringify(treeItems, null, 2);
          }
        } catch (e) {
          const errorMsg = `Error listing directory: ${e}`;
          stream.markdown(`\n> ${errorMsg}\n`);
          return errorMsg;
        }
      },
    });
  }
}
