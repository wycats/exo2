import { expect } from "vitest";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";
import { PromptService } from "../src/PromptService.ts";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

describe("PromptService", () => {
  const testDir = path.join(__dirname, "test-env");
  const contextDir = path.join(testDir, "docs", "agent-context");
  const promptsPath = path.join(contextDir, "prompts.toml");

  beforeEach(() => {
    if (fs.existsSync(testDir)) {
      fs.rmSync(testDir, { recursive: true, force: true });
    }
    fs.mkdirSync(contextDir, { recursive: true });
  });

  afterEach(() => {
    if (fs.existsSync(testDir)) {
      fs.rmSync(testDir, { recursive: true, force: true });
    }
  });

  it("should load prompts from prompts.toml", () => {
    const content = `
[test]
greeting = "Hello, {name}!"
`;
    fs.writeFileSync(promptsPath, content);

    const service = new PromptService(testDir);
    const prompt = service.get("test.greeting");
    expect(prompt).to.equal("Hello, {name}!");
  });

  it("should render prompts with variables", () => {
    const content = `
[test]
greeting = "Hello, {name}!"
`;
    fs.writeFileSync(promptsPath, content);

    const service = new PromptService(testDir);
    const rendered = service.render("test.greeting", { name: "World" });
    expect(rendered).to.equal("Hello, World!");
  });

  it("should return undefined for missing keys", () => {
    const service = new PromptService(testDir);
    const prompt = service.get("missing.key");
    expect(prompt).to.be.undefined;
  });

  it("should throw error when rendering missing key", () => {
    const service = new PromptService(testDir);
    expect(() => service.render("missing.key")).to.throw(
      "Prompt not found: missing.key",
    );
  });

  it("should handle nested keys", () => {
    const content = `
[section]
[section.subsection]
key = "value"
`;
    fs.writeFileSync(promptsPath, content);

    const service = new PromptService(testDir);
    const prompt = service.get("section.subsection.key");
    expect(prompt).to.equal("value");
  });

  it("should preserve missing variables during interpolation", () => {
    const content = `
[test]
greeting = "Hello, {name} {missing}!"
`;
    fs.writeFileSync(promptsPath, content);

    const service = new PromptService(testDir);
    const rendered = service.render("test.greeting", { name: "World" });
    expect(rendered).to.equal("Hello, World {missing}!");
  });

  it("should fall back to empty prompts when prompts.toml is invalid", () => {
    const content = `
[test
broken = "nope"
`;
    fs.writeFileSync(promptsPath, content);

    const originalError = console.error;
    console.error = () => undefined;

    const service = new PromptService(testDir);
    const prompt = service.get("test.broken");

    console.error = originalError;

    expect(prompt).to.be.undefined;
  });
});
