#!/usr/bin/env node
/**
 * Sync package.json languageModelTools with the curated Exosuit tool surface.
 *
 * CommandSpec still generates package-tools metadata for auditing, but generated
 * CommandSpec tools are not automatically contributed as VS Code LM tools. The
 * active LM tool surface is intentionally small and curated.
 *
 * 1. **Audit Mode** (default): Show curated vs declared plus generated inventory
 * 2. **Add Mode** (--add): Overwrite package.json with curated tools only
 * 3. **Check Mode** (--check): CI validation - ensure package.json is curated
 *
 * Usage:
 *   node scripts/sync-lm-tools.ts          # Show audit
 *   node scripts/sync-lm-tools.ts --add    # Write curated tools to package.json
 *   node scripts/sync-lm-tools.ts --check  # CI mode
 */

import { execSync } from "child_process";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT_DIR = path.resolve(__dirname, "..");
const PACKAGE_JSON_PATH = path.join(
  ROOT_DIR,
  "packages/exosuit-vscode/package.json",
);

interface LmToolContribution {
  name: string;
  displayName: string;
  toolReferenceName: string;
  canBeReferencedInPrompt: boolean;
  icon: string;
  tags: string[];
  userDescription: string;
  modelDescription: string;
  inputSchema: Record<string, unknown>;
  when?: string;
}

interface LmToolSetContribution {
  name: string;
  displayName: string;
  description: string;
  tools: string[];
}

interface PackageToolsPayload {
  tools: LmToolContribution[];
  toolSets: LmToolSetContribution[];
}

interface GeneratedOutput {
  result?: LmToolContribution[] | PackageToolsPayload;
}

const CURATED_TOOL_NAMES = [
  "exo-ai-chat-history",
  "exo-diagnostics",
  "exo-logs",
  "exo-ping",
  "exo-run",
];

const CURATED_TOOL_NAME_SET = new Set(CURATED_TOOL_NAMES);

function getGeneratedTools(): PackageToolsPayload {
  const output = execSync(
    "cargo run -p exo --quiet --bin exo -- json package-tools --format json",
    {
      cwd: ROOT_DIR,
      encoding: "utf-8",
    },
  );

  // Skip the CLI envelope lines and parse the JSON
  const lines = output.split("\n");
  const jsonStart = lines.findIndex((l) => l.trim().startsWith("{"));
  const jsonContent = lines.slice(jsonStart).join("\n");

  const parsed: GeneratedOutput = JSON.parse(jsonContent);
  const result = parsed.result ?? parsed;

  if (Array.isArray(result)) {
    return { tools: result, toolSets: [] };
  }

  return {
    tools: result.tools ?? [],
    toolSets: result.toolSets ?? [],
  };
}

function getPackageJsonTools(): LmToolContribution[] {
  const packageJson = JSON.parse(fs.readFileSync(PACKAGE_JSON_PATH, "utf-8"));
  return packageJson.contributes?.languageModelTools || [];
}

function getPackageJsonToolSets(): LmToolSetContribution[] {
  const packageJson = JSON.parse(fs.readFileSync(PACKAGE_JSON_PATH, "utf-8"));
  return packageJson.contributes?.languageModelToolSets || [];
}

function curatedToolsFromExisting(existing: LmToolContribution[]): {
  tools: LmToolContribution[];
  missing: string[];
} {
  const existingByName = new Map(existing.map((tool) => [tool.name, tool]));
  const missing = CURATED_TOOL_NAMES.filter(
    (name) => !existingByName.has(name),
  );
  const tools = CURATED_TOOL_NAMES.map((name) =>
    existingByName.get(name),
  ).filter((tool): tool is LmToolContribution => tool !== undefined);

  return { tools, missing };
}

function diff(
  expectedNames: string[],
  existing: LmToolContribution[],
): { added: string[]; removed: string[]; common: string[] } {
  const genNames = new Set(expectedNames);
  const existNames = new Set(existing.map((t) => t.name));

  const added = [...genNames].filter((n) => !existNames.has(n));
  const removed = [...existNames].filter((n) => !genNames.has(n));
  const common = [...genNames].filter((n) => existNames.has(n));

  return { added, removed, common };
}

function diffToolSets(
  generated: LmToolSetContribution[],
  existing: LmToolSetContribution[],
): { added: string[]; removed: string[]; common: string[] } {
  const genNames = new Set(generated.map((t) => t.name));
  const existNames = new Set(existing.map((t) => t.name));

  const added = [...genNames].filter((n) => !existNames.has(n));
  const removed = [...existNames].filter((n) => !genNames.has(n));
  const common = [...genNames].filter((n) => existNames.has(n));

  return { added, removed, common };
}

