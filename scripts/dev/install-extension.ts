#!/usr/bin/env node
/**
 * install-extension.ts — Package and install the Exosuit VS Code extension.
 *
 * Handles:
 *   1. Building the minified extension bundle + VSIX packaging
 *   2. Removing conflicting extension variants ("ghost" IDs)
 *   3. Installing via the `code` CLI
 *   4. Verifying the install succeeded (version + freshness check)
 *
 * The `code` CLI is the only supported install path. Direct-copy installs
 * bypass VS Code's extension management (version tracking, .obsolete cleanup,
 * activation ordering) and have historically caused stale-extension bugs.
 *
 * Environment variables:
 *   VSCODE_BINARY           — Override the `code` binary path
 *   EXOSUIT_SKIP_VERIFY     — Set to "1" to skip post-install verification
 *   EXOSUIT_KEEP_VSIX       — Set to "1" to keep the packaged VSIX after success
 */

import { execSync, spawn, spawnSync } from "child_process";
import type {
  ChildProcess,
  SpawnSyncReturns,
  StdioOptions,
} from "child_process";
import { createHash } from "crypto";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { fileURLToPath } from "url";
import { inflateRawSync } from "zlib";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT_DIR = path.resolve(__dirname, "../..");
const EXTENSION_DIR = path.join(ROOT_DIR, "packages/exosuit-vscode");

const CANONICAL_PUBLISHER = "exosuit";
const CANONICAL_NAME = "exosuit-context";
const CANONICAL_ID = `${CANONICAL_PUBLISHER}.${CANONICAL_NAME}`;
const BUNDLE_MANIFEST_SCHEMA = 2;
const BUNDLE_MANIFEST_KIND = "exosuit-vscode-extension-bundle";
const BUNDLE_RELATIVE_PATH = "out/extension.js";
const BUNDLE_MANIFEST_RELATIVE_PATH = "out/dev-host-bundle.json";
const PACKAGE_MANIFEST_RELATIVE_PATH = "package.json";
const VSIX_EXTENSION_PREFIX = "extension/";
const BUILD_STAMP_RE = /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z/g;
const IDENTITY_FIELDS = [
  "buildStamp",
  "bundleSha256",
  "bundleSizeBytes",
] as const;

export type LauncherMode = "direct" | "shell";

export interface ExtensionBundleIdentity {
  readonly extensionId: string;
  readonly packageVersion: string;
  readonly daemonRuntimePaths: string;
  readonly bundle: string;
  readonly buildStamp: string;
  readonly bundleSha256: string;
  readonly bundleSizeBytes: number;
}

export type ExtensionBundleContentIdentity = Pick<
  ExtensionBundleIdentity,
  (typeof IDENTITY_FIELDS)[number]
>;

export interface ExtensionBundleManifest extends ExtensionBundleIdentity {
  readonly schema: number;
  readonly kind: string;
  readonly bundleMtimeMs: number;
  readonly generatedAt: string;
}

export interface ExtensionBundleArtifacts {
  readonly source: string;
  readonly manifest: ExtensionBundleManifest;
  readonly identity: ExtensionBundleIdentity;
  readonly bundleMtimeMs: number | null;
}

export interface CommandResult {
  readonly mode: LauncherMode;
  readonly status: number | null;
  readonly signal: NodeJS.Signals | null;
  readonly stdout: string;
  readonly stderr: string;
  readonly error?: NodeJS.ErrnoException;
}

export interface CommandRunner {
  runSync(
    command: string,
    args: readonly string[],
    options: CommandRunnerOptions,
  ): Omit<CommandResult, "mode">;
  run(
    command: string,
    args: readonly string[],
    options: CommandRunnerOptions,
  ): Promise<Omit<CommandResult, "mode">>;
}

export interface CommandRunnerOptions {
  readonly cwd?: string;
  readonly shell: boolean;
  readonly stdio: StdioOptions;
}

interface LauncherRunOptions {
  readonly cwd?: string;
  readonly label: string;
  readonly logLauncher?: boolean;
  readonly stdio?: StdioOptions;
}

function log(msg: string): void {
  console.log(msg);
}

interface RunOptions {
  readonly cwd?: string;
  readonly env?: NodeJS.ProcessEnv;
}

