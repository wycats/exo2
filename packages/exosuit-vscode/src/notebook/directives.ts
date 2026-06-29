import * as vscode from "vscode";
import * as fs from "fs/promises";
import * as path from "path";
import { exec } from "child_process";
import { promisify } from "util";
import { getLogger } from "../logging";

const logger = getLogger("extension");

const execAsync = promisify(exec);

export async function executeDirective(text: string): Promise<string> {
  logger.trace(`Exosuit Directive: Executing '${text}'`);
  const trimmed = text.trim();

  if (trimmed.startsWith("@file:")) {
    return await handleFileDirective(trimmed);
  } else if (trimmed.startsWith("@run:")) {
    return await handleRunDirective(trimmed);
  }

  return `Unknown directive: ${trimmed.split(" ")[0]}`;
}

async function handleFileDirective(text: string): Promise<string> {
  // Format: @file: "path/to/file" or @file: path/to/file
  let relativePath = "";
  const quotedMatch = text.match(/@file:\s*["'](.+)["']/);
  if (quotedMatch) {
    relativePath = quotedMatch[1];
  } else {
    const unquotedMatch = text.match(/@file:\s*(.+)/);
    if (unquotedMatch) {
      relativePath = unquotedMatch[1].trim();
    }
  }

  if (!relativePath) {
    throw new Error('Invalid @file syntax. Expected: @file: "path/to/file"');
  }

  const workspaceFolders = vscode.workspace.workspaceFolders;
  logger.trace(
    `Exosuit Directive: Workspace Folders: ${JSON.stringify(workspaceFolders)}`,
  );

  if (!workspaceFolders) {
    throw new Error("No workspace open");
  }

  const rootPath = workspaceFolders[0].uri.fsPath;
  const fullPath = path.join(rootPath, relativePath);
  logger.trace(`Exosuit Directive: Reading file ${fullPath}`);

  try {
    const content = await fs.readFile(fullPath, "utf-8");
    return content;
  } catch (err: any) {
    logger.error(`Exosuit Directive: Read Failed: ${err.message}`);
    throw new Error(`Failed to read file ${relativePath}: ${err.message}`);
  }
}

async function handleRunDirective(text: string): Promise<string> {
  // Format: @run: "command"
  const match = text.match(/@run:\s*["'](.+)["']/);
  if (!match) {
    throw new Error('Invalid @run syntax. Expected: @run: "command"');
  }

  const command = match[1];
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders) {
    throw new Error("No workspace open");
  }

  const cwd = workspaceFolders[0].uri.fsPath;

  try {
    const { stdout, stderr } = await execAsync(command, { cwd });
    if (stderr) {
      return `STDOUT:\n${stdout}\n\nSTDERR:\n${stderr}`;
    }
    return stdout;
  } catch (err: any) {
    throw new Error(`Command failed: ${err.message}`);
  }
}
