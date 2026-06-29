import { fileURLToPath } from "node:url";
import * as fs from "node:fs";
import { Buffer } from "node:buffer";

function readWasmBytesFromUrl(url: URL): Uint8Array<ArrayBufferLike> {
  if (url.protocol === "data:") {
    const comma = url.href.indexOf(",");
    if (comma === -1) {
      throw new Error("UlidService: Invalid data: URL for WASM");
    }

    const metadata = url.href.slice(0, comma);
    const payload = url.href.slice(comma + 1);

    if (!metadata.includes(";base64")) {
      throw new Error(
        "UlidService: Expected base64-encoded data: URL for WASM"
      );
    }

    const buffer = Buffer.from(payload, "base64");
    return new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
  }

  if (url.protocol !== "file:") {
    throw new Error(
      `UlidService: Unsupported WASM URL protocol: ${url.protocol}`
    );
  }

  const buffer = fs.readFileSync(fileURLToPath(url));
  return new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
}

/**
 * Result of parsing a canonical reference (e.g., "phase@01HZ...").
 */
export interface CanonicalRef {
  /** The type name (e.g., "phase", "task", "epoch"). */
  typeName: string;
  /** The ULID string. */
  ulid: string;
}

/**
 * Service for ULID generation and manipulation, backed by Rust WASM.
 *
 * ULIDs (Universally Unique Lexicographically Sortable Identifiers) are used
 * as canonical identifiers for epochs, phases, tasks, and other entities.
 *
 * @example
 * ```typescript
 * const service = new UlidService();
 * await service.initialize();
 *
 * const ulid = await service.generateUlid();
 * const ref = await service.formatCanonicalRef("phase", ulid);
 * // ref = "phase@01HZ..."
 * ```
 */
export class UlidService {
  private _engine: any | undefined;
  private _ready: Promise<void> | undefined;

  /**
   * Initialize the WASM module. Must be called before any other method.
   * Subsequent calls are no-ops.
   */
  public async initialize(): Promise<void> {
    if (this._ready) {
      return this._ready;
    }

    this._ready = (async () => {
      const wasmModule = await import("../wasm/exosuit_ulid.js");

      // Use `new URL(..., import.meta.url)` so Vite can rewrite and emit the
      // `.wasm` as a file asset for the extension bundle.
      const wasmFileUrl = new URL(
        "../wasm/exosuit_ulid_bg.wasm",
        import.meta.url
      );
      const wasmBytes = readWasmBytesFromUrl(wasmFileUrl);

      if (typeof (wasmModule as any).initSync !== "function") {
        throw new Error("UlidService: initSync not found in WASM module");
      }
      (wasmModule as any).initSync(wasmBytes);

      const WasmUlidConstructor = (wasmModule as any).WasmUlid;
      if (!WasmUlidConstructor) {
        throw new Error("UlidService: WasmUlid constructor not found");
      }

      this._engine = new WasmUlidConstructor();
    })();

    return this._ready;
  }

  /**
   * Generate a new ULID.
   * @returns A 26-character ULID string.
   */
  public async generateUlid(): Promise<string> {
    await this.initialize();
    if (!this._engine) {
      throw new Error("UlidService: Engine not initialized");
    }
    return this._engine.generateUlid();
  }

  /**
   * Format a ULID as a canonical reference.
   * @param typeName - The entity type (e.g., "phase", "task", "epoch").
   * @param ulid - The ULID string.
   * @returns A canonical reference string (e.g., "phase@01HZ...").
   * @throws If the ULID is invalid.
   */
  public async formatCanonicalRef(
    typeName: string,
    ulid: string
  ): Promise<string> {
    await this.initialize();
    if (!this._engine) {
      throw new Error("UlidService: Engine not initialized");
    }
    return this._engine.formatCanonicalRef(typeName, ulid);
  }

  /**
   * Parse a ULID string.
   * @param s - The string to parse.
   * @returns The ULID string if valid, null otherwise.
   */
  public async parseUlid(s: string): Promise<string | null> {
    await this.initialize();
    if (!this._engine) {
      throw new Error("UlidService: Engine not initialized");
    }
    return this._engine.parseUlid(s);
  }

  /**
   * Check if a string is a valid ULID.
   * @param s - The string to check.
   * @returns True if the string is a valid ULID.
   */
  public async isValidUlid(s: string): Promise<boolean> {
    await this.initialize();
    if (!this._engine) {
      throw new Error("UlidService: Engine not initialized");
    }
    return this._engine.isValidUlid(s);
  }

  /**
   * Parse a canonical reference string (e.g., "phase@01HZ...").
   * @param s - The canonical reference string.
   * @returns The parsed reference, or null if invalid.
   */
  public async parseCanonicalRef(s: string): Promise<CanonicalRef | null> {
    await this.initialize();
    if (!this._engine) {
      throw new Error("UlidService: Engine not initialized");
    }
    return this._engine.parseCanonicalRef(s);
  }
}

/**
 * Singleton instance of UlidService.
 */
export const ulidService = new UlidService();
