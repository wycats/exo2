// ESM ESLint rule (this package is type:module)

function isStringLiteral(node) {
  return node && node.type === "Literal" && typeof node.value === "string";
}

function isTemplateLiteral(node) {
  return node && node.type === "TemplateLiteral";
}

function collectStrings(node, acc) {
  if (!node) return;

  if (isStringLiteral(node)) {
    acc.push(node.value);
    return;
  }

  if (isTemplateLiteral(node)) {
    for (const quasi of node.quasis || []) {
      if (quasi && quasi.value && typeof quasi.value.cooked === "string") {
        acc.push(quasi.value.cooked);
      }
    }
    return;
  }

  // path.join("docs", "agent-context", "feedback.toml")
  if (
    node.type === "CallExpression" &&
    node.callee &&
    node.callee.type === "MemberExpression" &&
    node.callee.property &&
    node.callee.property.type === "Identifier" &&
    (node.callee.property.name === "join" ||
      node.callee.property.name === "joinPath")
  ) {
    for (const arg of node.arguments || []) {
      collectStrings(arg, acc);
    }
    return;
  }

  if (node.type === "BinaryExpression") {
    collectStrings(node.left, acc);
    collectStrings(node.right, acc);
    return;
  }

  if (node.type === "ArrayExpression") {
    for (const el of node.elements || []) collectStrings(el, acc);
    return;
  }

  if (node.type === "ObjectExpression") {
    for (const prop of node.properties || []) {
      if (prop && prop.type === "Property") collectStrings(prop.value, acc);
    }
    return;
  }

  if (node.type === "CallExpression") {
    for (const arg of node.arguments || []) collectStrings(arg, acc);
  }
}

function forbiddenKind(strings) {
  for (const s of strings) {
    if (!s) continue;

    const normalized = String(s).replace(/\\/g, "/");

    // Narrow: feedback.toml
    if (normalized.includes("feedback.toml")) return "feedback";

    // Broad: any docs/agent-context/*.toml
    if (
      normalized.includes("docs/agent-context") &&
      normalized.endsWith(".toml")
    ) {
      return "agent_context";
    }
  }

  return null;
}

const WRITE_FN_NAMES = new Set([
  "writeFileSync",
  "writeFile",
  "appendFileSync",
  "appendFile",
  // Local helper used in the welcome wizard.
  "createFile",
]);

const WRITE_METHOD_NAMES = new Set([
  "writeFileSync",
  "writeFile",
  "appendFileSync",
  "appendFile",
  "createFile",
]);

function unwrapChain(node) {
  if (!node) return node;
  // Optional chaining comes through as ChainExpression in some parsers.
  if (node.type === "ChainExpression") return node.expression;
  return node;
}

function getMemberChain(node) {
  // Returns { root: <Identifier name | null>, chain: ["prop", ...] } for
  // MemberExpression chains like fs.promises.writeFile.
  // Only supports non-computed member access.
  const out = { root: null, chain: [] };
  let cur = unwrapChain(node);

  while (cur && cur.type === "MemberExpression" && !cur.computed) {
    const prop = cur.property;
    if (!prop || prop.type !== "Identifier") return null;
    out.chain.unshift(prop.name);
    cur = unwrapChain(cur.object);
  }

  if (cur && cur.type === "Identifier") {
    out.root = cur.name;
    return out;
  }

  return null;
}

function normalizeImportSource(source) {
  // Handle both "fs" and "node:fs".
  if (!source || typeof source !== "string") return source;
  return source.startsWith("node:") ? source.slice("node:".length) : source;
}

function getCalleeName(node) {
  if (!node) return null;
  if (node.type === "Identifier") return node.name;
  if (node.type === "MemberExpression" && !node.computed) {
    if (node.property && node.property.type === "Identifier") {
      return node.property.name;
    }
  }
  return null;
}

