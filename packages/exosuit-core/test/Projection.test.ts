import { expect } from "vitest";
import { stableJsonStringify, truncateWithNotice } from "../src/projection.ts";

describe("projection", () => {
  describe("stableJsonStringify", () => {
    it("sorts object keys recursively", () => {
      const input = {
        b: 2,
        a: 1,
        nested: {
          z: true,
          y: [
            { d: 4, c: 3 },
            { b: 2, a: 1 },
          ],
        },
      };

      const out = stableJsonStringify(input, 2);

      // Keys should be ordered a, b, nested at the top level.
      expect(out.indexOf('"a": 1')).to.be.lessThan(out.indexOf('"b": 2'));
      expect(out.indexOf('"b": 2')).to.be.lessThan(out.indexOf('"nested"'));

      // Nested object keys should be ordered y, z.
      expect(out.indexOf('"y"')).to.be.lessThan(out.indexOf('"z"'));

      // Array element objects should have keys ordered.
      expect(out).to.contain('{\n        "c": 3,\n        "d": 4\n      }');
      expect(out).to.contain('{\n        "a": 1,\n        "b": 2\n      }');
    });
  });

  describe("truncateWithNotice", () => {
    it("does not truncate when within budget", () => {
      const r = truncateWithNotice("hello", 10);
      expect(r.truncated).to.equal(false);
      expect(r.text).to.equal("hello");
    });

    it("truncates and appends a notice when over budget", () => {
      const r = truncateWithNotice("abcdefghij", 5, { ellipsis: "..." });
      expect(r.truncated).to.equal(true);
      expect(r.text.startsWith("ab...")).to.equal(true);
      expect(r.text).to.contain("TRUNCATED");
    });

    it("supports custom notice", () => {
      const r = truncateWithNotice("abcdefghij", 5, {
        ellipsis: "…",
        notice: ({
          originalLength,
          maxChars,
        }: {
          originalLength: number;
          maxChars: number;
        }) =>
          `\n[cut ${originalLength}->${maxChars}]`,
      });
      expect(r.text).to.contain("[cut 10->5]");
    });
  });
});
