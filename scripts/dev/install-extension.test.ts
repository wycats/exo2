import { describe, expect, it } from "vitest";
import { createHash } from "crypto";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { deflateRawSync } from "zlib";

import {
  CodeCliLauncher,
  comparePackageManifests,
  compareBundleIdentities,
  defaultVscodeExtensionsDir,
  extensionListHasVersion,
  packageManifestHash,
  installedDir,
  parseExtensionBundleManifest,
  readPackageVersion,
  readInstalledBundleArtifacts,
  readVsixPackageManifest,
  readVsixBundleArtifacts,
  readWorkspaceBundleArtifacts,
  selectExpectedVsix,
  verifyVsixBundleIdentity,
  type CommandRunner,
  type CommandRunnerOptions,
  type ExtensionBundleIdentity,
  type ExtensionBundleManifest,
} from "./install-extension";

class FakeRunner implements CommandRunner {
  readonly calls: Array<{
    command: string;
    args: readonly string[];
    shell: boolean;
  }> = [];
  #syncResults: Array<{
    status: number | null;
    signal: NodeJS.Signals | null;
    stdout: string;
    stderr: string;
    error?: NodeJS.ErrnoException;
  }>;
  #asyncResults: Array<{
    status: number | null;
    signal: NodeJS.Signals | null;
    stdout: string;
    stderr: string;
    error?: NodeJS.ErrnoException;
  }>;

  constructor(options: {
    syncResults?: Array<{
      status: number | null;
      signal: NodeJS.Signals | null;
      stdout?: string;
      stderr?: string;
      error?: NodeJS.ErrnoException;
    }>;
    asyncResults?: Array<{
      status: number | null;
      signal: NodeJS.Signals | null;
      stdout?: string;
      stderr?: string;
      error?: NodeJS.ErrnoException;
    }>;
  }) {
    this.#syncResults = (options.syncResults ?? []).map((result) => ({
      stdout: "",
      stderr: "",
      ...result,
    }));
    this.#asyncResults = (options.asyncResults ?? []).map((result) => ({
      stdout: "",
      stderr: "",
      ...result,
    }));
  }

  runSync(
    command: string,
    args: readonly string[],
    options: CommandRunnerOptions,
  ) {
    this.calls.push({ command, args, shell: options.shell });
    return (
      this.#syncResults.shift() ?? {
        status: 0,
        signal: null,
        stdout: "",
        stderr: "",
      }
    );
  }

  async run(
    command: string,
    args: readonly string[],
    options: CommandRunnerOptions,
  ) {
    this.calls.push({ command, args, shell: options.shell });
    return (
      this.#asyncResults.shift() ?? {
        status: 0,
        signal: null,
        stdout: "",
        stderr: "",
      }
    );
  }
}