const rule = {
  meta: {
    type: "problem",
    docs: {
      description:
        "Disallow direct writes to docs/agent-context/*.toml (machine-owned artifacts)",
    },
    schema: [],
    messages: {
      forbiddenFeedback:
        'Do not write docs/agent-context/feedback.toml directly from TS/JS. Feedback is machine-owned.\n\nSTEERING (machine channel v1):\n- next_call = {"kind":"call","params":{"address":{"kind":"operation","path":["feedback","thread","create"]},"input":{"target_file":"<relative path>","target_id":"<id>","target_field":"","status":"open","author":"user","content":"..."}}}}\n- reply:  path=["feedback","thread","reply"], input={thread_id, author, content}\n- status: path=["feedback","thread","status"], input={thread_id, status}\n\nIn this repo, call exo via packages/exosuit-vscode/src/agent/lmtool/machineChannel.ts (exoMachineChannel).',
      forbiddenAgentContext:
        'Do not write docs/agent-context/*.toml directly from TS/JS. These files are machine-owned (CLI/machine channel).\n\nSTEERING (discover the right operation):\n- next_call = {"kind":"help","params":{"address":{"kind":"root"}}}\n\nThen follow the returned namespaces/operations (and any steering.next_call) instead of writing files from the extension.',
    },
  },

  create(context) {
    // Import-aware detection: track local bindings so we can detect writes even
    // when functions are imported/aliased.
    const namespaceImports = new Map();
    // localName -> { source: "fs"|"fs/promises"|"vscode"|..., imported: "writeFile"|... }
    const namedImports = new Map();
    // localName -> module source from `require()` (best-effort)
    const requireNamespaces = new Map();

    const services = context.parserServices;
    const checker =
      services &&
      services.program &&
      typeof services.program.getTypeChecker === "function"
        ? services.program.getTypeChecker()
        : null;

    function recordImportDeclaration(node) {
      const source = normalizeImportSource(node.source && node.source.value);
      if (!source) return;

      for (const spec of node.specifiers || []) {
        if (!spec) continue;
        if (spec.type === "ImportNamespaceSpecifier") {
          namespaceImports.set(spec.local.name, source);
        } else if (spec.type === "ImportSpecifier") {
          const imported = spec.imported;
          const importedName =
            imported && imported.type === "Identifier" ? imported.name : null;
          if (!importedName) continue;
          namedImports.set(spec.local.name, { source, imported: importedName });
        }
      }
    }

    function recordRequireDeclarator(node) {
      // const fs = require("fs")
      // const vscode = require("vscode")
      if (!node || node.type !== "VariableDeclarator") return;
      if (!node.init || node.init.type !== "CallExpression") return;
      if (!node.init.callee || node.init.callee.type !== "Identifier") return;
      if (node.init.callee.name !== "require") return;
      const arg0 = node.init.arguments && node.init.arguments[0];
      if (!isStringLiteral(arg0)) return;
      const source = normalizeImportSource(arg0.value);

      if (node.id && node.id.type === "Identifier") {
        requireNamespaces.set(node.id.name, source);
      }
    }

    function typeAwareIsWriteCall(callNode) {
      if (!checker || !services || !services.esTreeNodeToTSNodeMap)
        return false;
      const callee = unwrapChain(callNode.callee);
      if (!callee) return false;

      const tsNode = services.esTreeNodeToTSNodeMap.get(callee);
      if (!tsNode) return false;

      const symbol = checker.getSymbolAtLocation(tsNode);
      if (!symbol) return false;

      const name = symbol.getName();
      if (!WRITE_METHOD_NAMES.has(name)) return false;

      const decls = symbol.getDeclarations ? symbol.getDeclarations() : [];
      for (const d of decls || []) {
        const file = d.getSourceFile && d.getSourceFile();
        const fileName =
          file && typeof file.fileName === "string" ? file.fileName : "";

        // Node fs typings
        if (
          fileName.includes("node_modules") &&
          fileName.includes("@types/node")
        ) {
          // Examples:
          // - .../@types/node/fs.d.ts
          // - .../@types/node/fs/promises.d.ts
          if (fileName.includes("/fs.d.ts") || fileName.includes("\\\\fs.d.ts"))
            return true;
          if (
            fileName.includes("/fs/promises") ||
            fileName.includes("\\\\fs\\\\promises")
          )
            return true;
        }

        // VS Code API typings (workspace.fs.writeFile)
        if (
          fileName.includes("node_modules") &&
          fileName.includes("@types/vscode")
        ) {
          return true;
        }
      }

      return false;
    }

    function isLikelyWriteCall(callNode) {
      const callee = unwrapChain(callNode.callee);
      if (!callee) return false;

      // Type-aware check is the most robust (handles aliases).
      if (typeAwareIsWriteCall(callNode)) return true;

      // Identifier call: writeFileSync(...)
      if (callee.type === "Identifier") {
        if (WRITE_FN_NAMES.has(callee.name)) return true;

        const named = namedImports.get(callee.name);
        if (named) {
          const source = named.source;
          const imported = named.imported;

          if (WRITE_METHOD_NAMES.has(imported)) {
            if (source === "fs" || source === "fs/promises") return true;
          }
        }
        return false;
      }

      // MemberExpression call: fs.promises.writeFile(...), vscode.workspace.fs.writeFile(...)
      if (callee.type === "MemberExpression") {
        const chainInfo = getMemberChain(callee);
        if (!chainInfo) return false;

        const { root, chain } = chainInfo;
        const last = chain[chain.length - 1];
        if (!WRITE_METHOD_NAMES.has(last)) return false;

        const rootSource =
          namespaceImports.get(root) || requireNamespaces.get(root) || null;

        const rootNamed = namedImports.get(root);

        // fs.* and fs.promises.*
        if (rootSource === "fs" || rootSource === "fs/promises") {
          return true;
        }

        // fsPromises.writeFile where fsPromises was imported as `import { promises as fsPromises } from "fs"`
        if (
          rootNamed &&
          rootNamed.source === "fs" &&
          rootNamed.imported === "promises" &&
          (last === "writeFile" || last === "appendFile")
        ) {
          return true;
        }

        if (rootSource === "vscode") {
          // vscode.workspace.fs.writeFile
          if (chain.join(".") === "workspace.fs.writeFile") return true;
          return false;
        }

        // workspace.fs.writeFile where workspace was imported from vscode.
        if (root === "workspace" && chain.join(".") === "fs.writeFile") {
          const maybeWorkspace = namedImports.get("workspace");
          if (maybeWorkspace && maybeWorkspace.source === "vscode") return true;

          // Best-effort fallback: treat workspace.fs.writeFile as a write call.
          return true;
        }

        return false;
      }

      return false;
    }

    function isAllowedBootstrapWrite() {
      const filename = context.getFilename ? context.getFilename() : "";
      const normalized = String(filename).replace(/\\/g, "/");
      if (
        !normalized.endsWith(
          "/packages/exosuit-vscode/src/DashboardProvider.ts"
        )
      ) {
        return false;
      }

      // Allow the welcome wizard initializer to seed initial context.
      // (This is a controlled exception; most other code should use exo.)
      const ancestors = context.getAncestors();
      for (const a of ancestors) {
        if (!a) continue;
        if (
          a.type === "FunctionDeclaration" &&
          a.id?.name === "initializeProject"
        ) {
          return true;
        }
        if (
          a.type === "FunctionExpression" &&
          a.id?.name === "initializeProject"
        ) {
          return true;
        }
        if (
          a.type === "MethodDefinition" &&
          a.key?.name === "initializeProject"
        ) {
          return true;
        }
      }

      return false;
    }

    return {
      ImportDeclaration(node) {
        recordImportDeclaration(node);
      },
      VariableDeclarator(node) {
        recordRequireDeclarator(node);
      },
      CallExpression(node) {
        if (!isLikelyWriteCall(node)) return;

        const strings = [];
        // Most write APIs use the first arg as the path/uri.
        if (node.arguments && node.arguments.length > 0) {
          collectStrings(node.arguments[0], strings);
        }

        // Also scan remaining args in case the path is nested (rare).
        for (let i = 1; i < (node.arguments || []).length; i++) {
          collectStrings(node.arguments[i], strings);
        }

        const kind = forbiddenKind(strings);
        if (!kind) return;

        if (isAllowedBootstrapWrite()) return;

        context.report({
          node,
          messageId:
            kind === "feedback" ? "forbiddenFeedback" : "forbiddenAgentContext",
        });
      },
    };
  },
};

export default rule;
