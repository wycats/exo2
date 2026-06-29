import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

export default {
  compilerOptions: {
    runes: true,
  },
  // Consult https://svelte.dev/docs#compile-time-svelte-preprocess
  // for more information about preprocessors
  preprocess: vitePreprocess(),
  // Tell svelte-check and Svelte Language Server to use webview tsconfig
  kit: undefined,
  onwarn: (warning, handler) => {
    // Suppress a11y warnings for now
    if (warning.code.startsWith("a11y-")) return;
    handler(warning);
  },
};
