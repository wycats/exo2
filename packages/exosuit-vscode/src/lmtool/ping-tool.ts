import * as vscode from "vscode";

declare const __BUILD_STAMP__: string;

/**
 * Input schema for the exo-ping LM tool. Intentionally empty — this is a
 * trivial diagnostic tool.
 */
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
interface PingToolInput {}

export interface PingToolIdentity {
  buildStamp?: string;
  pid?: number;
  extensionPath?: string;
  extensionUri?: string;
  extensionMode?: string;
  extensionKind?: string;
}

function modeName(mode: vscode.ExtensionMode | undefined): string | undefined {
  if (mode === undefined) {
    return undefined;
  }
  return vscode.ExtensionMode[mode] ?? String(mode);
}

function kindName(kind: vscode.ExtensionKind | undefined): string | undefined {
  if (kind === undefined) {
    return undefined;
  }
  return vscode.ExtensionKind[kind] ?? String(kind);
}

export function createPingToolIdentity(
  context: vscode.ExtensionContext,
): PingToolIdentity {
  return {
    buildStamp:
      typeof __BUILD_STAMP__ === "string" ? __BUILD_STAMP__ : "unknown",
    pid: process.pid,
    extensionPath: context.extensionPath,
    extensionUri: context.extensionUri.toString(),
    extensionMode: modeName(context.extensionMode),
    extensionKind: kindName(context.extension.extensionKind),
  };
}

function formatIdentity(identity: PingToolIdentity): string {
  return [
    `pong. build: ${identity.buildStamp ?? "unknown"}.`,
    `pid: ${identity.pid ?? process.pid}.`,
    `extensionPath: ${identity.extensionPath ?? "unknown"}.`,
    `extensionUri: ${identity.extensionUri ?? "unknown"}.`,
    `extensionMode: ${identity.extensionMode ?? "unknown"}.`,
    `extensionKind: ${identity.extensionKind ?? "unknown"}.`,
  ].join(" ");
}

/**
 * Creates the exo-ping LM tool.
 *
 * A diagnostic tool with no dependencies beyond the extension process itself.
 * Its job is to confirm whether the extension's LM tool registration is
 * functioning, independent of any other tool or the daemon.
 *
 * Use case: when `exo-run` reports "currently disabled by the user" but the
 * picker shows it enabled (a known VS Code chat-layer bug, tracked at
 * https://github.com/microsoft/vscode/issues/295683), invoke `exo-ping` to
 * distinguish:
 *
 *   - If `exo-ping` also fails → the whole extension's tool map is corrupted
 *     in the chat layer. Reload window.
 *   - If `exo-ping` succeeds → the corruption is per-tool. Try
 *     `Exo: Reset LM Tools` to re-register, then toggle the picker.
 *
 * Returns a one-line response containing the build stamp so the caller can
 * confirm which bundle is loaded.
 */
export function createPingTool(
  identity: PingToolIdentity = {
    buildStamp:
      typeof __BUILD_STAMP__ === "string" ? __BUILD_STAMP__ : "unknown",
    pid: process.pid,
  },
): vscode.LanguageModelTool<PingToolInput> {
  return {
    async invoke(
      _options: vscode.LanguageModelToolInvocationOptions<PingToolInput>,
      _token: vscode.CancellationToken,
    ): Promise<vscode.LanguageModelToolResult> {
      return new vscode.LanguageModelToolResult([
        new vscode.LanguageModelTextPart(formatIdentity(identity)),
      ]);
    },
  };
}