function writePackageJson(payload: PackageToolsPayload): void {
  const packageJson = JSON.parse(fs.readFileSync(PACKAGE_JSON_PATH, "utf-8"));
  packageJson.contributes.languageModelTools = payload.tools;
  delete packageJson.contributes.languageModelToolSets;

  fs.writeFileSync(
    PACKAGE_JSON_PATH,
    JSON.stringify(packageJson, null, 2) + "\n",
  );
}

function main(): void {
  const args = process.argv.slice(2);
  const addMode = args.includes("--add");
  const checkMode = args.includes("--check");

  console.log("Checking curated languageModelTools surface...\n");

  const generated = getGeneratedTools();
  const existing = getPackageJsonTools();
  const existingToolSets = getPackageJsonToolSets();
  const generatedNames = new Set(generated.tools.map((tool) => tool.name));

  console.log(`Curated:  ${CURATED_TOOL_NAMES.length} intended LM tools`);
  console.log(`Existing: ${existing.length} tools in package.json`);
  console.log(
    `Generated: ${generated.tools.length} CommandSpec tools (informational only)`,
  );
  console.log(
    `Generated: ${generated.toolSets.length} CommandSpec toolsets (informational only)`,
  );
  console.log(
    `Existing:  ${existingToolSets.length} toolsets in package.json\n`,
  );

  const { added, removed, common } = diff(CURATED_TOOL_NAMES, existing);
  const toolSetDiff = diffToolSets(generated.toolSets, existingToolSets);
  const declaredGeneratedTools = existing
    .map((tool) => tool.name)
    .filter((name) => generatedNames.has(name))
    .filter((name) => !CURATED_TOOL_NAME_SET.has(name));

  if (added.length > 0) {
    console.log("➕ Missing curated tools from package.json:");
    added.forEach((n) => console.log(`   ${n}`));
    console.log();
  }

  if (removed.length > 0) {
    console.log("⚠️  Tools in package.json outside the curated surface:");
    removed.forEach((n) => console.log(`   ${n}`));
    console.log("   (These will be removed by --add)\n");
  }

  console.log(`✓ ${common.length} curated tools declared\n`);

  if (declaredGeneratedTools.length > 0) {
    console.log("⚠️  Generated CommandSpec tools currently declared:");
    declaredGeneratedTools.forEach((n) => console.log(`   ${n}`));
    console.log(
      "   (CommandSpec tool inventory is not the VS Code LM surface)\n",
    );
  }

  if (toolSetDiff.added.length > 0) {
    console.log("📌 CommandSpec toolsets not declared in package.json:");
    toolSetDiff.added.forEach((n) => console.log(`   ${n}`));
    console.log(
      "   (This is expected; toolsets require proposed VS Code API)\n",
    );
    console.log();
  }

  if (toolSetDiff.removed.length > 0) {
    console.log("📌 Toolsets in package.json but not in generated output:");
    toolSetDiff.removed.forEach((n) => console.log(`   ${n}`));
    console.log();
  }

  console.log(`✓ ${toolSetDiff.common.length} CommandSpec toolsets declared\n`);

  if (checkMode) {
    const existingNames = existing.map((tool) => tool.name);
    const match =
      JSON.stringify(existingNames) === JSON.stringify(CURATED_TOOL_NAMES);

    if (!match) {
      console.log("ERROR: package.json tools do not match curated surface!");
      console.log("Run: node scripts/sync-lm-tools.ts --add");
      process.exit(1);
    }

    if (existingToolSets.length > 0) {
      console.log(
        "ERROR: package.json declares languageModelToolSets, which require the contribLanguageModelToolSets proposed API.",
      );
      console.log("Run: node scripts/sync-lm-tools.ts --add");
      process.exit(1);
    }

    console.log("✓ Sync check passed");
    process.exit(0);
  }

  if (addMode) {
    const { tools, missing } = curatedToolsFromExisting(existing);
    if (missing.length > 0) {
      console.log(
        `ERROR: Cannot write curated surface; missing tool definitions: ${missing.join(
          ", ",
        )}`,
      );
      process.exit(1);
    }

    writePackageJson({ tools, toolSets: [] });
    console.log(
      `✓ Wrote ${tools.length} curated tools and omitted ${generated.tools.length} generated CommandSpec tools plus ${generated.toolSets.length} proposed toolsets from package.json`,
    );
  } else {
    console.log(
      "Run with --add to rewrite package.json to the curated surface",
    );
    console.log("Run with --check for CI validation");
  }
}

main();
