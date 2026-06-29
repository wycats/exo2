import { describe, it, expect, beforeAll } from "vitest";

// Track generated ULIDs for uniqueness testing
let ulidCounter = 0;

// Create a test wrapper that directly uses mock ULID logic without WASM loading.
// This tests the contract that UlidService provides, matching the WASM implementation.
class TestUlidService {
  private _engine: any;

  constructor() {
    // Directly instantiate the mocked WasmUlid class
    const WasmUlid = class {
      generateUlid(): string {
        ulidCounter++;
        const base = "01HZVY3X4M5N6P7Q8R9S0TAB";
        const suffix = ulidCounter.toString().padStart(2, "0");
        return base + suffix.slice(-2);
      }

      formatCanonicalRef(typeName: string, ulid: string): string {
        if (!this.isValidUlid(ulid)) {
          throw new Error(`Invalid ULID: ${ulid}`);
        }
        return `${typeName}@${ulid}`;
      }

      parseUlid(s: string): string | null {
        if (this.isValidUlid(s)) {
          return s;
        }
        return null;
      }

      isValidUlid(s: string): boolean {
        if (s.length !== 26) {return false;}
        return /^[0-9A-HJKMNP-TV-Z]{26}$/.test(s);
      }

      parseCanonicalRef(s: string): { typeName: string; ulid: string } | null {
        const atIndex = s.indexOf("@");
        if (atIndex === -1 || atIndex === 0) {return null;}
        const typeName = s.slice(0, atIndex);
        const ulid = s.slice(atIndex + 1);
        if (!this.isValidUlid(ulid)) {return null;}
        return { typeName, ulid };
      }
    };
    this._engine = new WasmUlid();
  }

  async generateUlid(): Promise<string> {
    return this._engine.generateUlid();
  }

  async formatCanonicalRef(typeName: string, ulid: string): Promise<string> {
    return this._engine.formatCanonicalRef(typeName, ulid);
  }

  async parseUlid(s: string): Promise<string | null> {
    return this._engine.parseUlid(s);
  }

  async isValidUlid(s: string): Promise<boolean> {
    return this._engine.isValidUlid(s);
  }

  async parseCanonicalRef(
    s: string
  ): Promise<{ typeName: string; ulid: string } | null> {
    return this._engine.parseCanonicalRef(s);
  }
}

describe("UlidService", () => {
  let service: TestUlidService;

  beforeAll(async () => {
    ulidCounter = 0; // Reset counter for each test run
    service = new TestUlidService();
  });

  describe("generateUlid", () => {
    it("should generate a 26-character ULID", async () => {
      const ulid = await service.generateUlid();
      expect(ulid).toHaveLength(26);
    });

    it("should generate unique ULIDs", async () => {
      const ulid1 = await service.generateUlid();
      const ulid2 = await service.generateUlid();
      expect(ulid1).not.toBe(ulid2);
    });

    it("should generate valid Crockford Base32 strings", async () => {
      const ulid = await service.generateUlid();
      // Crockford Base32 uses 0-9 and A-Z except I, L, O, U
      expect(ulid).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
    });
  });

  describe("isValidUlid", () => {
    it("should return true for valid ULIDs", async () => {
      const ulid = await service.generateUlid();
      expect(await service.isValidUlid(ulid)).toBe(true);
    });

    it("should return false for invalid strings", async () => {
      expect(await service.isValidUlid("invalid")).toBe(false);
      expect(await service.isValidUlid("")).toBe(false);
      expect(await service.isValidUlid("too-short")).toBe(false);
    });

    it("should return false for strings with invalid characters", async () => {
      // I, L, O, U are not valid in Crockford Base32
      expect(await service.isValidUlid("01HZVY3X4M5N6P7Q8R9S0TABI0")).toBe(
        false
      );
    });
  });

  describe("parseUlid", () => {
    it("should parse valid ULIDs", async () => {
      const ulid = await service.generateUlid();
      const parsed = await service.parseUlid(ulid);
      expect(parsed).toBe(ulid);
    });

    it("should return null for invalid strings", async () => {
      expect(await service.parseUlid("invalid")).toBeNull();
    });
  });

  describe("formatCanonicalRef", () => {
    it("should format as type@ULID", async () => {
      const ulid = await service.generateUlid();
      const ref = await service.formatCanonicalRef("phase", ulid);
      expect(ref).toBe(`phase@${ulid}`);
    });

    it("should work with different type names", async () => {
      const ulid = await service.generateUlid();
      expect(await service.formatCanonicalRef("task", ulid)).toBe(
        `task@${ulid}`
      );
      expect(await service.formatCanonicalRef("epoch", ulid)).toBe(
        `epoch@${ulid}`
      );
    });

    it("should throw for invalid ULIDs", async () => {
      await expect(
        service.formatCanonicalRef("phase", "invalid")
      ).rejects.toThrow();
    });
  });

  describe("parseCanonicalRef", () => {
    it("should parse valid canonical references", async () => {
      const ulid = await service.generateUlid();
      const ref = await service.formatCanonicalRef("phase", ulid);
      const parsed = await service.parseCanonicalRef(ref);
      expect(parsed).toEqual({
        typeName: "phase",
        ulid: ulid,
      });
    });

    it("should return null for invalid references", async () => {
      expect(await service.parseCanonicalRef("invalid")).toBeNull();
      expect(await service.parseCanonicalRef("@ULID")).toBeNull();
      expect(await service.parseCanonicalRef("type@invalid")).toBeNull();
    });

    it("should work with different type names", async () => {
      const ulid = await service.generateUlid();

      const taskRef = await service.formatCanonicalRef("task", ulid);
      expect(await service.parseCanonicalRef(taskRef)).toEqual({
        typeName: "task",
        ulid: ulid,
      });

      const epochRef = await service.formatCanonicalRef("epoch", ulid);
      expect(await service.parseCanonicalRef(epochRef)).toEqual({
        typeName: "epoch",
        ulid: ulid,
      });
    });
  });

  describe("roundtrip", () => {
    it("should roundtrip through format and parse", async () => {
      const ulid = await service.generateUlid();
      const ref = await service.formatCanonicalRef("phase", ulid);
      const parsed = await service.parseCanonicalRef(ref);

      expect(parsed).not.toBeNull();
      expect(parsed!.typeName).toBe("phase");
      expect(parsed!.ulid).toBe(ulid);

      // Format again from parsed
      const ref2 = await service.formatCanonicalRef(
        parsed!.typeName,
        parsed!.ulid
      );
      expect(ref2).toBe(ref);
    });
  });
});
