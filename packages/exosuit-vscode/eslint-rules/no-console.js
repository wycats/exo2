const MESSAGE =
  "Console output is forbidden. Use the shared logger instead: import { getLogger } from '../logging' (extension) or initialize/get a webview logger from '../webview/lib/logger'.";

export default {
  meta: {
    type: "problem",
    docs: {
      description: "Disallow console usage in favor of Exosuit logger",
      recommended: true,
    },
    messages: {
      noConsole: MESSAGE,
    },
    schema: [],
  },
  create(context) {
    return {
      CallExpression(node) {
        const callee = node.callee;
        if (callee?.type !== "MemberExpression") {
          return;
        }

        const object = callee.object;
        if (object?.type !== "Identifier" || object.name !== "console") {
          return;
        }

        context.report({
          node,
          messageId: "noConsole",
        });
      },
    };
  },
};