function run(cmd: string, options: RunOptions = {}): void {
  execSync(cmd, {
    cwd: options.cwd ?? EXTENSION_DIR,
    stdio: "inherit",
    env: options.env ?? process.env,
  });
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function mtimeMs(filePath: string): number {
  try {
    return fs.statSync(filePath).mtimeMs;
  } catch {
    return 0;
  }
}

function fileExists(filePath: string): boolean {
  return fs.existsSync(filePath);
}

function readJsonFile(filePath: string): unknown {
  return JSON.parse(fs.readFileSync(filePath, "utf-8"));
}

export function readPackageVersion(packageJsonPath: string): string {
  const packageJson = readJsonFile(packageJsonPath);
  const object = assertObject(packageJson, `${packageJsonPath} package.json`);
  return readRequiredString(
    object,
    "version",
    `${packageJsonPath} package.json`,
  );
}

function sha256(bytes: Buffer): string {
  return createHash("sha256").update(bytes).digest("hex");
}

function shortHash(hash: string): string {
  return hash.slice(0, 12);
}

function isCliEntrypoint(): boolean {
  const entrypoint = process.argv[1];
  return entrypoint ? path.resolve(entrypoint) === __filename : false;
}

function stringifyOutput(output: string | Buffer | null | undefined): string {
  if (typeof output === "string") {
    return output;
  }
  if (Buffer.isBuffer(output)) {
    return output.toString("utf-8");
  }
  return "";
}

function toStdio(stdio: StdioOptions | undefined): StdioOptions {
  return stdio ?? "pipe";
}

export function isEnoent(error: NodeJS.ErrnoException | undefined): boolean {
  return error?.code === "ENOENT";
}

export function commandSucceeded(
  result: Pick<CommandResult, "error" | "signal" | "status">,
): boolean {
  return !result.error && result.signal === null && result.status === 0;
}

function shellQuote(value: string): string {
  if (process.platform === "win32") {
    return `"${value.replace(/(["^&|<>])/g, "^$1")}"`;
  }
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

function shellCommand(command: string, args: readonly string[]): string {
  return [command, ...args].map(shellQuote).join(" ");
}

function commandForMode(
  mode: LauncherMode,
  command: string,
  args: readonly string[],
): { command: string; args: readonly string[] } {
  if (mode === "shell") {
    return { command: shellCommand(command, args), args: [] };
  }
  return { command, args };
}

function normalizeSpawnSyncResult(
  result: SpawnSyncReturns<string | Buffer>,
): Omit<CommandResult, "mode"> {
  return {
    status: result.status,
    signal: result.signal,
    stdout: stringifyOutput(result.stdout),
    stderr: stringifyOutput(result.stderr),
    error: result.error as NodeJS.ErrnoException | undefined,
  };
}

export const defaultCommandRunner: CommandRunner = {
  runSync(
    command: string,
    args: readonly string[],
    options: CommandRunnerOptions,
  ): Omit<CommandResult, "mode"> {
    return normalizeSpawnSyncResult(
      spawnSync(command, [...args], {
        cwd: options.cwd,
        encoding: "utf-8",
        shell: options.shell,
        stdio: options.stdio,
      }),
    );
  },

  run(
    command: string,
    args: readonly string[],
    options: CommandRunnerOptions,
  ): Promise<Omit<CommandResult, "mode">> {
    return new Promise((resolve) => {
      let settled = false;
      let stdout = "";
      let stderr = "";
      const child: ChildProcess = spawn(command, [...args], {
        cwd: options.cwd,
        shell: options.shell,
        stdio: options.stdio,
      });

      child.stdout?.on("data", (chunk: Buffer | string) => {
        stdout += stringifyOutput(chunk);
      });
      child.stderr?.on("data", (chunk: Buffer | string) => {
        stderr += stringifyOutput(chunk);
      });

      child.on("error", (error: NodeJS.ErrnoException) => {
        if (settled) {
          return;
        }
        settled = true;
        resolve({ status: null, signal: null, stdout, stderr, error });
      });

      child.on("exit", (status, signal) => {
        if (settled) {
          return;
        }
        settled = true;
        resolve({ status, signal, stdout, stderr });
      });
    });
  },
};

export class CodeCliLauncher {
  readonly binary: string;

  #mode: LauncherMode | undefined;
  #runner: CommandRunner;
  #logger: (msg: string) => void;

  constructor(
    binary: string,
    runner: CommandRunner = defaultCommandRunner,
    logger: (msg: string) => void = log,
  ) {
    this.binary = binary;
    this.#runner = runner;
    this.#logger = logger;
  }

  runSync(args: readonly string[], options: LauncherRunOptions): CommandResult {
    if (this.#mode === "shell") {
      return this.#runSyncWithMode("shell", args, options);
    }

    const direct = this.#runSyncWithMode("direct", args, options);
    if (isEnoent(direct.error)) {
      this.#logger(
        `  ⚠️  ${options.label}: direct launcher hit ENOENT; retrying with shell`,
      );
      const shell = this.#runSyncWithMode("shell", args, options);
      if (commandSucceeded(shell)) {
        this.#mode = "shell";
      }
      return shell;
    }

    if (commandSucceeded(direct)) {
      this.#mode = "direct";
    }
    return direct;
  }

  async run(
    args: readonly string[],
    options: LauncherRunOptions,
  ): Promise<CommandResult> {
    if (this.#mode === "shell") {
      return this.#runWithMode("shell", args, options);
    }

    const direct = await this.#runWithMode("direct", args, options);
    if (isEnoent(direct.error)) {
      this.#logger(
        `  ⚠️  ${options.label}: direct launcher hit ENOENT; retrying with shell`,
      );
      const shell = await this.#runWithMode("shell", args, options);
      if (commandSucceeded(shell)) {
        this.#mode = "shell";
      }
      return shell;
    }

    if (commandSucceeded(direct)) {
      this.#mode = "direct";
    }
    return direct;
  }

  #runSyncWithMode(
    mode: LauncherMode,
    args: readonly string[],
    options: LauncherRunOptions,
  ): CommandResult {
    this.#logMode(mode, options);
    const command = commandForMode(mode, this.binary, args);
    return {
      mode,
      ...this.#runner.runSync(command.command, command.args, {
        cwd: options.cwd,
        shell: mode === "shell",
        stdio: toStdio(options.stdio),
      }),
    };
  }

  async #runWithMode(
    mode: LauncherMode,
    args: readonly string[],
    options: LauncherRunOptions,
  ): Promise<CommandResult> {
    this.#logMode(mode, options);
    const command = commandForMode(mode, this.binary, args);
    return {
      mode,
      ...(await this.#runner.run(command.command, command.args, {
        cwd: options.cwd,
        shell: mode === "shell",
        stdio: toStdio(options.stdio),
      })),
    };
  }

  #logMode(mode: LauncherMode, options: LauncherRunOptions): void {
    if (options.logLauncher === false) {
      return;
    }
    this.#logger(`  → ${options.label}: launcher=${mode}`);
  }
}

