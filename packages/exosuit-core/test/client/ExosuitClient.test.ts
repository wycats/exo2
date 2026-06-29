import { expect, describe, it } from "vitest";
import { ExosuitClient } from "../../src/client/ExosuitClient.ts";
import type {
  ServerInterface,
  ValidationStatus,
} from "../../src/client/ExosuitClient.ts";

class MockServer implements ServerInterface {
  private roots: Record<string, { digest: string; value: any }> = {};

  constructor() {
    this.roots["root1"] = { digest: "d1", value: "v1" };
  }

  updateRoot(id: string, digest: string, value: any) {
    this.roots[id] = { digest, value };
  }

  async validate(
    digests: Record<string, string>,
  ): Promise<Record<string, ValidationStatus>> {
    const results: Record<string, ValidationStatus> = {};
    for (const [id, digest] of Object.entries(digests)) {
      if (this.roots[id] && this.roots[id].digest === digest) {
        results[id] = { type: "valid" };
      } else if (this.roots[id]) {
        results[id] = { type: "invalid", newDigest: this.roots[id].digest };
      } else {
        // Root deleted or not found, treat as invalid/stale
        results[id] = { type: "invalid", newDigest: "deleted" };
      }
    }
    return results;
  }

  async fetch(rootId: string, expectedDigest: string): Promise<any> {
    const root = this.roots[rootId];
    if (root && root.digest === expectedDigest) {
      return root.value;
    }
    throw new Error(`Stale: expected ${expectedDigest}, found ${root?.digest}`);
  }
}

describe("ExosuitClient", () => {
  it("should reconcile state correctly", async () => {
    const server = new MockServer();
    const client = new ExosuitClient(server);

    // Initial state
    client.registerRoot("root1", "v1", "d1");

    // 1. Reconcile (should be valid)
    await client.reconcile();
    expect(client.getRoot("root1")?.status).to.equal("valid");
    expect(client.getRoot("root1")?.value).to.equal("v1");

    // 2. Update server
    server.updateRoot("root1", "d2", "v2");

    // 3. Reconcile (should detect invalid and fetch)
    await client.reconcile();
    expect(client.getRoot("root1")?.status).to.equal("valid");
    expect(client.getRoot("root1")?.value).to.equal("v2");
    expect(client.getRoot("root1")?.digest).to.equal("d2");
  });
});
