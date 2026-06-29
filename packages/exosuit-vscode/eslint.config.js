import tseslint from "@typescript-eslint/eslint-plugin";
import tsParser from "@typescript-eslint/parser";

import path from "node:path";
import { fileURLToPath } from "node:url";

import exosuit from "./eslint-rules/exosuit.js";

const tsconfigRootDir = path.dirname(fileURLToPath(import.meta.url));

// ---------------------------------------------------------------------------
// Private field enforcement
//
// Enforce JS-native #field syntax over the `private`/`protected`/`public`
// keyword. Omit `public` (it's the default). No parameter properties.
// ---------------------------------------------------------------------------

const noAccessibilityKeywordOnField = {
  selector:
    "PropertyDefinition[accessibility='private'], PropertyDefinition[accessibility='protected'], PropertyDefinition[accessibility='public']",
  message:
    "Don't use `private`/`protected`/`public` keywords on fields. " +
    "Use JS-native #field for private, and omit `public` (it's the default).",
};

const noAccessibilityKeywordOnMethod = {
  selector:
    "MethodDefinition[accessibility='private'], MethodDefinition[accessibility='protected'], MethodDefinition[accessibility='public']",
  message:
    "Don't use `private`/`protected`/`public` keywords on methods/getters/setters. " +
    "Use JS-native #method for private, and omit `public` (it's the default).",
};

const noParameterProperty = {
  selector: "TSParameterProperty",
  message:
    "Don't use constructor parameter properties (`constructor(private x: T)`). " +
    "Declare the field explicitly with #field syntax and assign in the constructor body.",
};

// Shared rules for all TypeScript files
const sharedRules = {
  // TODO: Re-enable naming-convention once the codebase is cleaned up.
  "@typescript-eslint/naming-convention": "off",

  // Auto-fixable rules as errors for consistent code style
  "@typescript-eslint/consistent-type-imports": [
    "error",
    { prefer: "type-imports", fixStyle: "inline-type-imports" },
  ],
  curly: ["error", "all"],
  eqeqeq: ["error", "always"],
  "prefer-const": "error",
  "no-var": "error",

  "no-throw-literal": "warn",
  semi: "off",

  "exosuit/no-console": "error",

  // CRITICAL: require() breaks VS Code language model tools.
  // The LM tool runtime does not support CommonJS - only ES modules.
  // This rule prevents regressions that would break the core user experience.
  "@typescript-eslint/no-require-imports": "error",

  // Exosuit invariant: agent-context TOML files are machine-owned.
  "exosuit/no-agent-context-toml-writes": "error",
};

export default [
  {
    ignores: ["out/**", "dist/**", "**/*.d.ts", "playground/**"],
  },
  // Extension source (excludes webview - covered by separate config block)
  {
    files: ["src/**/*.ts"],
    ignores: ["src/webview/**/*.ts"],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: 2020,
        sourceType: "module",

        // Type-aware linting using projectService (typescript-eslint v8+)
        // More efficient than explicit project paths - auto-discovers tsconfig
        projectService: true,
        tsconfigRootDir,
      },
    },
    plugins: {
      "@typescript-eslint": tseslint,
      exosuit,
    },
    rules: sharedRules,
  },
  // Enforce JS-native #field syntax on new/migrated files.
  // Added as files are migrated to TracedProvider; will eventually cover all src/.
  {
    files: [
      "src/services/TracedProvider.ts",
      "src/services/TraceCache.ts",
      "src/IdeasTreeProvider.ts",
      "src/EpochContextProvider.ts",
      "src/PhaseDetailsProvider.ts",
      "src/RfcPipelineProvider.ts",
    ],
    rules: {
      "no-restricted-syntax": [
        "error",
        noAccessibilityKeywordOnField,
        noAccessibilityKeywordOnMethod,
        noParameterProperty,
      ],
    },
  },
  // Webview source (uses tsconfig.webview.json)
  {
    files: ["src/webview/**/*.ts"],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: 2020,
        sourceType: "module",
        project: "./tsconfig.webview.json",
        tsconfigRootDir,
      },
    },
    plugins: {
      "@typescript-eslint": tseslint,
      exosuit,
    },
    rules: sharedRules,
  },
  {
    files: ["tests/e2e/**/*.ts"],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: 2020,
        sourceType: "module",
      },
    },
    plugins: {
      "@typescript-eslint": tseslint,
      exosuit,
    },
    rules: {
      // E2E tests must treat the extension as a black box.
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["src/**"],
              message:
                "E2E tests should treat the extension as a black box. Do not import from src/.",
            },
          ],
        },
      ],

      // E2E tests are allowed to seed context files.
      "exosuit/no-agent-context-toml-writes": "off",
    },
  },
  {
    files: ["tests/e2e/lib/**"],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: 2020,
        sourceType: "module",
      },
    },
    plugins: {
      "@typescript-eslint": tseslint,
      exosuit,
    },
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["src/**", "**/src/**"],
              message:
                "The test library must not depend on extension source code. We are maintaining strict boundaries to facilitate future extraction without the overhead of a separate package (The Middle Way).",
            },
          ],
        },
      ],
      "exosuit/no-agent-context-toml-writes": "off",
    },
  },
  {
    files: ["src/test/**/*.ts"],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: 2020,
        sourceType: "module",
      },
    },
    plugins: {
      "@typescript-eslint": tseslint,
      exosuit,
    },
    rules: {},
  },
  {
    files: ["tests/e2e/**/*.test.ts"],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: 2020,
        sourceType: "module",
      },
    },
    plugins: {
      "@typescript-eslint": tseslint,
      exosuit,
    },
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["src/**"],
              message:
                "E2E tests should treat the extension as a black box. Do not import from src/.",
            },
          ],
          paths: [
            {
              name: "@playwright/test",
              importNames: ["test"],
              message:
                "E2E tests must use the Exosuit Electron fixtures so VS Code is actually launched. Replace `import { test } from '@playwright/test'` with `import { test } from './fixtures'`. You can continue importing `expect`/types from `@playwright/test`.",
            },
          ],
        },
      ],
      "exosuit/no-agent-context-toml-writes": "off",
    },
  },
];