function formatCommandFailure(result: CommandResult): string {
  if (result.error) {
    return `${result.error.code ?? "ERROR"}: ${result.error.message}`;
  }
  if (result.signal) {
    return `terminated by ${result.signal}`;
  }
  return `exit ${result.status ?? "unknown"}`;
}

function logCommandFailure(prefix: string, result: CommandResult): void {
  log(`  ❌ ${prefix}: ${formatCommandFailure(result)}`);
  const stderr = result.stderr.trim();
  if (stderr.length > 0) {
    log(`     stderr: ${stderr}`);
  }
}

export function expectedVsixName(version: string): string {
  return `${CANONICAL_NAME}-${version}.vsix`;
}

export function selectExpectedVsix(
  extensionDir: string,
  version: string,
): string {
  const expected = path.join(extensionDir, expectedVsixName(version));
  if (!fileExists(expected)) {
    throw new Error(
      `Expected VSIX was not generated: ${expected}. Refusing to install stale VSIX artifacts.`,
    );
  }
  return expected;
}

export function extensionListHasVersion(
  listOutput: string,
  extensionId: string,
  version: string,
): boolean {
  const expected = `${extensionId.toLowerCase()}@${version}`;
  return listOutput
    .split(/\r?\n/)
    .map((line) => line.trim().toLowerCase())
    .some((line) => line === expected);
}

function isInsidersCodeBinary(codeBinary: string | undefined): boolean {
  if (!codeBinary) {
    return false;
  }
  const normalized = codeBinary.toLowerCase().replace(/\\/g, "/");
  return (
    normalized.includes("code-insiders") ||
    normalized.includes("code - insiders")
  );
}

export function defaultVscodeExtensionsDir(codeBinary?: string): string {
  const profileDir = isInsidersCodeBinary(codeBinary)
    ? ".vscode-insiders"
    : ".vscode";
  return path.join(os.homedir(), profileDir, "extensions");
}

export function installedDir(version: string, codeBinary?: string): string {
  return path.join(
    defaultVscodeExtensionsDir(codeBinary),
    `${CANONICAL_ID}-${version}`,
  );
}

function installedMarkerFile(version: string, codeBinary?: string): string {
  return path.join(installedDir(version, codeBinary), BUNDLE_RELATIVE_PATH);
}

function assertObject(
  value: unknown,
  description: string,
): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${description} must be a JSON object`);
  }
  return value as Record<string, unknown>;
}

function readRequiredString(
  object: Record<string, unknown>,
  field: string,
  description: string,
): string {
  const value = object[field];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${description}.${field} must be a non-empty string`);
  }
  return value;
}

function readRequiredNumber(
  object: Record<string, unknown>,
  field: string,
  description: string,
): number {
  const value = object[field];
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`${description}.${field} must be a finite number`);
  }
  return value;
}

export function parseExtensionBundleManifest(
  value: unknown,
  source: string,
): ExtensionBundleManifest {
  const object = assertObject(value, `${source} manifest`);
  const schema = readRequiredNumber(object, "schema", `${source} manifest`);
  const kind = readRequiredString(object, "kind", `${source} manifest`);
  if (schema !== BUNDLE_MANIFEST_SCHEMA) {
    throw new Error(
      `${source} manifest schema mismatch: expected ${BUNDLE_MANIFEST_SCHEMA}, got ${schema}`,
    );
  }
  if (kind !== BUNDLE_MANIFEST_KIND) {
    throw new Error(
      `${source} manifest kind mismatch: expected ${BUNDLE_MANIFEST_KIND}, got ${kind}`,
    );
  }

  return {
    schema,
    kind,
    extensionId: readRequiredString(
      object,
      "extensionId",
      `${source} manifest`,
    ),
    packageVersion: readRequiredString(
      object,
      "packageVersion",
      `${source} manifest`,
    ),
    daemonRuntimePaths: readRequiredString(
      object,
      "daemonRuntimePaths",
      `${source} manifest`,
    ),
    bundle: readRequiredString(object, "bundle", `${source} manifest`),
    buildStamp: readRequiredString(object, "buildStamp", `${source} manifest`),
    bundleSha256: readRequiredString(
      object,
      "bundleSha256",
      `${source} manifest`,
    ),
    bundleSizeBytes: readRequiredNumber(
      object,
      "bundleSizeBytes",
      `${source} manifest`,
    ),
    bundleMtimeMs: readRequiredNumber(
      object,
      "bundleMtimeMs",
      `${source} manifest`,
    ),
    generatedAt: readRequiredString(
      object,
      "generatedAt",
      `${source} manifest`,
    ),
  };
}

