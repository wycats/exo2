import { EventEmitter } from "events";
import { type Logger, createNoopLogger } from "../Logger.ts";

export type Digest = string;
export type RootID = string;

export interface RootState<T = any> {
  digest: Digest;
  value: T;
  status: "valid" | "fetching" | "stale";
}

export type ValidationStatus =
  | { type: "valid" }
  | { type: "invalid"; newDigest: Digest };

export interface ServerInterface {
  validate(
    digests: Record<RootID, Digest>
  ): Promise<Record<RootID, ValidationStatus>>;
  fetch(rootId: RootID, expectedDigest: Digest): Promise<any>;
}

export class ExosuitClient extends EventEmitter {
  private roots: Map<RootID, RootState> = new Map();
  private server: ServerInterface;
  private logger: Logger;

  constructor(server: ServerInterface, logger?: Logger) {
    super();
    this.server = server;
    this.logger = logger ?? createNoopLogger("core");
  }

  public registerRoot(
    rootId: RootID,
    initialValue: any,
    initialDigest: Digest
  ) {
    this.roots.set(rootId, {
      digest: initialDigest,
      value: initialValue,
      status: "valid",
    });
  }

  public getRoot(rootId: RootID): RootState | undefined {
    return this.roots.get(rootId);
  }

  public async reconcile() {
    // 1. Collect current digests
    const currentDigests: Record<RootID, Digest> = {};
    for (const [id, root] of this.roots) {
      currentDigests[id] = root.digest;
    }

    // 2. Validate with Server
    const validationResults = await this.server.validate(currentDigests);

    // 3. Handle Invalid Roots
    const fetchPromises: Promise<void>[] = [];
    for (const [id, status] of Object.entries(validationResults)) {
      if (status.type === "invalid") {
        fetchPromises.push(this.fetchRoot(id, status.newDigest));
      }
    }

    await Promise.all(fetchPromises);
    this.emit("reconciled");
  }

  private async fetchRoot(rootId: RootID, expectedDigest: Digest) {
    const root = this.roots.get(rootId);
    if (!root) return;

    root.status = "fetching";
    this.emit("change", rootId);

    try {
      const newValue = await this.server.fetch(rootId, expectedDigest);

      // Update state
      root.value = newValue;
      root.digest = expectedDigest;
      root.status = "valid";

      this.emit("change", rootId);
    } catch (e) {
      this.logger.error(`Failed to fetch root ${rootId}`, e);
      root.status = "stale"; // Or error
      this.emit("change", rootId);
    }
  }
}