function withTempDir<T>(fn: (dir: string) => T): T {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "install-extension-test-"));
  try {
    return fn(dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

function sha256(bytes: Buffer): string {
  return createHash("sha256").update(bytes).digest("hex");
}

function bundleSource(stamp = "2026-05-17T16:20:00.000Z"): Buffer {
  return Buffer.from(
    `console.log("[exosuit] Build stamp: ${stamp}");\n`,
    "utf-8",
  );
}

function manifestForBundle(
  bundleBytes: Buffer,
  overrides: Partial<ExtensionBundleManifest> = {},
): ExtensionBundleManifest {
  return {
    schema: 2,
    kind: "exosuit-vscode-extension-bundle",
    extensionId: "exosuit.exosuit-context",
    packageVersion: "0.0.12",
    daemonRuntimePaths: "project-resolve",
    bundle: "out/extension.js",
    buildStamp: "2026-05-17T16:20:00.000Z",
    bundleSha256: sha256(bundleBytes),
    bundleSizeBytes: bundleBytes.byteLength,
    bundleMtimeMs: 1234,
    generatedAt: "2026-05-17T16:21:00.000Z",
    ...overrides,
  };
}

function writeBundleArtifacts(
  root: string,
  bundleBytes: Buffer,
  manifest: ExtensionBundleManifest = manifestForBundle(bundleBytes),
): void {
  const outDir = path.join(root, "out");
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(path.join(outDir, "extension.js"), bundleBytes);
  fs.writeFileSync(
    path.join(outDir, "dev-host-bundle.json"),
    `${JSON.stringify(manifest, null, 2)}\n`,
    "utf-8",
  );
}

function crc32(bytes: Buffer): number {
  let crc = ~0;
  for (const byte of bytes) {
    crc ^= byte;
    for (let i = 0; i < 8; i++) {
      crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
    }
  }
  return ~crc >>> 0;
}

function writeTinyZip(zipPath: string, entries: Record<string, Buffer>): void {
  const fileParts: Buffer[] = [];
  const centralDirectoryParts: Buffer[] = [];
  let offset = 0;

  for (const [name, data] of Object.entries(entries)) {
    const nameBytes = Buffer.from(name, "utf-8");
    const compressed = deflateRawSync(data);
    const checksum = crc32(data);

    const localHeader = Buffer.alloc(30);
    localHeader.writeUInt32LE(0x04034b50, 0);
    localHeader.writeUInt16LE(20, 4);
    localHeader.writeUInt16LE(0, 6);
    localHeader.writeUInt16LE(8, 8);
    localHeader.writeUInt32LE(0, 10);
    localHeader.writeUInt32LE(checksum, 14);
    localHeader.writeUInt32LE(compressed.length, 18);
    localHeader.writeUInt32LE(data.length, 22);
    localHeader.writeUInt16LE(nameBytes.length, 26);
    localHeader.writeUInt16LE(0, 28);
    fileParts.push(localHeader, nameBytes, compressed);

    const centralDirectoryHeader = Buffer.alloc(46);
    centralDirectoryHeader.writeUInt32LE(0x02014b50, 0);
    centralDirectoryHeader.writeUInt16LE(20, 4);
    centralDirectoryHeader.writeUInt16LE(20, 6);
    centralDirectoryHeader.writeUInt16LE(0, 8);
    centralDirectoryHeader.writeUInt16LE(8, 10);
    centralDirectoryHeader.writeUInt32LE(0, 12);
    centralDirectoryHeader.writeUInt32LE(checksum, 16);
    centralDirectoryHeader.writeUInt32LE(compressed.length, 20);
    centralDirectoryHeader.writeUInt32LE(data.length, 24);
    centralDirectoryHeader.writeUInt16LE(nameBytes.length, 28);
    centralDirectoryHeader.writeUInt16LE(0, 30);
    centralDirectoryHeader.writeUInt16LE(0, 32);
    centralDirectoryHeader.writeUInt16LE(0, 34);
    centralDirectoryHeader.writeUInt16LE(0, 36);
    centralDirectoryHeader.writeUInt32LE(0, 38);
    centralDirectoryHeader.writeUInt32LE(offset, 42);
    centralDirectoryParts.push(centralDirectoryHeader, nameBytes);

    offset += localHeader.length + nameBytes.length + compressed.length;
  }

  const centralDirectoryOffset = offset;
  const centralDirectorySize = centralDirectoryParts.reduce(
    (size, part) => size + part.length,
    0,
  );
  const endOfCentralDirectory = Buffer.alloc(22);
  const entryCount = Object.keys(entries).length;
  endOfCentralDirectory.writeUInt32LE(0x06054b50, 0);
  endOfCentralDirectory.writeUInt16LE(0, 4);
  endOfCentralDirectory.writeUInt16LE(0, 6);
  endOfCentralDirectory.writeUInt16LE(entryCount, 8);
  endOfCentralDirectory.writeUInt16LE(entryCount, 10);
  endOfCentralDirectory.writeUInt32LE(centralDirectorySize, 12);
  endOfCentralDirectory.writeUInt32LE(centralDirectoryOffset, 16);
  endOfCentralDirectory.writeUInt16LE(0, 20);

  fs.writeFileSync(
    zipPath,
    Buffer.concat([
      ...fileParts,
      ...centralDirectoryParts,
      endOfCentralDirectory,
    ]),
  );
}

function writeVsix(
  vsixPath: string,
  bundleBytes: Buffer,
  manifest: ExtensionBundleManifest = manifestForBundle(bundleBytes),
  packageManifest: unknown = {
    name: "exosuit-context",
    version: manifest.packageVersion,
    contributes: {},
  },
): void {
  writeTinyZip(vsixPath, {
    "extension/package.json": Buffer.from(
      JSON.stringify(packageManifest),
      "utf-8",
    ),
    "extension/out/extension.js": bundleBytes,
    "extension/out/dev-host-bundle.json": Buffer.from(
      JSON.stringify(manifest),
      "utf-8",
    ),
  });
}

function identity(
  overrides: Partial<ExtensionBundleIdentity> = {},
): ExtensionBundleIdentity {
  return {
    extensionId: "exosuit.exosuit-context",
    packageVersion: "0.0.12",
    daemonRuntimePaths: "project-resolve",
    bundle: "out/extension.js",
    buildStamp: "2026-05-17T16:20:00.000Z",
    bundleSha256: "a".repeat(64),
    bundleSizeBytes: 42,
    ...overrides,
  };
}

describe("install-extension helpers", () => {
  it("selects the expected VSIX for the package version", () => {
    withTempDir((dir) => {
      const expected = path.join(dir, "exosuit-context-0.0.12.vsix");
      fs.writeFileSync(expected, "vsix", "utf-8");

      expect(selectExpectedVsix(dir, "0.0.12")).toBe(expected);
    });
  });

  it("fails when the expected VSIX is missing even if a stale newer VSIX exists", () => {
    withTempDir((dir) => {
      fs.writeFileSync(
        path.join(dir, "exosuit-context-0.0.13.vsix"),
        "stale",
        "utf-8",
      );

      expect(() => selectExpectedVsix(dir, "0.0.12")).toThrow(
        /Expected VSIX was not generated/,
      );
    });
  });

  it("uses the direct CLI launcher when direct spawn works", () => {
    const runner = new FakeRunner({
      syncResults: [{ status: 0, signal: null, stdout: "1.0.0\n" }],
    });
    const launcher = new CodeCliLauncher("code", runner, () => undefined);

    const result = launcher.runSync(["--version"], { label: "code --version" });

    expect(result.mode).toBe("direct");
    expect(runner.calls).toEqual([
      { command: "code", args: ["--version"], shell: false },
    ]);
  });

  it("falls back to the shell launcher when direct spawn reports ENOENT", () => {
    const enoent = Object.assign(new Error("spawn code ENOENT"), {
      code: "ENOENT",
    }) as NodeJS.ErrnoException;
    const runner = new FakeRunner({
      syncResults: [
        { status: null, signal: null, error: enoent },
        { status: 0, signal: null, stdout: "1.0.0\n" },
      ],
    });
    const launcher = new CodeCliLauncher("code", runner, () => undefined);

    const result = launcher.runSync(["--version"], { label: "code --version" });

    expect(result.mode).toBe("shell");
    expect(runner.calls).toEqual([
      { command: "code", args: ["--version"], shell: false },
      { command: "'code' '--version'", args: [], shell: true },
    ]);
  });

  it("recognizes exosuit.exosuit-context@0.0.12 from CLI list verification", () => {
    expect(
      extensionListHasVersion(
        "publisher.other@1.0.0\nexosuit.exosuit-context@0.0.12\n",
        "exosuit.exosuit-context",
        "0.0.12",
      ),
    ).toBe(true);
  });

  it("uses the platform home directory for installed extension diagnostics", () => {
    expect(defaultVscodeExtensionsDir()).not.toContain("~");
    expect(installedDir("0.0.12")).toBe(
      path.join(defaultVscodeExtensionsDir(), "exosuit.exosuit-context-0.0.12"),
    );
  });

  it("uses the Insiders extension directory for code-insiders diagnostics", () => {
    expect(defaultVscodeExtensionsDir("code-insiders")).toBe(
      path.join(os.homedir(), ".vscode-insiders/extensions"),
    );
    expect(installedDir("0.0.12", "/usr/local/bin/code-insiders")).toBe(
      path.join(
        os.homedir(),
        ".vscode-insiders/extensions/exosuit.exosuit-context-0.0.12",
      ),
    );
  });

  it("parses schema 2 bundle manifests with identity fields", () => {
    const bundleBytes = bundleSource();
    const manifest = parseExtensionBundleManifest(
      manifestForBundle(bundleBytes),
      "test",
    );

    expect(manifest).toMatchObject({
      schema: 2,
      kind: "exosuit-vscode-extension-bundle",
      extensionId: "exosuit.exosuit-context",
      packageVersion: "0.0.12",
      daemonRuntimePaths: "project-resolve",
      bundle: "out/extension.js",
      buildStamp: "2026-05-17T16:20:00.000Z",
      bundleSha256: sha256(bundleBytes),
      bundleSizeBytes: bundleBytes.byteLength,
      bundleMtimeMs: 1234,
      generatedAt: "2026-05-17T16:21:00.000Z",
    });
  });

  it("rejects old bundle manifest schemas", () => {
    const bundleBytes = bundleSource();

    expect(() =>
      parseExtensionBundleManifest(
        manifestForBundle(bundleBytes, { schema: 1 }),
        "workspace",
      ),
    ).toThrow(/manifest schema mismatch/);
  });

  it("verifies workspace bundle identity by hash, stamp, and size", () => {
    withTempDir((dir) => {
      const bundleBytes = bundleSource();
      writeBundleArtifacts(dir, bundleBytes);

      const artifacts = readWorkspaceBundleArtifacts(dir);

      expect(artifacts.identity).toMatchObject({
        buildStamp: "2026-05-17T16:20:00.000Z",
        bundleSha256: sha256(bundleBytes),
        bundleSizeBytes: bundleBytes.byteLength,
      });
    });
  });

  it("rejects a workspace manifest with a stale bundle hash", () => {
    withTempDir((dir) => {
      const bundleBytes = bundleSource();
      writeBundleArtifacts(
        dir,
        bundleBytes,
        manifestForBundle(bundleBytes, { bundleSha256: "b".repeat(64) }),
      );

      expect(() => readWorkspaceBundleArtifacts(dir)).toThrow(
        /Bundle identity mismatch: bundleSha256/,
      );
    });
  });

  it("reads VSIX bundle identity without a ZIP dependency", () => {
    withTempDir((dir) => {
      const bundleBytes = bundleSource();
      const vsixPath = path.join(dir, "exosuit-context-0.0.12.vsix");
      writeVsix(vsixPath, bundleBytes);

      const artifacts = readVsixBundleArtifacts(vsixPath);

      expect(artifacts.identity.bundleSha256).toBe(sha256(bundleBytes));
      expect(artifacts.identity.bundleSizeBytes).toBe(bundleBytes.byteLength);
      expect(artifacts.identity.buildStamp).toBe("2026-05-17T16:20:00.000Z");
    });
  });

  it("reads VSIX package manifest without a ZIP dependency", () => {
    withTempDir((dir) => {
      const bundleBytes = bundleSource();
      const vsixPath = path.join(dir, "exosuit-context-0.0.12.vsix");
      writeVsix(vsixPath, bundleBytes, manifestForBundle(bundleBytes), {
        name: "exosuit-context",
        version: "0.0.12",
        contributes: {
          views: {
            "exosuit-plan": [{ id: "exosuit.sidecarStatus" }],
          },
        },
      });

      expect(readVsixPackageManifest(vsixPath)).toMatchObject({
        name: "exosuit-context",
        version: "0.0.12",
        contributes: {
          views: {
            "exosuit-plan": [{ id: "exosuit.sidecarStatus" }],
          },
        },
      });
    });
  });

  it("detects stale installed package manifests with matching versions", () => {
    const expected = {
      name: "exosuit-context",
      version: "0.0.12",
      contributes: {
        views: {
          "exosuit-plan": [
            { id: "exosuit.projectPlan" },
            { id: "exosuit.sidecarStatus" },
          ],
        },
      },
    };
    const installed = {
      name: "exosuit-context",
      version: "0.0.12",
      contributes: {
        views: {
          "exosuit-plan": [{ id: "exosuit.projectPlan" }],
        },
      },
    };

    expect(packageManifestHash(expected)).not.toBe(
      packageManifestHash(installed),
    );
    expect(() =>
      comparePackageManifests(expected, installed, {
        expectedSource: "VSIX",
        actualSource: "installed",
      }),
    ).toThrow(/Package manifest mismatch/);
  });

  it("ignores VS Code installed package metadata when comparing manifests", () => {
    const expected = {
      name: "exosuit-context",
      version: "0.0.12",
      contributes: {
        views: {
          "exosuit-plan": [
            { id: "exosuit.projectPlan" },
            { id: "exosuit.sidecarStatus" },
          ],
        },
      },
    };
    const installed = {
      version: "0.0.12",
      contributes: {
        views: {
          "exosuit-plan": [
            { id: "exosuit.projectPlan" },
            { id: "exosuit.sidecarStatus" },
          ],
        },
      },
      name: "exosuit-context",
      __metadata: {
        installedTimestamp: Date.now(),
        targetPlatform: "undefined",
        size: 12345,
      },
    };

    expect(packageManifestHash(expected)).toBe(packageManifestHash(installed));
    expect(() =>
      comparePackageManifests(expected, installed, {
        expectedSource: "VSIX",
        actualSource: "installed",
      }),
    ).not.toThrow();
  });

  it("detects workspace-to-VSIX identity mismatches", () => {
    withTempDir((dir) => {
      const workspaceBundle = bundleSource();
      const vsixBundle = bundleSource("2026-05-17T16:25:00.000Z");
      writeBundleArtifacts(dir, workspaceBundle);
      const vsixPath = path.join(dir, "exosuit-context-0.0.12.vsix");
      writeVsix(
        vsixPath,
        vsixBundle,
        manifestForBundle(vsixBundle, {
          buildStamp: "2026-05-17T16:25:00.000Z",
        }),
      );

      const workspaceArtifacts = readWorkspaceBundleArtifacts(dir);

      expect(() =>
        verifyVsixBundleIdentity(vsixPath, workspaceArtifacts),
      ).toThrow(/Bundle identity mismatch: buildStamp/);
    });
  });

  it("detects stale installed bundle identity", () => {
    withTempDir((dir) => {
      const expected = bundleSource();
      const installed = bundleSource("2026-05-17T16:25:00.000Z");
      writeBundleArtifacts(
        dir,
        installed,
        manifestForBundle(installed, {
          buildStamp: "2026-05-17T16:25:00.000Z",
        }),
      );

      const installedArtifacts = readInstalledBundleArtifacts(dir);
      if (!installedArtifacts) {
        throw new Error("Expected installed artifacts to be readable");
      }

      expect(() =>
        compareBundleIdentities(
          {
            ...installedArtifacts.identity,
            buildStamp: "2026-05-17T16:20:00.000Z",
            bundleSha256: sha256(expected),
            bundleSizeBytes: expected.byteLength,
          },
          installedArtifacts.identity,
          { expectedSource: "VSIX", actualSource: "installed" },
        ),
      ).toThrow(/Bundle identity mismatch: buildStamp/);
    });
  });

  it("returns null when installed bundle identity files are unavailable", () => {
    withTempDir((dir) => {
      expect(readInstalledBundleArtifacts(dir)).toBeNull();
    });
  });

  it("compares identity by hash and ignores mtime diagnostics", () => {
    const expected = identity({ bundleSha256: "a".repeat(64) });
    const actual = identity({ bundleSha256: "b".repeat(64) });

    expect(() =>
      compareBundleIdentities(expected, actual, {
        expectedSource: "workspace",
        actualSource: "installed",
      }),
    ).toThrow(/bundleSha256/);
  });

  it("does not treat non-hash identity metadata as bundle identity", () => {
    expect(() =>
      compareBundleIdentities(
        {
          buildStamp: "2026-05-17T16:20:00.000Z",
          bundleSha256: "a".repeat(64),
          bundleSizeBytes: 42,
        },
        {
          buildStamp: "2026-05-17T16:20:00.000Z",
          bundleSha256: "a".repeat(64),
          bundleSizeBytes: 42,
        },
        { expectedSource: "workspace", actualSource: "installed" },
      ),
    ).not.toThrow();
  });

  it("requires a non-empty package version", () => {
    withTempDir((dir) => {
      const packageJsonPath = path.join(dir, "package.json");
      fs.writeFileSync(packageJsonPath, JSON.stringify({ name: "demo" }));

      expect(() => readPackageVersion(packageJsonPath)).toThrow(
        /package\.json\.version must be a non-empty string/,
      );
    });
  });
});