function identityFromManifest(
  manifest: ExtensionBundleManifest,
): ExtensionBundleIdentity {
  return {
    extensionId: manifest.extensionId,
    packageVersion: manifest.packageVersion,
    daemonRuntimePaths: manifest.daemonRuntimePaths,
    bundle: manifest.bundle,
    buildStamp: manifest.buildStamp,
    bundleSha256: manifest.bundleSha256,
    bundleSizeBytes: manifest.bundleSizeBytes,
  };
}

function validateIdentityBasics(
  identity: ExtensionBundleIdentity,
  source: string,
): void {
  if (identity.extensionId !== CANONICAL_ID) {
    throw new Error(
      `${source} extensionId mismatch: expected ${CANONICAL_ID}, got ${identity.extensionId}`,
    );
  }
  if (identity.bundle !== BUNDLE_RELATIVE_PATH) {
    throw new Error(
      `${source} bundle path mismatch: expected ${BUNDLE_RELATIVE_PATH}, got ${identity.bundle}`,
    );
  }
  if (identity.daemonRuntimePaths !== "project-resolve") {
    throw new Error(
      `${source} daemon runtime path mode mismatch: expected project-resolve, got ${identity.daemonRuntimePaths}`,
    );
  }
}

function extractBuildStamp(bundleSource: string, source: string): string {
  const buildStampLineMatch = bundleSource.match(
    /Build stamp:[^0-9]{0,120}(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z)/,
  );
  if (buildStampLineMatch) {
    return buildStampLineMatch[1];
  }

  const matches = bundleSource.match(BUILD_STAMP_RE) ?? [];
  if (matches.length === 0) {
    throw new Error(`${source} bundle does not contain a build stamp`);
  }

  const counts = new Map<string, number>();
  for (const stamp of matches) {
    counts.set(stamp, (counts.get(stamp) ?? 0) + 1);
  }

  const ranked = [...counts.entries()].sort((a, b) => b[1] - a[1]);
  const first = ranked[0];
  const second = ranked[1];
  if (first && (!second || first[1] > second[1])) {
    return first[0];
  }

  throw new Error(
    `${source} bundle does not contain a unique build stamp: ${[
      ...counts.keys(),
    ].join(", ")}`,
  );
}

function identityFromBundleBytes(
  bundleBytes: Buffer,
  manifest: ExtensionBundleManifest,
  source: string,
): ExtensionBundleIdentity {
  const bundleSource = bundleBytes.toString("utf-8");
  return {
    extensionId: manifest.extensionId,
    packageVersion: manifest.packageVersion,
    daemonRuntimePaths: manifest.daemonRuntimePaths,
    bundle: manifest.bundle,
    buildStamp: extractBuildStamp(bundleSource, source),
    bundleSha256: sha256(bundleBytes),
    bundleSizeBytes: bundleBytes.byteLength,
  };
}

function formatIdentityValue(
  identity: ExtensionBundleIdentity,
  field: (typeof IDENTITY_FIELDS)[number],
): string {
  const value = identity[field];
  if (field === "bundleSha256") {
    return shortHash(String(value));
  }
  return String(value);
}

export function compareBundleIdentities(
  expected: ExtensionBundleContentIdentity,
  actual: ExtensionBundleContentIdentity,
  options: { readonly expectedSource: string; readonly actualSource: string },
): void {
  const mismatches = IDENTITY_FIELDS.filter(
    (field) => expected[field] !== actual[field],
  );
  if (mismatches.length === 0) {
    return;
  }

  const details = mismatches
    .map(
      (field) =>
        `${field}: ${options.expectedSource}=${formatIdentityValue(
          expected,
          field,
        )}, ${options.actualSource}=${formatIdentityValue(actual, field)}`,
    )
    .join("; ");
  throw new Error(`Bundle identity mismatch: ${details}`);
}

function verifyBundleMatchesManifest(
  bundleBytes: Buffer,
  manifest: ExtensionBundleManifest,
  source: string,
): ExtensionBundleIdentity {
  validateIdentityBasics(manifest, source);
  const actual = identityFromBundleBytes(bundleBytes, manifest, source);
  compareBundleIdentities(identityFromManifest(manifest), actual, {
    expectedSource: `${source} manifest`,
    actualSource: `${source} bundle`,
  });
  return actual;
}

export function readWorkspaceBundleArtifacts(
  extensionDir: string = EXTENSION_DIR,
): ExtensionBundleArtifacts {
  const bundlePath = path.join(extensionDir, BUNDLE_RELATIVE_PATH);
  const manifestPath = path.join(extensionDir, BUNDLE_MANIFEST_RELATIVE_PATH);
  const manifest = parseExtensionBundleManifest(
    readJsonFile(manifestPath),
    "workspace",
  );
  const bundleBytes = fs.readFileSync(bundlePath);
  const identity = verifyBundleMatchesManifest(
    bundleBytes,
    manifest,
    "workspace",
  );

  return {
    source: "workspace",
    manifest,
    identity,
    bundleMtimeMs: fs.statSync(bundlePath).mtimeMs,
  };
}

function findEndOfCentralDirectory(zip: Buffer): number {
  const minEocdSize = 22;
  const maxCommentSize = 0xffff;
  const searchStart = Math.max(0, zip.length - minEocdSize - maxCommentSize);
  for (let offset = zip.length - minEocdSize; offset >= searchStart; offset--) {
    if (zip.readUInt32LE(offset) === 0x06054b50) {
      return offset;
    }
  }
  throw new Error("Could not find ZIP end-of-central-directory record");
}

export function readZipEntry(zipPath: string, entryName: string): Buffer {
  const zip = fs.readFileSync(zipPath);
  const eocdOffset = findEndOfCentralDirectory(zip);
  const entryCount = zip.readUInt16LE(eocdOffset + 10);
  const centralDirectoryOffset = zip.readUInt32LE(eocdOffset + 16);
  let cursor = centralDirectoryOffset;

  for (let i = 0; i < entryCount; i++) {
    if (zip.readUInt32LE(cursor) !== 0x02014b50) {
      throw new Error(
        `Invalid ZIP central-directory header at offset ${cursor}`,
      );
    }

    const compressionMethod = zip.readUInt16LE(cursor + 10);
    const compressedSize = zip.readUInt32LE(cursor + 20);
    const uncompressedSize = zip.readUInt32LE(cursor + 24);
    const fileNameLength = zip.readUInt16LE(cursor + 28);
    const extraFieldLength = zip.readUInt16LE(cursor + 30);
    const fileCommentLength = zip.readUInt16LE(cursor + 32);
    const localHeaderOffset = zip.readUInt32LE(cursor + 42);
    const nameStart = cursor + 46;
    const nameEnd = nameStart + fileNameLength;
    const name = zip.toString("utf-8", nameStart, nameEnd);

    if (name === entryName) {
      if (zip.readUInt32LE(localHeaderOffset) !== 0x04034b50) {
        throw new Error(`Invalid ZIP local-file header for ${entryName}`);
      }
      const localFileNameLength = zip.readUInt16LE(localHeaderOffset + 26);
      const localExtraFieldLength = zip.readUInt16LE(localHeaderOffset + 28);
      const dataStart =
        localHeaderOffset + 30 + localFileNameLength + localExtraFieldLength;
      const dataEnd = dataStart + compressedSize;
      const compressed = zip.subarray(dataStart, dataEnd);

      if (compressionMethod === 0) {
        return Buffer.from(compressed);
      }
      if (compressionMethod === 8) {
        const inflated = inflateRawSync(compressed);
        if (inflated.byteLength !== uncompressedSize) {
          throw new Error(
            `Inflated ZIP entry ${entryName} size mismatch: expected ${uncompressedSize}, got ${inflated.byteLength}`,
          );
        }
        return inflated;
      }
      throw new Error(
        `Unsupported ZIP compression method ${compressionMethod} for ${entryName}`,
      );
    }

    cursor = nameEnd + extraFieldLength + fileCommentLength;
  }

  throw new Error(`ZIP entry not found in ${zipPath}: ${entryName}`);
}

export function readVsixBundleArtifacts(
  vsixPath: string,
): ExtensionBundleArtifacts {
  const manifestBytes = readZipEntry(
    vsixPath,
    `${VSIX_EXTENSION_PREFIX}${BUNDLE_MANIFEST_RELATIVE_PATH}`,
  );
  const bundleBytes = readZipEntry(
    vsixPath,
    `${VSIX_EXTENSION_PREFIX}${BUNDLE_RELATIVE_PATH}`,
  );
  const manifest = parseExtensionBundleManifest(
    JSON.parse(manifestBytes.toString("utf-8")),
    "VSIX",
  );
  const identity = verifyBundleMatchesManifest(bundleBytes, manifest, "VSIX");

  return {
    source: "VSIX",
    manifest,
    identity,
    bundleMtimeMs: null,
  };
}

export function readVsixPackageManifest(vsixPath: string): unknown {
  return JSON.parse(
    readZipEntry(
      vsixPath,
      `${VSIX_EXTENSION_PREFIX}${PACKAGE_MANIFEST_RELATIVE_PATH}`,
    ).toString("utf-8"),
  );
}

export function packageManifestHash(packageManifest: unknown): string {
  return sha256(
    Buffer.from(
      JSON.stringify(normalizePackageManifest(packageManifest)),
      "utf-8",
    ),
  );
}

function normalizePackageManifest(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(normalizePackageManifest);
  }
  if (value && typeof value === "object") {
    const object = value as Record<string, unknown>;
    return Object.fromEntries(
      Object.keys(object)
        .filter((key) => key !== "__metadata")
        .sort()
        .map((key) => [key, normalizePackageManifest(object[key])]),
    );
  }
  return value;
}

export function comparePackageManifests(
  expected: unknown,
  actual: unknown,
  options: { readonly expectedSource: string; readonly actualSource: string },
): void {
  const expectedHash = packageManifestHash(expected);
  const actualHash = packageManifestHash(actual);
  if (expectedHash === actualHash) {
    return;
  }

  throw new Error(
    `Package manifest mismatch: ${options.expectedSource}=${shortHash(
      expectedHash,
    )}, ${options.actualSource}=${shortHash(actualHash)}`,
  );
}

export function readInstalledBundleArtifacts(
  installedDirPath: string,
): ExtensionBundleArtifacts | null {
  const manifestPath = path.join(
    installedDirPath,
    BUNDLE_MANIFEST_RELATIVE_PATH,
  );
  const bundlePath = path.join(installedDirPath, BUNDLE_RELATIVE_PATH);
  if (!fileExists(manifestPath) || !fileExists(bundlePath)) {
    return null;
  }

  const manifest = parseExtensionBundleManifest(
    readJsonFile(manifestPath),
    "installed",
  );
  const bundleBytes = fs.readFileSync(bundlePath);
  const identity = verifyBundleMatchesManifest(
    bundleBytes,
    manifest,
    "installed",
  );

  return {
    source: "installed",
    manifest,
    identity,
    bundleMtimeMs: fs.statSync(bundlePath).mtimeMs,
  };
}

// ---------------------------------------------------------------------------
// Phase 1: Build and Package
// ---------------------------------------------------------------------------

function buildAndPackage(): string {
  log("📦 Packaging extension...");

  // Build the minified bundle.
  run("pnpm run bundle", { env: { ...process.env, MINIFY: "true" } });

  // Ensure webview build outputs exist.
  const dashboardCss = path.join(EXTENSION_DIR, "out/webview/dashboard.css");
  if (!fileExists(dashboardCss)) {
    log("🧱 Webview assets missing; building webview");
    run("pnpm run build:webview");
  }

  run("pnpm exec vsce package --no-yarn --no-dependencies");

  // Find the VSIX for the current version.
  const pkgJson = JSON.parse(
    fs.readFileSync(path.join(EXTENSION_DIR, "package.json"), "utf-8"),
  );
  const version: string = pkgJson.version;
  const vsixPath = selectExpectedVsix(EXTENSION_DIR, version);

  const size = (fs.statSync(vsixPath).size / 1024).toFixed(0);
  log(`  📦 VSIX: ${vsixPath} (${size}K)`);

  return vsixPath;
}

function formatBundleIdentity(identity: ExtensionBundleIdentity): string {
  return `${identity.extensionId}@${identity.packageVersion} stamp=${identity.buildStamp} sha=${shortHash(identity.bundleSha256)} size=${identity.bundleSizeBytes}`;
}

function logBundleIdentity(
  label: string,
  artifacts: ExtensionBundleArtifacts,
): void {
  log(`  ${label}: ${formatBundleIdentity(artifacts.identity)}`);
  if (
    artifacts.bundleMtimeMs !== null &&
    artifacts.manifest.bundleMtimeMs !== artifacts.bundleMtimeMs
  ) {
    log(
      `     mtime diagnostic differs: manifest=${artifacts.manifest.bundleMtimeMs}, disk=${artifacts.bundleMtimeMs}`,
    );
  }
}

export function verifyVsixBundleIdentity(
  vsixPath: string,
  workspaceArtifacts: ExtensionBundleArtifacts,
): ExtensionBundleArtifacts {
  const vsixArtifacts = readVsixBundleArtifacts(vsixPath);
  compareBundleIdentities(workspaceArtifacts.identity, vsixArtifacts.identity, {
    expectedSource: "workspace",
    actualSource: "VSIX",
  });
  return vsixArtifacts;
}

function verifyPackagedBundleIdentity(
  vsixPath: string,
): ExtensionBundleArtifacts {
  log("🔍 Verifying packaged bundle identity...");
  const workspaceArtifacts = readWorkspaceBundleArtifacts();
  const vsixArtifacts = verifyVsixBundleIdentity(vsixPath, workspaceArtifacts);
  logBundleIdentity("workspace", workspaceArtifacts);
  logBundleIdentity("VSIX", vsixArtifacts);
  return workspaceArtifacts;
}

// ---------------------------------------------------------------------------
// Phase 2: Remove conflicting extension variants ("ghosts")
// ---------------------------------------------------------------------------

const CANONICAL_RE = new RegExp(
  `^${CANONICAL_ID.replace(".", "\\.")}-\\d+\\.\\d+\\.\\d+$`,
);

function cleanupVsix(vsixPath: string, installSucceeded: boolean): void {
  if (!installSucceeded) {
    log(`  📦 Keeping VSIX after install failure: ${vsixPath}`);
    return;
  }

  if (process.env["EXOSUIT_KEEP_VSIX"] === "1") {
    log(`  📦 Keeping VSIX because EXOSUIT_KEEP_VSIX=1: ${vsixPath}`);
    return;
  }

  try {
    fs.unlinkSync(vsixPath);
    log(`  🧹 Removed VSIX after successful install: ${vsixPath}`);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    log(`  ⚠️  Could not remove VSIX after successful install: ${message}`);
  }
}

function removeGhostExtensions(extDir: string, label: string): void {
  if (!fs.existsSync(extDir)) return;

  for (const entry of fs.readdirSync(extDir)) {
    const fullPath = path.join(extDir, entry);
    if (!fs.statSync(fullPath).isDirectory()) continue;
    if (!entry.startsWith(`${CANONICAL_PUBLISHER}.`)) continue;

    // Keep the canonical ID (any version).
    if (CANONICAL_RE.test(entry)) continue;

    log(`  🗑️  Removing ghost extension from ${label}: ${entry}`);
    fs.rmSync(fullPath, { recursive: true, force: true });
  }
}

// ---------------------------------------------------------------------------
// Phase 3: Install via `code` CLI
// ---------------------------------------------------------------------------

function resolveCodeBinary(
  runner: CommandRunner = defaultCommandRunner,
): CodeCliLauncher | null {
  const override = process.env["VSCODE_BINARY"];
  if (override) {
    const launcher = new CodeCliLauncher(override, runner);
    const result = launcher.runSync(["--version"], {
      label: `${override} --version`,
      logLauncher: true,
    });
    if (commandSucceeded(result)) {
      log(`  ✅ VS Code CLI: ${override}`);
      return launcher;
    }
    logCommandFailure(`VS Code CLI override failed (${override})`, result);
    return null;
  }

  for (const candidate of ["code", "code-insiders"]) {
    const launcher = new CodeCliLauncher(candidate, runner);
    const result = launcher.runSync(["--version"], {
      label: `${candidate} --version`,
      logLauncher: true,
    });
    if (commandSucceeded(result)) {
      log(`  ✅ VS Code CLI: ${candidate}`);
      return launcher;
    }
    logCommandFailure(`${candidate} --version failed`, result);
  }
  return null;
}

async function installViaCli(
  vsixPath: string,
  version: string,
  runner: CommandRunner = defaultCommandRunner,
): Promise<boolean> {
  const launcher = resolveCodeBinary(runner);
  if (!launcher) {
    log("  ⚠️  No 'code' CLI found");
    return false;
  }

  log(`  → Installing via: ${launcher.binary}`);

  const expectedDir = installedDir(version, launcher.binary);
  const markerFile = installedMarkerFile(version, launcher.binary);
  const beforeMtime = mtimeMs(markerFile);

  const installResult = await launcher.run(
    ["--install-extension", vsixPath, "--force"],
    {
      label: `${launcher.binary} --install-extension`,
      stdio: "inherit",
    },
  );
  if (!commandSucceeded(installResult)) {
    logCommandFailure("code --install-extension failed", installResult);
    return false;
  }

  const MAX_WAIT_S = 30;
  log("  ⏳ Waiting for extension list verification...");

  for (let waited = 0; waited < MAX_WAIT_S * 2; waited++) {
    await sleep(500);
    const listResult = launcher.runSync(
      ["--list-extensions", "--show-versions"],
      {
        label: `${launcher.binary} --list-extensions`,
        logLauncher: waited === 0,
      },
    );
    if (!commandSucceeded(listResult)) {
      logCommandFailure("code --list-extensions failed", listResult);
      continue;
    }
    if (extensionListHasVersion(listResult.stdout, CANONICAL_ID, version)) {
      const currentMtime = mtimeMs(markerFile);
      log(
        `  ✅ Install verified by extension list (${(waited * 0.5).toFixed(1)}s)`,
      );
      if (currentMtime > beforeMtime) {
        log(`     Disk freshness diagnostic advanced: ${markerFile}`);
      } else {
        log(`     Disk freshness diagnostic did not advance: ${markerFile}`);
      }
      return true;
    }
  }

  log(
    `  ❌ Install did not appear in 'code --list-extensions --show-versions' within ${MAX_WAIT_S}s`,
  );
  log(`     Expected: ${expectedDir}`);
  return false;
}

// ---------------------------------------------------------------------------
// Phase 4: Verify
// ---------------------------------------------------------------------------

function verify(
  version: string,
  expectedIdentity: ExtensionBundleIdentity,
  expectedPackageManifest: unknown,
  runner: CommandRunner = defaultCommandRunner,
): boolean {
  log("🔍 Verifying install...");

  let ok = true;
  let installedDirPath = installedDir(version);
  let vscodeExtDir = defaultVscodeExtensionsDir();

  const launcher = resolveCodeBinary(runner);
  if (!launcher) {
    log("  ❌ Could not verify via VS Code CLI: no 'code' CLI found");
    ok = false;
  } else {
    installedDirPath = installedDir(version, launcher.binary);
    vscodeExtDir = defaultVscodeExtensionsDir(launcher.binary);
    const listResult = launcher.runSync(
      ["--list-extensions", "--show-versions"],
      {
        label: `${launcher.binary} --list-extensions`,
      },
    );
    if (!commandSucceeded(listResult)) {
      logCommandFailure("code --list-extensions failed", listResult);
      ok = false;
    } else if (
      extensionListHasVersion(listResult.stdout, CANONICAL_ID, version)
    ) {
      log(`  ✅ CLI extension list: ${CANONICAL_ID}@${version}`);
    } else {
      log(`  ❌ CLI extension list missing ${CANONICAL_ID}@${version}`);
      ok = false;
    }
  }

  if (!fs.existsSync(installedDirPath)) {
    log(`  ⚠️  Expected directory not found: ${installedDirPath}`);
    log(
      "     (This may be normal if VS Code uses a different extensions path)",
    );
  } else {
    try {
      const installedArtifacts = readInstalledBundleArtifacts(installedDirPath);
      if (installedArtifacts) {
        compareBundleIdentities(expectedIdentity, installedArtifacts.identity, {
          expectedSource: "workspace/VSIX",
          actualSource: "installed",
        });
        logBundleIdentity("installed", installedArtifacts);
      } else {
        log(
          "  ⚠️  Installed bundle identity diagnostic unavailable: bundle or manifest missing",
        );
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      log(`  ❌ Installed bundle identity mismatch: ${message}`);
      ok = false;
    }

    // Filesystem package manifest diagnostic
    try {
      const installedPkg = readJsonFile(
        path.join(installedDirPath, "package.json"),
      ) as Record<string, unknown>;
      comparePackageManifests(expectedPackageManifest, installedPkg, {
        expectedSource: "VSIX",
        actualSource: "installed",
      });
      log(
        `  ✅ Filesystem package manifest diagnostic: ${shortHash(
          packageManifestHash(installedPkg),
        )}`,
      );
      if (installedPkg.version === version) {
        log(`  ✅ Filesystem version diagnostic: ${installedPkg.version}`);
      } else {
        ok = false;
        log(
          `  ⚠️  Filesystem version mismatch diagnostic: expected ${version}, got ${installedPkg.version}`,
        );
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      log(`  ❌ Installed package manifest mismatch: ${message}`);
      ok = false;
    }

    // Filesystem freshness diagnostic
    const bundlePath = path.join(installedDirPath, "out/extension.js");
    if (fileExists(bundlePath)) {
      const ageS = (Date.now() - mtimeMs(bundlePath)) / 1000;
      if (ageS < 300) {
        log(
          `  ✅ Filesystem bundle diagnostic: fresh (${ageS.toFixed(0)}s old)`,
        );
      } else {
        log(
          `  ⚠️  Filesystem bundle diagnostic is ${ageS.toFixed(0)}s old — may be stale`,
        );
      }
    } else {
      log("  ⚠️  No extension.js found in installed directory diagnostic");
    }
  }

  // Ghost check
  let ghostCount = 0;
  if (fs.existsSync(vscodeExtDir)) {
    for (const entry of fs.readdirSync(vscodeExtDir)) {
      if (!entry.startsWith(`${CANONICAL_PUBLISHER}.`)) continue;
      if (!fs.statSync(path.join(vscodeExtDir, entry)).isDirectory()) continue;
      if (!entry.startsWith(`${CANONICAL_ID}-`)) {
        log(`  ⚠️  Ghost extension still present: ${entry}`);
        ghostCount++;
      }
    }
  }
  if (ghostCount === 0) {
    log("  ✅ No ghost extensions");
  }

  if (!ok) {
    log("");
    log(
      "  ⚠️  Verification found issues. If the sidebar doesn't work after reload,",
    );
    log("     check the Exosuit output channel for schema validation errors.");
  }

  return ok;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  log("=== Installing Exosuit Extension for Dogfooding ===");

  const vsixPath = buildAndPackage();
  const workspaceArtifacts = verifyPackagedBundleIdentity(vsixPath);
  const vsixPackageManifest = readVsixPackageManifest(vsixPath);

  const version = readPackageVersion(path.join(EXTENSION_DIR, "package.json"));
  if (workspaceArtifacts.identity.packageVersion !== version) {
    throw new Error(
      `Workspace bundle manifest package version mismatch: expected ${version}, got ${workspaceArtifacts.identity.packageVersion}`,
    );
  }

  log("🧹 Checking for conflicting extension variants...");
  const home = process.env["HOME"] ?? "~";
  removeGhostExtensions(
    path.join(home, ".vscode/extensions"),
    ".vscode/extensions",
  );
  removeGhostExtensions(
    path.join(home, ".vscode-server/extensions"),
    ".vscode-server/extensions",
  );

  const installed = await installViaCli(vsixPath, version);
  if (!installed) {
    cleanupVsix(vsixPath, false);
    console.error(
      "Error: Could not install extension. The 'code' CLI is required.",
    );
    console.error("");
    console.error(
      "  Make sure VS Code is installed and 'code' is on your PATH.",
    );
    console.error(
      "  You can also set VSCODE_BINARY to point to the code binary.",
    );
    process.exit(1);
  }

  if (process.env["EXOSUIT_SKIP_VERIFY"] !== "1") {
    const verified = verify(
      version,
      workspaceArtifacts.identity,
      vsixPackageManifest,
    );
    if (!verified) {
      cleanupVsix(vsixPath, false);
      console.error("Error: Extension install verification failed.");
      process.exit(1);
    }
  }

  cleanupVsix(vsixPath, true);

  log("=== Done! ===");
  log(
    `Expected runtime build stamp after reload: ${workspaceArtifacts.identity.buildStamp}`,
  );
  log("Run 'Developer: Reload Window' from the Command Palette to activate.");
}

if (isCliEntrypoint()) {
  main().catch((err) => {
    console.error(err);
    process.exit(1);
  });
}
